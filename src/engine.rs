//! The polling engine. One background loop drives local sqlite polls (every
//! few seconds) plus per-remote ssh polls on their own schedules with
//! exponential backoff. UI and `stats` subcommand read snapshots from it.

use crate::config::Config;
use crate::remote::{self, Remote};
use crate::state::PersistedState;
use crate::{db, tailscale};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DISCOVERY_EVERY: Duration = Duration::from_secs(600);

pub struct Engine {
    pub state: Mutex<PersistedState>,
    /// session_id -> epoch of last row seen (live detection)
    pub last_seen: Mutex<HashMap<String, i64>>,
    pub config: Config,
    remotes: Mutex<Vec<Remote>>,
    db_path: PathBuf,
    tz: String,
    stop: AtomicBool,
}

fn epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn day_of(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default()
}

fn hour_of(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%-H").to_string())
        .unwrap_or_default()
}

fn today() -> String {
    day_of(epoch())
}

// pub wrappers for CLI subcommands
pub fn now_pub() -> i64 {
    epoch()
}
pub fn today_pub() -> String {
    today()
}
pub fn day_of_pub() -> impl Fn(i64) -> String {
    day_of
}
pub fn hour_of_pub() -> impl Fn(i64) -> String {
    hour_of
}

pub fn last_n_days(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let t = epoch() - (i as i64) * 86400;
            day_of(t)
        })
        .rev()
        .collect()
}

