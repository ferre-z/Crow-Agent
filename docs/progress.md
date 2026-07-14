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

## Wave 1 — Foundation (Phase 0) ✅ DONE

**Status:** All 6 tasks merged to main. 53 lib tests + 1 integration test pass. fmt + clippy + test gate all clean.

### Commits (newest first)
- `22a0e36` fix(phase0): wire lib.rs exports + rewrite phase0_smoke to use real APIs
- `86cd431` merge: task 1.6 (phase 0 public API smoke)
- `23aad82` merge: task 1.4 (scripted mock provider)
- `ad3fb1d` merge: task 1.3 (JSONL session writer)
- `cf8ce16` feat(crow): scripted mock provider + JSONL fixtures (task 1.4)
- `4bae793` test(crow): phase 0 public API smoke (task 1.6)
- `1cf0e8a` feat(crow): append-only JSONL session writer (task 1.3)
- `6d2f20d` fix(lib): register cancel module + re-exports (post-merge wiring)
- `87d7e77` merge: task 1.5 (cancellation helper)
- `9bfce85` feat(crow): cancellation helper (task 1.5)
- `c9cb901` merge: task 1.2 (ID + event + message + session types)
- `2aa03ce` feat(crow): ID + event + message + session types (task 1.2)
- `c110d90` fix(toolchain): bump Rust pin from 1.75 to 1.85
- `7693897` merge: task 1.1 (Cargo crate + CI scaffolding)
- `7719d4b` chore(workspace): Cargo crate + CI scaffolding (task 1.1)

### Module shape
```
src/
  cancel.rs         - CancellationToken re-export + timeout_or_cancel
  event.rs          - AgentEvent enum (live, in-memory)
  ids.rs            - SessionId, RunId, MessageId, ToolCallId, Timestamp
  lib.rs            - module wiring + re-exports
  main.rs           - binary: prints "crow 0.1.0"
  message.rs        - Message, Part, Role
  provider/
    mod.rs          - Provider trait, ModelRequest, ProviderStream, ProviderError
    mock.rs         - ScriptedProvider (loads JSONL fixtures)
  session.rs        - SessionWriter, SessionEntry enum (durable)
  session_entry.rs  - actually defined in session.rs (rename later)
```

### What was learned (saved as skill)
See `~/.hermes/skills/orchestrating-coding-agents-spire.md`. Key points:
- `delegate_task` is broken in this environment; use `claude --dangerously-skip-permissions -p`
- max-turns 30 is the right budget for non-trivial tasks
- `genai = 0.6.5` needs Rust 1.85+ (edition 2024)
- `#[serde(tag = "type")]` requires struct variants, not newtype variants
- Don't trust `cargo test` from the implementer; always re-run yourself
- 5-turn "finish" dispatches often hit max-turns; just do the fix yourself

## Wave 2 — Read-only agent loop (Phase 1)
**Status:** plan written, briefs written, NOT yet dispatched.

Tasks: 2.1 (stream processor), 2.2 (genai adapter), 2.3 (read tool), 2.4 (tool registry), 2.5 (AGENTS.md discovery), 2.6 (agent state machine), 2.7 (headless `crow exec`), 2.8 (integration tests), 2.9 (Nemotron research).

## Wave 3 — Mutation + recovery (Phase 2+3)
**Status:** plan written, briefs written, NOT yet dispatched.

Tasks: 3.1 (write tool), 3.2 (edit tool), 3.3 (bash tool), 3.4 (crash recovery), 3.5 (symlink/path escape security tests), 3.6 (CLI sessions + resume + integration sweeps).

## Open issues

1. **Max-turns wasted budget:** agents that hit max-turns on small fix tasks (5-turn finishers). Either pre-allocate more turns or do the fix manually.
2. **Worktree duplicate files:** when 1.3, 1.4, 1.6 all run in parallel and all touch `src/lib.rs`, 1.6 ends up with stale duplicates of 1.3/1.4 files. Reconciliation was manual.
3. **Auto-merge loses `pub mod` declarations.** Each merge that touches lib.rs needs verification of the full module list.

## Plan for next session
- Wave 2 round D (stream processor + tool registry + Nemotron research) — 3 parallel agents
- Then E, F, G as per the wave brief
- Wave 3 same pattern
