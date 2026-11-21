//! Remote machines: SSH polling, self-installing agent, health tracking.
//!
//! The remote side is this same binary in `agent` mode, pushed to
//! `~/.joule/joule-agent` on the remote when missing or outdated.

use crate::state::MachineState;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const AGENT_PATH: &str = "~/.joule/joule-agent";
const AGENT_DIR: &str = "~/.joule";

/// Embedded linux-x86_64 agent binary (static musl). Populated by
/// `scripts/package.sh` before building the macOS app.
static AGENT_LINUX_X86_64: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/agent-linux-x86_64"
));

/// A remote machine to poll.
#[derive(Debug, Clone)]
pub struct Remote {
    pub name: String,
    /// ssh target: "user@host" or a Host alias.
    pub target: String,
    /// true when added by tailscale discovery.
    #[allow(dead_code)]
    pub discovered: bool,
    pub next_poll_at: Instant,
    pub fail_count: u32,
}

impl Remote {
    pub fn new(name: String, target: String, discovered: bool) -> Self {
        Self {
            name,
            target,
            discovered,
            next_poll_at: Instant::now(),
            fail_count: 0,
        }
    }

    /// Exponential backoff: 20s, 40s, 80s … capped at 5 min.
    pub fn backoff(&self, base_secs: u64) -> Duration {
        let mult = 1u64 << self.fail_count.min(4);
        Duration::from_secs((base_secs * mult).min(300))
    }
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Run a command on the remote over ssh. Returns stdout on success.
fn ssh(target: &str, remote_cmd: &str, timeout_secs: u64) -> Result<Vec<u8>> {
    let out = Command::new("/usr/bin/ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=6",
            "-o",
            &format!("ServerAliveInterval={timeout_secs}"),
            "-C",
            target,
            remote_cmd,
        ])
        .stderr(Stdio::null())
        .output()
        .context("spawn ssh")?;
    if !out.status.success() {
        bail!("ssh exit {}", out.status);
    }
    Ok(out.stdout)
}

/// Probe whether a host has a Devin database (used by tailscale discovery).
pub fn probe_has_devin(target: &str) -> bool {
    ssh(target, "test -f ~/.local/share/devin/cli/sessions.db", 4).is_ok()
}

/// Make sure the remote agent binary exists and matches our version.
/// Pushes the embedded build if missing or wrong arch/version.
pub fn ensure_agent(target: &str) -> Result<()> {
    let check = ssh(
        target,
        &format!("test -x {AGENT_PATH} && {AGENT_PATH} --version 2>/dev/null || echo missing"),
        8,
    );
    let current = check
        .ok()
        .map(|o| String::from_utf8_lossy(&o).trim().to_string())
        .unwrap_or_default();
    if current.starts_with(env!("CARGO_PKG_NAME")) {
        return Ok(()); // any version is fine; protocol is stable
    }
    if AGENT_LINUX_X86_64.is_empty() {
        bail!("no embedded agent binary (dev build?)");
    }
    let arch = ssh(target, "uname -m", 5)
        .map(|o| String::from_utf8_lossy(&o).trim().to_string())
        .unwrap_or_default();
    if arch != "x86_64" {
        bail!("unsupported remote arch: {arch}");
    }
    // Push: stream the binary through ssh stdin.
    let mut child = Command::new("/usr/bin/ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            target,
            &format!("mkdir -p {AGENT_DIR} && cat > {AGENT_PATH} && chmod +x {AGENT_PATH}"),
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .context("spawn push ssh")?;
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(AGENT_LINUX_X86_64)?;
    let status = child.wait()?;
    if !status.success() {
        bail!("agent push failed");
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct RowsResponse {
    pub max: i64,
    #[serde(default)]
    pub rows: Vec<Vec<serde_json::Value>>,
    #[serde(default)]
    pub meta: std::collections::HashMap<String, MetaEntry>,
}

#[derive(Deserialize)]
pub struct MetaEntry {
    pub t: Option<String>,
    pub p: Option<String>,
}

/// Poll a remote for rows newer than `m.last_row_id`; fold into `m`.
/// Returns the session ids that produced new rows (for live detection).
pub fn poll(
    remote: &Remote,
    m: &mut MachineState,
    day_of: &dyn Fn(i64) -> String,
    hour_of: &dyn Fn(i64) -> String,
) -> Result<Vec<String>> {
    let body = match ensure_agent(&remote.target).and_then(|_| {
        ssh(
            &remote.target,
            &format!("{AGENT_PATH} agent rows {}", m.last_row_id),
            15,
        )
    }) {
        Ok(b) => b,
        Err(e) => {
            m.last_error = Some(e.to_string());
            return Err(e);
        }
    };
    let resp: RowsResponse = serde_json::from_slice(&body).context("parse rows")?;

    for (sid, meta) in &resp.meta {
        if let Some(t) = &meta.t {
            m.titles.insert(sid.clone(), t.clone());
        }
        if let Some(p) = &meta.p {
            m.projects.insert(sid.clone(), p.clone());
        }
    }

    // Day rollover: reset the hourly buckets for the new day.
    let today = day_of(now_epoch());
    if m.hours_date != today {
        m.hours.clear();
        m.hours_date = today;
    }

    let mut sids = Vec::new();
    for raw in &resp.rows {
        let v = |i: usize| raw.get(i).and_then(|x| x.as_i64()).unwrap_or(0);
        let sid = raw.get(0).and_then(|x| x.as_str()).unwrap_or("");
        let model = raw.get(2).and_then(|x| x.as_str()).unwrap_or("unknown");
        if sid.is_empty() {
            continue;
        }
        let day = day_of(v(1));
        let hour = hour_of(v(1));
        let proj = m.projects.get(sid).cloned();
        m.fold(
            &day,
            &hour,
            sid,
            model,
            v(3),
            v(4),
            v(5),
            v(6),
            v(7),
            proj.as_deref(),
        );
        m.last_row_id = m.last_row_id.max(v(8));
        sids.push(sid.to_string());
    }
    m.last_row_id = m.last_row_id.max(resp.max);
    m.last_ok_at = Some(now_epoch());
    m.last_error = None;
    Ok(sids)
}

/// Full backfill for a newly added remote: replaces `m` with remote baseline.
/// The remote agent buckets days in *our* timezone via the TZ env var so all
/// machines share the same day boundaries.
pub fn backfill(remote: &Remote, tz: &str) -> Result<MachineState> {
    ensure_agent(&remote.target)?;
    let body = ssh(
        &remote.target,
        &format!("TZ='{tz}' {AGENT_PATH} agent baseline"),
        180,
    )?;
    let mut m: MachineState = serde_json::from_slice(&body).context("parse baseline")?;
    m.last_ok_at = Some(now_epoch());
    Ok(m)
}
