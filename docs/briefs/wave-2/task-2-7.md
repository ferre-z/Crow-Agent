### Task 2.7 — Headless `crow exec`

Extends `src/cli.rs` + `src/main.rs`:
- Subcommand: `crow exec "task description"` — runs the agent loop without the TUI, prints events to stdout
- `crow sessions` — lists sessions, prints `(id, started_at, message_count, last_status)`
- `crow --resume ID` — reopens a session, replays history to stdout

**Spec:** §15. `exec` is added in this wave; spec doesn't require it but Phase 1 roadmap says "headless `exec` command".
**Acceptance:** 5+ tests + a CLI smoke test using `assert_cmd`.
