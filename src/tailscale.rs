//! Tailscale discovery: find machines on the tailnet that run Devin CLI.
//!
//! `tailscale status --json` gives every peer's DNS name, IPs, OS and online
//! state. For each online non-self peer we probe `user@dns` for a sessions.db;
//! hits become discovered remotes (they can be made permanent in config.toml).

use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Peer {
    pub hostname: String,
    /// FQDN like "agent-server.tail-abc.ts.net."
    pub dns: String,
    pub online: bool,
    pub is_self: bool,
}

#[derive(Deserialize)]
struct Status {
    #[serde(rename = "Self", default)]
    self_node: Option<SelfNode>,
    #[serde(rename = "Peer", default)]
    peer: HashMap<String, PeerJson>,
}

#[derive(Deserialize)]
struct SelfNode {
    #[serde(rename = "HostName", default)]
    host_name: String,
}

#[derive(Deserialize)]
struct PeerJson {
    #[serde(rename = "HostName", default)]
    host_name: String,
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "Online", default)]
    online: bool,
    #[serde(rename = "TailscaleIPs", default)]
    #[allow(dead_code)]
    ips: Vec<String>,
}

/// Peers visible on the tailnet. Returns empty when tailscale is absent.
pub fn peers() -> Vec<Peer> {
    let out = Command::new("tailscale")
        .args(["status", "--json"])
        .output();
    let Ok(out) = out else { return vec![] };
    let Ok(status) = serde_json::from_slice::<Status>(&out.stdout) else {
        return vec![];
    };
    let self_name = status.self_node.map(|s| s.host_name).unwrap_or_default();
    status
        .peer
        .into_values()
        .map(|p| {
            let host = p.host_name.clone();
            Peer {
                hostname: host.clone(),
                dns: p.dns_name.trim_end_matches('.').to_string(),
                online: p.online,
                is_self: host == self_name,
            }
        })
        .collect()
}

/// Online, non-self peers that look like devin hosts: not ignored and not
/// already covered by a configured remote name.
pub fn discoverable(ignore: &[String], existing: &[String]) -> Vec<Peer> {
    peers()
        .into_iter()
        .filter(|p| p.online && !p.is_self)
        .filter(|p| {
            let short = p.hostname.to_lowercase();
            let dns = p.dns.to_lowercase();
            !ignore
                .iter()
                .any(|i| short == i.to_lowercase() || dns.starts_with(&i.to_lowercase()))
                && !existing
                    .iter()
                    .any(|e| short == e.to_lowercase() || dns.starts_with(&e.to_lowercase()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tailscale_status() {
        let json = br#"{
            "Self": {"HostName": "macbook"},
            "Peer": {
                "k1": {"HostName": "agent-server", "DNSName": "agent-server.tail-1.ts.net.", "Online": true, "TailscaleIPs": ["100.1.2.3"]},
                "k2": {"HostName": "nas", "DNSName": "nas.tail-1.ts.net.", "Online": false}
            }
        }"#;
        let s: Status = serde_json::from_slice(json).unwrap();
        assert_eq!(s.self_node.unwrap().host_name, "macbook");
        let p = &s.peer["k1"];
        assert_eq!(p.host_name, "agent-server");
        assert!(p.online);
    }
}
