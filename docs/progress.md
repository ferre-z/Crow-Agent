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

### Wave 2 — Read-only agent loop
**Status:** Round D complete (2.1, 2.9 done; 2.4 done by orchestrator). 2.4 merged.

- [x] 2.1 stream processor (merged, `bf3c5f8`)
- [x] 2.4 tool registry + read tool (merged, `b338466` via `4bb642f`)
- [x] 2.9 Nemotron research (merged, `ac7c337`)
- [→] 2.2 genai adapter — agent dispatched, max-turns 30
- [ ] 2.5 AGENTS.md discovery — agent dispatched, max-turns 30
- [ ] 2.6 agent state machine — round F, after 2.5
- [ ] 2.7 headless CLI — round G
- [ ] 2.8 integration tests — round G

### Wave 3 — Mutation + recovery
**Status:** briefs written, not dispatched.

## Phase 2 — Desktop App (waves 4-7)
**Status:** plans + 27 task briefs written, NOT dispatched.

## Open issues

1. **`delegate_task` is broken in this session.** Use `claude --dangerously-skip-permissions -p`.
2. **Implementer agents hit max-turns on non-trivial tasks** (30 isn't enough). For finishers, use the pattern: "commit after every fix, even if tests are still failing". Finishers with 5-10 turn budgets often hang.
3. **For task 2.4, the orchestrator did 4 fix-and-test cycles manually** because two dispatched agents hit max-turns. The pattern: orchestrator fix is faster than dispatching finishers for small mechanical issues.

## Plan
- Wait for 2.2 + 2.5 to complete.
- Then dispatch 2.6 (agent state machine, round F — depends on 2.2 + 2.5).
- Then dispatch 2.7 + 2.8 (round G, sequential).
- Then wave 3 rounds H, I, J.
- Then phase 2.
