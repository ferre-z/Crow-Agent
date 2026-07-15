# Crow v0 — Progress Ledger

> Single source of truth for "where are we". Read this on session start.

## Routing
- MiniMax M3 = default coding model (Ferre budget ~100M tok/day)
- Nemotron Ultra = small features + research
- GLM-5.2 = debugging only

## Decisions
- `01-binary-name.md` — binary is `crow` not `pale` (Ferre, 2026-07-14)
- `02-chrono-dependency.md` — RESCINDED. Use project-owned `Timestamp(SystemTime)`.
- `03-rust-toolchain.md` — Rust pin is 1.85 (superseded)
- `04-rust-toolchain-wave-2.md` — bumped to 1.88 (genai transitive deps)
- `05-nemotron-genai-api.md` — research doc for the genai adapter

## Phase 1 — Kernel

### Wave 1 — Foundation ✅ DONE
All 6 tasks merged. 53 lib + 1 integration tests at HEAD `575036c`.

### Wave 2 — Read-only agent loop ✅ DONE
Tasks 2.1–2.6 merged. Phase 1 "kernel repair" follow-up landed the
live provider request shape (system prompt + AGENTS.md + tool
schemas/descriptions + tool calls/results) plus stop-reason handling,
live event sink, and durable `RunFailed` records.

- [x] 2.1 stream processor
- [x] 2.2 genai adapter
- [x] 2.3 read tool
- [x] 2.4 tool registry
- [x] 2.5 AGENTS.md discovery
- [x] 2.6 agent state machine
- [x] 2.7 headless CLI + layered config
- [x] 2.8 integration tests (genai_request_shape gate + others)
- [x] 2.9 Nemotron research

### Wave 3 — Mutation + recovery ✅ DONE
Tools `write`, `edit`, `bash` shipped; session recovery (sequence
resume, stale-lock eviction, crash-tail detection, `Agent::resume_into`)
shipped; path-confinement symlink-parent bug fixed.

- [x] 3.1 `write` tool
- [x] 3.2 `edit` tool
- [x] 3.3 `bash` tool
- [x] 3.4 crash recovery + `Agent::resume_into`
- [x] 3.5 symlink + path-escape security fixes
- [x] 3.6 CLI `crow sessions` + `crow --resume`

## Phase 2 — Desktop App (waves 4-7)
Plans + 27 task briefs written, NOT dispatched. See
`docs/waves/00-master-plan.md`. The frontend direction is **Tauri 2
desktop** (not Ratatui TUI). The Ratatui references in the README
and older docs have been removed.

## Open issues

1. `delegate_task` is broken in this session. Use `claude --dangerously-skip-permissions -p`.
2. Implementer agents hit max-turns on non-trivial tasks (30 isn't enough). For finishers, use the pattern: "commit after every fix, even if tests are still failing". Finishers with 5-10 turn budgets often hang.

## Plan
- Next: dispatch wave 4 (app-server + approvals).
- Then: wave 5 (Tauri shell).
- Then: waves 6 and 7 in parallel where possible.
