# Crow v0 — Progress Ledger

> Single source of truth for "where are we". Read this on session start. If your context lost track, trust the ledger and `git log` over your own recollection.

## Routing
- MiniMax M3 = default coding model (Ferre budget ~100M tok/day)
- Nemotron Ultra = small features + research
- GLM-5.2 = debugging only

## Decisions
- `01-binary-name.md` — binary is `crow` not `pale` (Ferre, 2026-07-14)

## Wave 1 — Foundation (Phase 0)
**Status:** plan written, 3-reviewer swarm dispatched, awaiting findings.
**Branch:** `wave-1-foundation` (created at `4885377`)
**Worktree:** `~/code/crow/.worktrees/wave-1-foundation/`

Tasks (status):
- [ ] 1.1 Cargo workspace + CI
- [ ] 1.2 ID + event-envelope + message types
- [ ] 1.5 Cancellation primitive
- [ ] 1.3 JSONL session writer
- [ ] 1.4 Scripted mock provider
- [ ] 1.6 Public API smoke

Reviewer swarm: `deleg_c5ca655b` (3 parallel subagents, spec + rust + test)

## Wave 2 — Read-only agent loop (Phase 1)
**Status:** plan written, not yet dispatched.
**Tasks:** 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9 (research)

## Wave 3 — Mutation + recovery (Phase 2+3)
**Status:** plan written, not yet dispatched.
**Tasks:** 3.1, 3.2, 3.3, 3.4, 3.5, 3.6
