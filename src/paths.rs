//! Filesystem locations. Everything lives under `~/.config/devin-usage/`
//! except the Devin CLI database itself.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    dirs::home_dir().expect("no home directory")
}

/// Devin CLI's session database (read-only for us).
pub fn default_db_path() -> PathBuf {
    home().join(".local/share/devin/cli/sessions.db")
}

/// Our own directory: config, state, logs.
pub fn our_dir() -> PathBuf {
    home().join(".config/devin-usage")
}

pub fn state_path() -> PathBuf {
    our_dir().join("state.json")
}

pub fn config_path() -> PathBuf {
    our_dir().join("config.toml")
}

/// Legacy state file from the pre-Rust build; migrated on startup.
pub fn legacy_state_path() -> PathBuf {
    home().join(".local/share/devin-usage-tracker/state.json")
}
