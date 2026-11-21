//! Read models: combine per-machine state into what the dashboard shows.
//! All functions are pure — fed by `PersistedState` + live-session info.

use crate::state::{MachineState, PersistedState};
use crate::stats::Stats;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

pub struct View<'a> {
    pub state: &'a PersistedState,
    /// session_id -> last-seen epoch (live detection window)
    pub last_seen: &'a HashMap<String, i64>,
    pub now: i64,
    pub today: String,
    /// last 14 local-tz day keys, oldest first
    pub last14: Vec<String>,
}

#[derive(Serialize)]
pub struct SessionRow {
    pub id: String,
    pub title: String,
    pub machine: String,
    pub in_out: i64,
    pub live: bool,
}

#[derive(Serialize)]
pub struct DayBar {
    pub date: String,
    pub local: i64,
    pub remote: i64,
}

#[derive(Serialize)]
pub struct Render {
    pub today: Stats,
    pub local_today: Stats,
    pub remote_today: Stats,
    pub week: Stats,
    pub all_time: Stats,
    pub hours: Vec<i64>,       // 24 in+out buckets (local+remote merged)
    pub days14: Vec<DayBar>,   // 14-day history for the stacked chart
    pub live: Vec<SessionRow>, // currently active sessions
    pub top_sessions: Vec<SessionRow>,
    pub models: Vec<(String, i64)>,
    pub projects: Vec<(String, i64)>,
    pub machines: Vec<MachineInfo>,
}

#[derive(Serialize)]
pub struct MachineInfo {
    pub name: String,
    pub today_in_out: i64,
    pub ok: bool,
    /// seconds since last successful poll; None for local
    pub stale_secs: Option<i64>,
    pub error: Option<String>,
}

const LIVE_WINDOW_SECS: i64 = 15;

impl<'a> View<'a> {
    pub fn new(
        state: &'a PersistedState,
        last_seen: &'a HashMap<String, i64>,
        today: String,
        last14: Vec<String>,
        now: i64,
    ) -> Self {
        Self {
            state,
            last_seen,
            now,
            today,
            last14,
        }
    }

    fn is_live(&self, sid: &str) -> bool {
        self.last_seen
            .get(sid)
            .map(|t| self.now - t <= LIVE_WINDOW_SECS)
            .unwrap_or(false)
    }

    fn title_of(&self, m: &MachineState, sid: &str) -> String {
        m.titles
            .get(sid)
            .filter(|t| !t.is_empty())
            .cloned()
            .unwrap_or_else(|| sid.chars().take(12).collect())
    }

    fn combined_today(&self) -> Stats {
        let mut s = self
            .state
            .local
            .days
            .get(&self.today)
            .cloned()
            .unwrap_or_default();
        for m in self.state.remotes.values() {
            if let Some(d) = m.days.get(&self.today) {
                s.merge(d);
            }
        }
        s
    }

    fn sum_range(&self, days: usize) -> Stats {
        let mut s = Stats::default();
        let keys: Vec<String> = self.last14.iter().rev().take(days).cloned().collect();
        for key in &keys {
            if let Some(d) = self.state.local.days.get(key) {
                s.merge(d);
            }
            for m in self.state.remotes.values() {
                if let Some(d) = m.days.get(key) {
                    s.merge(d);
                }
            }
        }
        s
    }

    fn sum_all(&self) -> Stats {
        let mut s = Stats::default();
        for d in self.state.local.days.values() {
            s.merge(d);
        }
        for m in self.state.remotes.values() {
            for d in m.days.values() {
                s.merge(d);
            }
        }
        s
    }

    fn remote_today(&self) -> Stats {
        let mut s = Stats::default();
        for m in self.state.remotes.values() {
            if let Some(d) = m.days.get(&self.today) {
                s.merge(d);
            }
        }
        s
    }

