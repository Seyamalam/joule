# devin-usage

A super-lightweight native macOS menu-bar app that tracks your Devin CLI token
usage — live — across every machine you run Devin on.

- **Native** — one Rust binary, `NSStatusItem` + `NSPopover` + `WKWebView`. No
  Electron, no dock icon, ~5 MB on disk.
- **Local** — incremental polling of Devin CLI's `sessions.db` (a `row_id`
  range query that costs nothing even on multi-GB databases).
- **Remote** — any number of SSH-reachable machines. A tiny static musl agent
  is pushed automatically (`scp`-style over `ssh`, no daemon, no ports) and
  streams only new rows.
- **Tailscale** — optionally auto-discovers tailnet peers that run Devin CLI
  and adds them on its own.

## What it shows

- Menu bar: `⚡ 25.1M` — today's input+output tokens across all machines.
- Click for the dashboard: combined totals with per-machine split, hourly
  activity, 14-day history (stacked local vs remote), top models, top
  sessions with real titles, per-project spend, and a **LIVE NOW** card for
  whatever session is burning tokens right now.

## Install

```sh
cargo build --release
./scripts/package-app.sh            # builds ~/Applications/DevinUsage.app
./scripts/install-launchagent.sh    # start at login
```

State lives in `~/.config/devin-usage/state.json`; it's safe to delete — the
app re-baselines from the local DB and remote agents on next start (one-time
cost).

## Configuration

`~/.config/devin-usage/config.toml`:

```toml
local_name = "this mac"
local_poll_secs = 3        # local sqlite poll interval
remote_poll_secs = 20      # per-remote base interval (backs off on failure)
ssh_user = "me"            # default user for tailscale-discovered hosts

# Permanent remotes:
[remotes.vivo]
target = "seyamalam@agent-server"   # or any Host alias from ~/.ssh/config

[remotes.bigbox]
target = "me@10.0.0.5"
disabled = false

# Auto-discovery over tailscale:
[tailscale]
enabled = true
ssh_user = "me"             # overrides global ssh_user for discovered peers
ignore = ["nas", "printer"]
```

SSH auth uses your existing keys/agent (`ssh -o BatchMode=yes`) — no
credentials are stored or copied anywhere.

## How remotes work

On first contact the app pushes itself (the embedded
`assets/agent-linux-x86_64` static musl binary) to
`~/.devin-usage/devin-usage-agent` on the remote, then polls it:

```sh
devin-usage agent baseline   # one-time aggregate dump (days/sessions/models)
devin-usage agent rows <N>   # metric rows with row_id > N (incremental)
devin-usage agent meta       # session id -> title/project map
devin-usage agent db-path    # where the devin db lives on this host
```

You can install the agent yourself on any Linux/macOS machine; the dashboard
only ever talks to it over ssh stdout — nothing listens on a socket.

Remotes are polled independently with exponential backoff (20 s → 5 min). A
remote that goes offline shows `stale Nm` in the dashboard footer and resumes
where it left off — no double counting, no rescan.

## CLI

```sh
devin-usage            # run the menu-bar app
devin-usage stats      # combined totals as JSON (both machines)
devin-usage doctor     # check every configured + discovered remote
devin-usage baseline   # rebuild local aggregates from scratch
devin-usage agent …    # remote-side mode (see above)
```

## Development

```sh
cargo test
cargo build --release                      # macOS app binary
./scripts/build-agent.sh                   # cross-compile the embedded
                                           # linux-x86_64 musl agent
                                           # (needs `zig` + the rustup target)
```

The agent binary is gitignored; if it's missing, the app builds fine but
`ensure_agent` refuses to push (dev builds). CI builds it with `musl-gcc`.

## Layout

```
src/
  main.rs       CLI entry
  engine.rs     poll scheduler, backoff, tailscale discovery
  db.rs         read-only sqlite access to sessions.db
  remote.rs     ssh transport + agent install/probe
  agent.rs      remote-side JSON subcommands
  state.rs      persisted per-machine aggregates + legacy migration
  aggregate.rs  merged view model for the dashboard
  tailscale.rs  `tailscale status --json` parsing
  ui/app.rs     NSStatusItem, NSPopover, WKWebView glue (objc2)
assets/
  dashboard.html  the popover UI
```

## Caveats

- Numbers are **tokens**, not billed ACU. Devin's credit/ACU accounting is
  server-side; the local DB only records token metrics per request.
- Timestamps are bucketed by *your* local timezone (the remote agent gets
  `TZ` passed through ssh).

## License

MIT — see [LICENSE](LICENSE).
