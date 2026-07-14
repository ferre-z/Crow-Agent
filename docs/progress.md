# Crow v0 — Progress Ledger

> Single source of truth for "where are we". Read this on session start. If your context lost track, trust the ledger and `git log` over your own recollection.

## Routing
- MiniMax M3 = default coding model (Ferre budget ~100M tok/day)
- Nemotron Ultra = small features + research
- GLM-5.2 = debugging only

## Decisions
- `01-binary-name.md` — binary is `crow` not `pale` (Ferre, 2026-07-14)
- `02-chrono-dependency.md` — RESCINDED. Use project-owned `Timestamp(SystemTime)` instead (no chrono).
- `03-rust-toolchain.md` — Rust pin is 1.85 (later updated by Decision 04)
- `04-rust-toolchain-wave-2.md` — bumped to 1.88 (genai 0.6.5 transitive deps)
- `05-nemotron-genai-api.md` — research doc, what the genai adapter implementer needs

## Phase 1 — Kernel (waves 1-3)

**Status:** waves 1-2 partial, wave 3 not started. Wave 1 fully merged to main. Wave 2 in progress (task 2.4 mid-fix).

### Wave 1 — Foundation ✅ DONE
All 6 tasks merged to main. 53 lib tests + 1 integration test pass at HEAD `575036c`.

### Wave 2 — Read-only agent loop (Phase 1)
**Status:** Round D in progress. Tasks 2.1 + 2.9 done; 2.4 in worktree, 80% done, 3 clippy errors + 13 test failures remaining.

### Wave 3 — Mutation + recovery (Phase 2+3)
**Status:** briefs written, not dispatched.

## Phase 2 — Desktop App (waves 4-7)

**Status:** plans + briefs written, **NOT dispatched**. Ferre can't test right now; wave 4 doesn't start until Ferre returns to test the headless app-server.

### Wave 4 — App-server + approvals (the protocol layer)
**Status:** plans + 7 task briefs written. Foundation: spawns `crow serve` over a JSON-RPC-over-stdio protocol; layers an approval policy on top of the autonomous kernel.

### Wave 5 — Tauri shell + web frontend
**Status:** plans + 8 task briefs written. Tauri 2 desktop, project picker, session sidebar, chat pane, composer with slash-commands, IPC bridge, native packaging.

### Wave 6 — Approvals UX + keyring + image attachments
**Status:** plans + 5 task briefs written. Approval cards with diff preview, OS keyring for API keys, image attachments (PNG/JPEG).

### Wave 7 — Plan mode + activity pane + notifications + polish
**Status:** plans + 7 task briefs written. Plan mode (read-only run with apply button), activity pane (full event log), system notifications, settings pane, onboarding flow, E2E test suite.

## Open issues

1. **Wave 2 task 2.4 mid-fix.** The implementer hit max-turns with 80% of the code written. The orchestrator was working through 3 clippy errors + 13 test failures when the iteration cap was hit. Resume: fix the 3 clippy micro-issues, restore the `Vec<Component>` for `tail_components` (lost the `..` handling), fix the `format_lines` trailing-newline behaviour, commit, merge.
2. **`delegate_task` is broken in this session.** Continues to silently drop results. Workaround: use `claude --dangerously-skip-permissions -p` via `terminal(background=true, notify_on_complete=true)`. Documented in the `orchestrating-coding-agents-spire` skill.
3. **Implementer agents hit max-turns on non-trivial tasks** (30 isn't enough). Workaround: finish manually when they stall at 80% — it's faster than dispatching finishers.

## Process improvements for phase 2

- [ ] **Each task gets a reviewer dispatch** after the implementer (2 reviewers: spec + quality). Phase 1 skipped this; phase 2 should enforce it.
- [ ] **Inline the post-merge API of dependencies in each brief.** Phase 1's failure mode: the implementer guessed wrong APIs.
- [ ] **Tauri tests run via `tauri test`, not `cargo test`.** The Tauri runtime requires it.
- [ ] **Pre-write the frontend TypeScript types** as a shared schema that the Rust side serializes to. Avoids drift between Rust and TS.

## Next session

1. Resume wave 2 task 2.4 (small fix; merge to main).
2. Dispatch wave 2 round E (2.2 genai adapter, 2.5 AGENTS.md discovery).
3. Then wave 2 round F (2.6 agent state machine).
4. Then wave 2 round G (2.7 headless CLI, 2.8 integration tests).
5. Then wave 3 (3.1-3.6 in 3 rounds).
6. Then wave 4 onwards (per the phase 2 master plan).

## Open questions for Ferre (carry from phase 1 post-mortem)

1. **Should the kernel's `crow serve` binary be in the same crate as the library, or a separate `crow-server` crate?** Currently planned as a subcommand of the existing `crow` binary. Cleaner separation is a separate crate.
2. **What is the default approval policy for the desktop?** Three options: (a) `NoOp` (autonomous, matches the spec), (b) `Ask` for every tool, (c) configurable per session. **Recommendation: (a) by default, (c) opt-in via `SessionStart.policy: "ask"`.**
3. **Tauri 2.x or Tauri 1.x?** Tauri 2 is the current stable. Use Tauri 2.
4. **Frontend framework: vanilla TS + custom elements, or Preact, or Svelte?** **Recommendation: vanilla TS + a 50-line `Component` base class.** Smallest bundle, easiest to learn.

## Handover doc

The full phase 2 plan is at `docs/phase-2-handover.md` and `docs/waves/00-master-plan.md` (the new canonical master plan).
