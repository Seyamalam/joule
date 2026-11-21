# Contributing

Thanks for helping improve devin-usage. The project is intentionally tiny —
please keep it that way. A single Rust binary, no runtime dependencies, no
daemons, no Electron.

## Dev setup

```sh
git clone https://github.com/Seyamalam/devin-usage
cd devin-usage
cargo test                    # must pass
cargo build --release         # the macOS app binary
./scripts/package-app.sh      # installs ~/Applications/DevinUsage.app
```

For remote-agent work you also need the embedded linux binary:

```sh
./scripts/build-agent.sh      # needs zig (macOS) or musl-tools (Linux)
```

## Ground rules

- **Stay lightweight.** No new dependencies without a strong reason — every
  crate is weight the user carries in their menu bar. If a feature needs a
  dependency, open an issue first.
- **Never touch the Devin DB for writes.** `db.rs` is read-only on purpose.
  All persistence goes to `~/.config/devin-usage/state.json`.
- **No secrets, ever.** SSH auth is the user's own keys/agent via
  `ssh -o BatchMode=yes`. Don't add token handling, key files, or network
  listeners.
- **Incremental only.** Never scan the whole `sessions.db` in the steady
  state — everything works off `row_id > last_seen` cursors. A feature that
  requires a full scan needs a good excuse and a one-time `baseline` path.
- **macOS-native UI.** `ui/` uses `objc2`/`objc2-app-kit`/`objc2-web-kit`
  directly. Keep Objective-C interop confined to `ui/`; core logic must stay
  platform-neutral (it also builds the Linux agent).
- **Main-thread discipline.** Anything touching AppKit/WebKit runs on the
  main thread; the engine runs on worker threads and snapshots state.

## Style

- `cargo fmt` clean, `cargo clippy` quiet, `cargo test` green — CI enforces
  fmt + tests on macOS and Linux.
- Match the existing style: small functions, module-level `//!` docs,
  compact code, comments only where non-obvious.
- Cross-platform code must not use macOS-only APIs outside `#[cfg]` gates or
  the `ui` module.

## Commits and PRs

- Small, focused commits. Describe the *why*, not just the *what*.
- Keep PRs reviewable — one feature/fix per PR, ideally under ~300 lines of
  diff. Bigger work: open an issue and split it up first.
- Fill in the PR template: summary, how you tested it, and screenshots for
  anything user-visible in the popover/dashboard.
- PRs get CI (fmt + tests) plus maintainer review. Address review comments
  in new commits; maintainer squashes on merge.
- Don't bump the version in `Cargo.toml` in feature PRs — releases are
  tagged by the maintainer.

## Reporting issues

- Use the issue templates (bug / feature request).
- For bugs: include `devin-usage doctor` output and whether the problem is
  local, remote, or the UI. Redact hostnames/usernames if you prefer.
- Security issues: see [SECURITY.md](SECURITY.md) — please don't file those
  publicly.
