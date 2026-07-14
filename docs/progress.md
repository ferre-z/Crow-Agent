# Crow v0 — Progress Ledger

> Single source of truth for "where are we". Read this on session start. If your context lost track, trust the ledger and `git log` over your own recollection.

## Routing
- MiniMax M3 = default coding model (Ferre budget ~100M tok/day)
- Nemotron Ultra = small features + research
- GLM-5.2 = debugging only

## Decisions
- `01-binary-name.md` — binary is `crow` not `pale` (Ferre, 2026-07-14)
- `02-chrono-dependency.md` — RESCINDED. Use project-owned `Timestamp(SystemTime)` instead (no chrono).
- `03-rust-toolchain.md` — Rust pin is 1.85 (not 1.75), needed by genai 0.6.5's edition 2024.

## Wave 1 — Foundation (Phase 0)
**Status:** Round A done, Round B done, Round C running.

### Round A (sequential, no deps)
- [x] 1.1 Cargo crate + CI scaffolding (commit `7719d4b` → merged `7693897`)
- [x] 1.7 Toolchain pin fix 1.75 → 1.85 (commit `c110d90`)

### Round B (parallel: 1.2 + 1.5)
- [x] 1.2 ID + event + message + session types (commit `2aa03ce` → merged `c9cb901`)
  - 22 lib tests pass
  - Fixed: `Timestamp::now()` truncates to ms for lossless round-trip
  - Fixed: `RunFailed` field order test rewritten to use struct-typed deserialize
  - Fixed: `SCHEMA_VERSION` import scoped to `#[cfg(test)]` module
  - Removed `pedantic` from `[lints.clippy]` (too noisy for spec-exact code)
- [x] 1.5 Cancellation helper (commit `9bfce85` → merged `87d7e77`)
  - 10 tokio tests pass
  - Module exports: `CancellationToken`, `timeout_or_cancel`, `CancelOutcome`
- [x] 1.7b Post-merge wiring fix (commit `6d2f20d`)
  - `pub mod cancel` + re-exports were lost in merge; manually restored

### Round C (parallel: 1.3 + 1.4 + 1.6)
**Status:** dispatched in parallel, 3 agents running, max-turns 30 each.
- [ ] 1.3 JSONL session writer
- [ ] 1.4 Scripted mock provider
- [ ] 1.6 Public API smoke test
- Sessions: `proc_91292359d165` (1.3), `proc_4beba11b98de` (1.4), `proc_263b675cfe6f` (1.6)

## Wave 2 — Read-only agent loop (Phase 1)
**Status:** plan written, not yet dispatched.

## Wave 3 — Mutation + recovery (Phase 2+3)
**Status:** plan written, not yet dispatched.

## Open issues from wave 1

1. **`delegate_task` is broken in this session.** Returns nothing. Workaround: `claude --dangerously-skip-permissions -p` via terminal tool, with the brief inline.
2. **Interactive claude via tmux hangs after first dialog.** Workaround: stay on print mode.
3. **Merge auto-conflict on Cargo.lock** when two parallel branches both pull dependencies. Workaround: take theirs + `cargo build` to regenerate.

## Plan
- After wave 1 round C: merge 1.3, 1.4, 1.6, run full review on the integrated wave, then move to wave 2.
- Wave 2 is bigger (9 tasks, several rounds of parallelism). Plan to dispatch in 4 rounds (D, E, F, G) per the wave brief.
