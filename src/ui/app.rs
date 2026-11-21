use crate::aggregate;
use crate::engine::{self, Engine};
use crate::stats::compact;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{class, define_class, msg_send, sel, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSColor, NSImage, NSPopover, NSPopoverBehavior,
    NSStatusBar, NSStatusItem, NSView, NSViewController,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};
use objc2_web_kit::{
    WKScriptMessage, WKScriptMessageHandler, WKUserContentController, WKWebView,
    WKWebViewConfiguration,
};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DASHBOARD_HTML: &str = include_str!("../../assets/dashboard.html");

// ---------------------------------------------------------------------------
// Obj-C helper objects
// ---------------------------------------------------------------------------

define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DUActionHandler"]
    struct ActionHandler;

    impl ActionHandler {
        /// Status-bar button click -> toggle the popover.
        #[unsafe(method(toggle:))]
        fn toggle(&self, _sender: Option<&AnyObject>) {
            unsafe { ui_mut().toggle_popover() }
        }
    }

    unsafe impl NSObjectProtocol for ActionHandler {}

    /// Lets the dashboard's Quit link reach us via
    /// `window.webkit.messageHandlers.action.postMessage(...)`.
    unsafe impl WKScriptMessageHandler for ActionHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn on_message(&self, _ucc: &WKUserContentController, _msg: &WKScriptMessage) {
            unsafe { ui_mut().quit() }
        }
    }
);

// ---------------------------------------------------------------------------
// UI state (main thread only — never touch from worker threads)
// ---------------------------------------------------------------------------

struct Ui {
    item: Retained<NSStatusItem>,
    popover: Retained<NSPopover>,
    webview: Retained<WKWebView>,
    engine: Arc<Engine>,
}

impl Ui {
    unsafe fn toggle_popover(&mut self) {
        let shown: bool = msg_send![&self.popover, isShown];
        if shown {
            let _: () = msg_send![&self.popover, performClose: std::ptr::null::<AnyObject>()];
        } else if let Some(button) = self.item.button(MainThreadMarker::new_unchecked()) {
            let bounds: NSRect = msg_send![&button, bounds];
            let _: () = msg_send![&self.popover, showRelativeToRect: bounds,
                                   ofView: &*button, preferredEdge: NSRectEdge::MinY];
        }
    }

    unsafe fn update(&mut self, title: &str, json: &str, hours: &[i64]) {
        if let Some(button) = self.item.button(MainThreadMarker::new_unchecked()) {
            button.setTitle(&NSString::from_str(&format!(" {title}")));
            let img = sparkline_image(hours);
            button.setImage(Some(&*img));
        }
        let js = NSString::from_str(&format!("render({json})"));
        let _: Option<Retained<AnyObject>> = msg_send![
            &self.webview, evaluateJavaScript: &*js,
            completionHandler: std::ptr::null::<AnyObject>()
        ];
    }

    unsafe fn quit(&mut self) {
        self.engine.state.lock().unwrap().save();
        std::process::exit(0);
    }
}

static mut UI: Option<Ui> = None;

unsafe fn ui_mut() -> &'static mut Ui {
    (*std::ptr::addr_of_mut!(UI))
        .as_mut()
        .expect("UI not initialized")
}

/// Bitcast a reference to `AnyObject` (for APIs typed as `&AnyObject`).
fn as_any<T>(r: &T) -> &AnyObject {
    unsafe { &*(r as *const T as *const AnyObject) }
}

// ---------------------------------------------------------------------------
// Sparkline: 24h token activity, drawn as a tiny template image
// ---------------------------------------------------------------------------