    /// Merge top-level maps keyed the same way (models / projects / sessions).
    fn merged_map(
        &self,
        pick: impl Fn(&MachineState) -> Option<&BTreeMap<String, Stats>>,
    ) -> Vec<(String, i64)> {
        let mut acc: HashMap<String, Stats> = HashMap::new();
        let mut fold = |m: &MachineState| {
            if let Some(map) = pick(m) {
                for (k, v) in map {
                    acc.entry(k.clone()).or_default().merge(v);
                }
            }
        };
        fold(&self.state.local);
        for m in self.state.remotes.values() {
            fold(m);
        }
        let mut v: Vec<_> = acc.into_iter().map(|(k, s)| (k, s.in_plus_out())).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }

    pub fn render(&self) -> Render {
        let today = self.combined_today();
        let local_today = self
            .state
            .local
            .days
            .get(&self.today)
            .cloned()
            .unwrap_or_default();
        let remote_today = self.remote_today();

        // 24h merged buckets
        let mut hours = vec![0i64; 24];
        if self.state.local.hours_date == self.today {
            for (h, s) in &self.state.local.hours {
                if let Ok(i) = h.parse::<usize>() {
                    if i < 24 {
                        hours[i] += s.in_plus_out();
                    }
                }
            }
        }
        for m in self.state.remotes.values() {
            if m.hours_date == self.today {
                for (h, s) in &m.hours {
                    if let Ok(i) = h.parse::<usize>() {
                        if i < 24 {
                            hours[i] += s.in_plus_out();
                        }
                    }
                }
            }
        }

        // 14-day stacked series
        let days14: Vec<DayBar> = self
            .last14
            .iter()
            .map(|d| {
                let local = self
                    .state
                    .local
                    .days
                    .get(d)
                    .map(|s| s.in_plus_out())
                    .unwrap_or(0);
                let remote = self
                    .state
                    .remotes
                    .values()
                    .map(|m| m.days.get(d).map(|s| s.in_plus_out()).unwrap_or(0))
                    .sum();
                DayBar {
                    date: d.clone(),
                    local,
                    remote,
                }
            })
            .collect();

        // sessions: merge local + every remote, tagged by machine
        let mut sessions: Vec<SessionRow> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        let collect = |m: &MachineState,
                       machine: &str,
                       sessions: &mut Vec<SessionRow>,
                       seen: &mut HashSet<String>| {
            if let Some(day) = m.day_sessions.get(&self.today) {
                for (sid, s) in day {
                    if !seen.insert(sid.clone()) {
                        continue;
                    }
                    sessions.push(SessionRow {
                        id: sid.clone(),
                        title: self.title_of(m, sid),
                        machine: machine.to_string(),
                        in_out: s.in_plus_out(),
                        live: self.is_live(sid),
                    });
                }
            }
        };
        collect(&self.state.local, "mac", &mut sessions, &mut seen);
        for (name, m) in &self.state.remotes {
            collect(m, name, &mut sessions, &mut seen);
        }
        sessions.sort_by(|a, b| b.in_out.cmp(&a.in_out));
        let live: Vec<SessionRow> = sessions
            .iter()
            .filter(|s| s.live)
            .map(|s| SessionRow {
                id: s.id.clone(),
                title: s.title.clone(),
                machine: s.machine.clone(),
                in_out: s.in_out,
                live: true,
            })
            .collect();

        let models = self.merged_map(|m| m.day_models.get(&self.today));
        let projects = self.merged_map(|m| m.day_projects.get(&self.today));

        let mut machines = vec![MachineInfo {
            name: "mac".into(),
            today_in_out: local_today.in_plus_out(),
            ok: true,
            stale_secs: None,
            error: None,
        }];
        for (name, m) in &self.state.remotes {
            machines.push(MachineInfo {
                name: name.clone(),
                today_in_out: m
                    .days
                    .get(&self.today)
                    .map(|s| s.in_plus_out())
                    .unwrap_or(0),
                ok: m.last_error.is_none(),
                stale_secs: m.last_ok_at.map(|t| self.now - t),
                error: m.last_error.clone(),
            });
        }

        Render {
            today,
            local_today,
            remote_today,
            week: self.sum_range(7),
            all_time: self.sum_all(),
            hours,
            days14,
            live,
            top_sessions: sessions.into_iter().take(6).collect(),
            models: models.into_iter().take(5).collect(),
            projects: projects.into_iter().take(6).collect(),
            machines,
        }
    }
}