impl Engine {
    pub fn new(db_path: PathBuf) -> Self {
        let config = Config::load();
        let state = PersistedState::load();
        let remotes: Vec<Remote> = config
            .remotes
            .iter()
            .filter(|(_, r)| !r.disabled)
            .map(|(name, r)| Remote::new(name.clone(), r.target.clone(), false))
            .collect();
        // Previously discovered remotes persist across restarts.
        for name in &state.discovered {
            if !remotes.iter().any(|r| &r.name == name) {
                // We don't know its target anymore; rediscovery re-adds it.
                eprintln!("devin-usage: discovered remote '{name}' needs re-discovery");
            }
        }
        Self {
            state: Mutex::new(state),
            last_seen: Mutex::new(HashMap::new()),
            config,
            remotes: Mutex::new(remotes),
            db_path,
            tz: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into()),
            stop: AtomicBool::new(false),
        }
    }

    /// Take a consistent snapshot for rendering.
    pub fn snapshot(&self) -> (PersistedState, HashMap<String, i64>) {
        (
            clone_state(&self.state.lock().unwrap()),
            self.last_seen.lock().unwrap().clone(),
        )
    }

    /// Local-only quick render for the `stats` subcommand.
    pub fn poll_local_once(&self) -> Result<()> {
        self.poll_local()
    }

    fn poll_local(&self) -> Result<()> {
        let con = db::open(&self.db_path)?;
        let mut st = self.state.lock().unwrap();
        let last = st.local.last_row_id;
        let rows = db::fetch_rows(&con, last)?;
        if !rows.is_empty() {
            let now = epoch();
            let t = today();
            if st.local.hours_date != t {
                st.local.hours.clear();
                st.local.hours_date = t;
            }
            for r in &rows {
                let proj = st.local.projects.get(&r.session_id).cloned();
                st.local.fold(
                    &day_of(r.created_at),
                    &hour_of(r.created_at),
                    &r.session_id,
                    &r.model,
                    r.input,
                    r.output,
                    r.cache_read,
                    r.cache_create,
                    r.context,
                    proj.as_deref(),
                );
                st.local.last_row_id = st.local.last_row_id.max(r.row_id);
                self.last_seen
                    .lock()
                    .unwrap()
                    .insert(r.session_id.clone(), now);
            }
            st.save();
        }
        drop(st);
        // Titles/projects are cheap to refresh and sessions rename often.
        let (titles, projects) = (db::fetch_titles(&con)?, db::fetch_projects(&con)?);
        let mut st = self.state.lock().unwrap();
        st.local.titles.extend(titles);
        st.local.projects.extend(projects);
        Ok(())
    }

    /// Kick off background workers. Returns a handle to stop them.
    pub fn start(self: Arc<Self>) {
        // Local baseline on first run (no history yet).
        if self.state.lock().unwrap().local.last_row_id == 0 {
            let me = self.clone();
            thread::spawn(move || {
                if let Ok(con) = db::open(&me.db_path) {
                    if let Ok(m) = db::baseline(&con, &day_of, &hour_of) {
                        me.state.lock().unwrap().local = m;
                        me.state.lock().unwrap().save();
                    }
                }
            });
        }

        // Backfill any remote we don't have state for yet.
        for r in self
            .remotes
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.name.clone())
            .collect::<Vec<_>>()
        {
            if !self.state.lock().unwrap().remotes.contains_key(&r) {
                self.spawn_backfill(r);
            }
        }

        // Tailscale discovery.
        if self.config.tailscale.enabled {
            let me = self.clone();
            thread::spawn(move || me.discovery_loop());
        }

        // Main loop: local poll on cadence, remotes on their own backoff.
        let me = self.clone();
        thread::spawn(move || {
            let mut next_local = Instant::now();
            while !me.stop.load(Ordering::Relaxed) {
                if Instant::now() >= next_local {
                    let _ = me.poll_local();
                    next_local = Instant::now() + Duration::from_secs(me.config.local_poll_secs);
                }
                me.due_remotes();
                thread::sleep(Duration::from_millis(500));
            }
        });
    }

    /// Spawn ssh polls for remotes whose schedule has arrived.
    fn due_remotes(self: &Arc<Self>) {
        let due: Vec<String> = {
            let remotes = self.remotes.lock().unwrap();
            remotes
                .iter()
                .filter(|r| Instant::now() >= r.next_poll_at)
                .map(|r| r.name.clone())
                .collect()
        };
        for name in due {
            let (target, base) = {
                let mut remotes = self.remotes.lock().unwrap();
                let r = remotes.iter_mut().find(|r| r.name == name).unwrap();
                // Push next_poll_at forward now to prevent double-firing.
                r.next_poll_at = Instant::now() + r.backoff(self.config.remote_poll_secs);
                (r.target.clone(), self.config.remote_poll_secs)
            };
            let me = self.clone();
            let remote = Remote::new(name.clone(), target, false);
            thread::spawn(move || {
                // Take the state out so we don't hold the lock over ssh.
                let mut m = {
                    let mut st = me.state.lock().unwrap();
                    std::mem::take(st.remotes.entry(name.clone()).or_default())
                };
                let res = remote::poll(&remote, &mut m, &day_of, &hour_of);
                let failed = res.is_err();
                if let Ok(sids) = &res {
                    if !sids.is_empty() {
                        let now = epoch();
                        let mut seen = me.last_seen.lock().unwrap();
                        for sid in sids {
                            seen.insert(sid.clone(), now);
                        }
                    }
                }
                {
                    let mut st = me.state.lock().unwrap();
                    *st.remotes.entry(name.clone()).or_default() = m;
                    if !failed {
                        st.save();
                    }
                }
                let mut remotes = me.remotes.lock().unwrap();
                if let Some(r) = remotes.iter_mut().find(|r| r.name == name) {
                    if failed {
                        r.fail_count = r.fail_count.saturating_add(1);
                    } else {
                        r.fail_count = 0;
                    }
                    r.next_poll_at = Instant::now() + r.backoff(base);
                }
            });
        }
    }

    fn spawn_backfill(self: &Arc<Self>, name: String) {
        let target = {
            let remotes = self.remotes.lock().unwrap();
            remotes
                .iter()
                .find(|r| r.name == name)
                .map(|r| r.target.clone())
        };
        let Some(target) = target else { return };
        let remote = Remote::new(name.clone(), target, false);
        let tz = self.tz.clone();
        let me = self.clone();
        thread::spawn(move || match remote::backfill(&remote, &tz) {
            Ok(m) => {
                let mut st = me.state.lock().unwrap();
                st.remotes.insert(name, m);
                st.save();
            }
            Err(e) => {
                let mut st = me.state.lock().unwrap();
                let m = st.remotes.entry(name).or_default();
                m.last_error = Some(format!("backfill: {e}"));
            }
        });
    }

    /// Periodically scan the tailnet for new devin machines.
    fn discovery_loop(self: &Arc<Self>) {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            self.discover_once();
            for _ in 0..DISCOVERY_EVERY.as_secs() {
                if self.stop.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    fn discover_once(self: &Arc<Self>) {
        let existing: Vec<String> = self
            .remotes
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.name.clone())
            .collect();
        let peers = tailscale::discoverable(&self.config.tailscale.ignore, &existing);
        for p in peers {
            let user = self
                .config
                .tailscale
                .ssh_user
                .clone()
                .or_else(|| self.config.ssh_user.clone())
                .unwrap_or_else(|| whoami());
            let target = format!("{user}@{}", p.dns);
            if !remote::probe_has_devin(&target) {
                continue;
            }
            eprintln!("devin-usage: discovered {} on tailnet", p.hostname);
            {
                let mut remotes = self.remotes.lock().unwrap();
                remotes.push(Remote::new(p.hostname.clone(), target, true));
            }
            {
                let mut st = self.state.lock().unwrap();
                if !st.discovered.contains(&p.hostname) {
                    st.discovered.push(p.hostname.clone());
                }
            }
            self.spawn_backfill(p.hostname);
        }
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".into())
}

fn clone_state(s: &PersistedState) -> PersistedState {
    serde_json::from_value(serde_json::to_value(s).unwrap_or_default()).unwrap_or_default()
}