fn sparkline_image(hours: &[i64]) -> Retained<NSImage> {
    let size = NSSize {
        width: 17.0,
        height: 14.0,
    };
    let allocated: Allocated<NSImage> = unsafe { msg_send![class!(NSImage), alloc] };
    let img: Retained<NSImage> = unsafe { msg_send![allocated, initWithSize: size] };
    unsafe {
        let _: () = msg_send![&img, lockFocus];
        let color: Retained<NSColor> = msg_send![class!(NSColor), labelColor];
        let _: () = msg_send![&color, setFill];
        let max = hours.iter().copied().max().unwrap_or(1).max(1) as f64;
        let w = 16.0 / 24.0;
        for (i, v) in hours.iter().take(24).enumerate() {
            let h = 2.0 + 11.0 * (*v as f64 / max);
            let rect = NSRect {
                origin: NSPoint {
                    x: 0.5 + i as f64 * w,
                    y: 0.0,
                },
                size: NSSize {
                    width: w * 0.7,
                    height: h,
                },
            };
            let _: () = msg_send![class!(NSBezierPath), fillRect: rect];
        }
        let _: () = msg_send![&img, unlockFocus];
        let _: () = msg_send![&img, setTemplate: true];
    }
    img
}

// ---------------------------------------------------------------------------
// App entry
// ---------------------------------------------------------------------------

pub fn run(engine: Arc<Engine>) {
    let mtm = MainThreadMarker::new().expect("must run on main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // --- handler object -----------------------------------------------------
    let handler: Retained<ActionHandler> = unsafe { msg_send![ActionHandler::class(), new] };

    // --- status item --------------------------------------------------------
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
    if let Some(button) = item.button(mtm) {
        unsafe {
            button.setTitle(&NSString::from_str(" …"));
            button.setTarget(Some(as_any(&*handler)));
            button.setAction(Some(sel!(toggle:)));
        }
    }

    // --- popover + webview --------------------------------------------------
    let popover: Retained<NSPopover> = unsafe { msg_send![class!(NSPopover), new] };
    let vc: Retained<NSViewController> = unsafe { msg_send![class!(NSViewController), new] };
    let config: Retained<WKWebViewConfiguration> =
        unsafe { msg_send![class!(WKWebViewConfiguration), new] };
    let ucc: Retained<WKUserContentController> =
        unsafe { msg_send![&config, userContentController] };
    let proto: &ProtocolObject<dyn WKScriptMessageHandler> = ProtocolObject::from_ref(&*handler);
    unsafe {
        ucc.addScriptMessageHandler_name(proto, &NSString::from_str("action"));
    }
    let wv: Retained<WKWebView> = unsafe {
        let allocated: Allocated<WKWebView> = msg_send![class!(WKWebView), alloc];
        msg_send![allocated, initWithFrame: NSRect::ZERO, configuration: &*config]
    };
    unsafe {
        let _: () = msg_send![&wv, setFrameSize: NSSize { width: 420.0, height: 600.0 }];
    }
    let html = NSString::from_str(DASHBOARD_HTML);
    let _: Option<Retained<AnyObject>> =
        unsafe { msg_send![&wv, loadHTMLString: &*html, baseURL: std::ptr::null::<NSURL>()] };
    let view: &NSView = unsafe { &*(&*wv as *const WKWebView as *const NSView) };
    vc.setView(view);
    popover.setContentViewController(Some(&vc));
    popover.setBehavior(NSPopoverBehavior::Transient);

    unsafe {
        UI = Some(Ui {
            item,
            popover,
            webview: wv,
            engine: engine.clone(),
        });
    }

    // --- start the polling engine ------------------------------------------
    let engine2 = engine.clone();
    engine.start();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(3));
        let (st, seen) = engine2.snapshot();
        let view = aggregate::View::new(
            &st,
            &seen,
            engine::today_pub(),
            engine::last_n_days(14),
            engine::now_pub(),
        );
        let render = view.render();
        let title = compact(render.today.in_plus_out());
        let hours: Vec<i64> = render
            .hours_local
            .iter()
            .zip(&render.hours_remote)
            .map(|(l, r)| l + r)
            .collect();
        if let Ok(json) = serde_json::to_string(&render) {
            dispatch::Queue::main().exec_async(move || unsafe {
                ui_mut().update(&title, &json, &hours);
            });
        }
    });

    app.run();
}
