//! Persisted usage state. Serialized to `~/.config/devin-usage/state.json`
//! so the app only ever scans *new* database rows after the first backfill.

use crate::stats::Stats;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Usage aggregates for one machine (this Mac or one remote host).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MachineState {
    #[serde(default)]
    pub last_row_id: i64,
    /// day ("YYYY-MM-DD", local tz) -> totals
    #[serde(default)]
    pub days: BTreeMap<String, Stats>,
    /// day -> model -> totals
    #[serde(default)]
    pub day_models: BTreeMap<String, BTreeMap<String, Stats>>,
    /// day -> session_id -> totals
    #[serde(default)]
    pub day_sessions: BTreeMap<String, BTreeMap<String, Stats>>,
    /// day -> project name -> totals
    #[serde(default)]
    pub day_projects: BTreeMap<String, BTreeMap<String, Stats>>,
    /// hour-of-day ("0".."23") -> totals, for `hours_date` only
    #[serde(default)]
    pub hours: BTreeMap<String, Stats>,
    #[serde(default)]
    pub hours_date: String,
    /// session_id -> session title
    #[serde(default)]
    pub titles: HashMap<String, String>,
    /// session_id -> project name (basename of working_directory)
    #[serde(default)]
    pub projects: HashMap<String, String>,
    /// epoch seconds of last successful poll (remotes only; None = never)
    #[serde(default)]
    pub last_ok_at: Option<i64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl MachineState {
    /// Fold one request row in. `day`/`hour` are pre-bucketed local-tz keys.
    pub fn fold(
        &mut self,
        day: &str,
        hour: &str,
        sid: &str,
        model: &str,
        input: i64,
        output: i64,
        cr: i64,
        cc: i64,
        ctx: i64,
        project: Option<&str>,
    ) {
        self.days
            .entry(day.into())
            .or_default()
            .add(input, output, cr, cc, ctx);
        if self.hours_date == day {
            self.hours
                .entry(hour.into())
                .or_default()
                .add(input, output, cr, cc, ctx);
        }
        self.day_models
            .entry(day.into())
            .or_default()
            .entry(model.into())
            .or_default()
            .add(input, output, cr, cc, ctx);
        self.day_sessions
            .entry(day.into())
            .or_default()
            .entry(sid.into())
            .or_default()
            .add(input, output, cr, cc, ctx);
        let proj = project.unwrap_or("unknown").to_string();
        self.day_projects
            .entry(day.into())
            .or_default()
            .entry(proj)
            .or_default()
            .add(input, output, cr, cc, ctx);
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PersistedState {
    #[serde(default)]
    pub local: MachineState,
    /// remote name -> machine state
    #[serde(default)]
    pub remotes: BTreeMap<String, MachineState>,
    /// names of remotes added by tailscale discovery (vs configured)
    #[serde(default)]
    pub discovered: Vec<String>,
}

impl PersistedState {
    pub fn load() -> Self {
        let path = crate::paths::state_path();
        if let Ok(data) = std::fs::read(&path) {
            if let Ok(s) = serde_json::from_slice(&data) {
                return s;
            }
        }
        Self::migrate_legacy().unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = crate::paths::our_dir();
        let _ = std::fs::create_dir_all(&dir);
        let tmp = dir.join("state.json.tmp");
        if let Ok(data) = serde_json::to_vec(self) {
            if std::fs::write(&tmp, &data).is_ok() {
                let _ = std::fs::rename(&tmp, crate::paths::state_path());
            }
        }
    }

    /// Convert the Swift-era state file (different shape) on first run.
    fn migrate_legacy() -> Option<Self> {
        #[derive(Deserialize)]
        struct Old {
            #[serde(default)]
            last_row_id: i64,
            #[serde(default)]
            days: BTreeMap<String, Stats>,
            #[serde(default)]
            day_models: BTreeMap<String, BTreeMap<String, Stats>>,
            #[serde(default)]
            day_sessions: BTreeMap<String, BTreeMap<String, Stats>>,
            #[serde(default)]
            hours: BTreeMap<String, Stats>,
            #[serde(default)]
            hours_date: String,
            #[serde(default)]
            remote: Option<OldRemote>,
        }
        #[derive(Deserialize)]
        struct OldRemote {
            #[serde(default)]
            last_row_id: i64,
            #[serde(default)]
            days: BTreeMap<String, Stats>,
            #[serde(default)]
            day_models: BTreeMap<String, BTreeMap<String, Stats>>,
            #[serde(default)]
            day_sessions: BTreeMap<String, BTreeMap<String, Stats>>,
        }
        let data = std::fs::read(crate::paths::legacy_state_path()).ok()?;
        let old: Old = serde_json::from_slice(&data).ok()?;
        let mut s = PersistedState::default();
        s.local.last_row_id = old.last_row_id;
        s.local.days = old.days;
        s.local.day_models = old.day_models;
        s.local.day_sessions = old.day_sessions;
        s.local.hours = old.hours;
        s.local.hours_date = old.hours_date;
        if let Some(r) = old.remote {
            let mut m = MachineState {
                last_row_id: r.last_row_id,
                days: r.days,
                day_models: r.day_models,
                day_sessions: r.day_sessions,
                ..Default::default()
            };
            m.last_ok_at = Some(chrono::Utc::now().timestamp());
            s.remotes.insert("vivo".into(), m);
            s.discovered.push("vivo".into());
        }
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrips_and_tolerates_missing_keys() {
        // Minimal old-format JSON must still decode.
        let s: PersistedState = serde_json::from_str(r#"{"local":{"last_row_id":5}}"#).unwrap();
        assert_eq!(s.local.last_row_id, 5);
        assert!(s.remotes.is_empty());
    }

    #[test]
    fn fold_updates_all_dimensions() {
        let mut m = MachineState::default();
        m.fold(
            "2026-11-21",
            "10",
            "s1",
            "swe-2-high",
            10,
            5,
            100,
            0,
            110,
            Some("repo"),
        );
        assert_eq!(m.days["2026-11-21"].in_plus_out(), 15);
        assert_eq!(m.day_models["2026-11-21"]["swe-2-high"].requests, 1);
        assert_eq!(m.day_sessions["2026-11-21"]["s1"].input_tokens, 10);
        assert_eq!(m.day_projects["2026-11-21"]["repo"].output_tokens, 5);
    }
}
