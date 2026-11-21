# Security Policy

## Reporting a vulnerability

Please do **not** open a public issue for security problems. Report them via
GitHub's private vulnerability reporting (Security → Report a vulnerability)
or by contacting the maintainer directly.

## Scope notes

devin-usage reads your local Devin CLI `sessions.db` **read-only** and talks
to remote machines exclusively through your own `ssh` (BatchMode, key auth).
It stores no credentials and opens no network listeners.

Reports we especially care about:

- Anything that writes to (or corrupts) `sessions.db`
- Command injection via ssh targets, paths, or session titles rendered in the
  WebKit dashboard
- The agent-install path pushing unexpected code to remotes
- Secrets or sensitive data being persisted into `state.json` or logs

## Supported versions

Only the latest `master` and the most recent tagged release receive fixes.
