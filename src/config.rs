//! User configuration: `~/.config/devin-usage/config.toml`
//!
//! ```toml
//! local_name = "mac"
//! ssh_user = "seyamalam"                 # default user for discovered hosts
//!
//! [remotes.vivo]
//! target = "seyamalam@agent-server"
//!
//! [tailscale]
//! enabled = true
//! # ssh_user = "other-user"             # overrides global for discovered peers
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Label for this machine in the dashboard split.
    #[serde(default = "default_local_name")]
    pub local_name: String,
    /// SSH user assumed for auto-discovered machines (tailscale peers).
    #[serde(default)]
    pub ssh_user: Option<String>,
    /// Seconds between local database polls.
    #[serde(default = "default_local_interval")]
    pub local_poll_secs: u64,
    /// Base seconds between remote polls (backs off on failure).
    #[serde(default = "default_remote_interval")]
    pub remote_poll_secs: u64,
    #[serde(default)]
    pub remotes: BTreeMap<String, RemoteConfig>,
    #[serde(default)]
    pub tailscale: TailscaleConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// ssh target, e.g. "user@host" or a Host alias from ~/.ssh/config
    pub target: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TailscaleConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ssh_user: Option<String>,
    /// Hosts (by Tailscale DNS name or hostname) to never auto-add.
    #[serde(default)]
    pub ignore: Vec<String>,
}

fn default_local_name() -> String {
    "this mac".into()
}
fn default_local_interval() -> u64 {
    3
}
fn default_remote_interval() -> u64 {
    20
}

impl Default for Config {
    fn default() -> Self {
        Self {
            local_name: default_local_name(),
            ssh_user: None,
            local_poll_secs: default_local_interval(),
            remote_poll_secs: default_remote_interval(),
            remotes: Default::default(),
            tailscale: Default::default(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = crate::paths::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("devin-usage: bad config.toml: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Write an annotated starter config if none exists.
    pub fn write_default() -> std::io::Result<()> {
        let path = crate::paths::config_path();
        if path.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, DEFAULT_CONFIG)
    }
}

const DEFAULT_CONFIG: &str = r#"# devin-usage configuration
# local_name = "this mac"
# local_poll_secs = 3
# remote_poll_secs = 20

# Machines you want tracked permanently:
# [remotes.vivo]
# target = "seyamalam@agent-server"

# Auto-discover machines on your tailnet that run Devin CLI:
# [tailscale]
# enabled = true
# ssh_user = "me"          # user to ssh as on discovered peers
# ignore = ["nas", "printer"]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remotes_and_tailscale() {
        let c: Config = toml::from_str(
            r#"
            local_name = "macbook"
            ssh_user = "seyamalam"
            [remotes.vivo]
            target = "seyamalam@agent-server"
            [tailscale]
            enabled = true
            ignore = ["nas"]
        "#,
        )
        .unwrap();
        assert_eq!(c.local_name, "macbook");
        assert_eq!(c.remotes["vivo"].target, "seyamalam@agent-server");
        assert!(c.tailscale.enabled);
        assert_eq!(c.tailscale.ignore, vec!["nas"]);
    }

    #[test]
    fn empty_config_uses_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.local_poll_secs, 3);
        assert_eq!(c.remote_poll_secs, 20);
        assert!(!c.tailscale.enabled);
    }
}
