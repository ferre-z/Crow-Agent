### Task 3.6 — `crow sessions` + `crow --resume` + integration test sweeps

- `crow sessions` — lists sessions newest-first, prints `(id, started, last_status, message_count)`, table format
- `crow --resume ID` — reopens a session, replays history to stdout, then drops into `crow exec` mode for further input
- New `tests/integration/full_scenarios.rs` runs the v0 spec §18 acceptance criteria 1–9 as integration tests against a temporary Git repo, using the scripted provider (so they pass without network)

**Spec:** §15, §18.
**Acceptance:** 8+ tests, all the spec's "demonstrated in a temporary Git repository" criteria, except TUI-specific ones (deferred to wave 4).

## Review gate

Two reviewers per task, same as prior waves.

**Additional reject condition for wave 3:** any reviewer that finds a path escape or symlink bypass test that the implementer marked `#[ignore]` or skipped gets an automatic re-dispatch. Path escape tests are not optional.

## Decision log updates

- `docs/decisions/04-secret-redaction-patterns.md` — what patterns the redaction list matches, where it lives, and the explicit statement that it's not a security boundary.
- `docs/decisions/05-trust-model-v0.md` — restate the spec's "trusted-user tool" model and what wave 3 does NOT add (no sandbox, no permission prompts).
