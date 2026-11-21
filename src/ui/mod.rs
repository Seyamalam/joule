//! macOS menu bar app. A `NSStatusItem` (icon = live 24h sparkline) opens an
//! `NSPopover` hosting a `WKWebView` that renders `assets/dashboard.html`.
//! A refresh thread pushes a JSON `Render` into the page via `render(json)`
//! every few seconds — no reloading, no flicker.

mod app;

pub use app::run;
