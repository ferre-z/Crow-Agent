---
type: wave-plan
status: detailed
wave: 2
phase: 1 (per 07-Build-Roadmap.md)
parent: 00-master-plan.md
---

# Wave 2 — Read-only agent loop (Phase 1)

**Goal:** Read-only agent loop + repository instructions + headless `crow exec`. Green gate: harness can answer a repository question through multiple `read` tool cycles via the scripted provider.

**Builds on:** Wave 1 (types, JSONL, cancellation, scripted provider, Rust 1.88).

## Dispatch strategy (revised post-wave-1)

Wave 1's parallel strategy caused the task 1.6 failure (implementer wrote tests against guessed APIs that didn't match the merged types). For wave 2 we use a **mostly sequential** strategy: each task branches from main AFTER the previous task's merge.

| Round | Tasks | Strategy |
|---|---|---|
| **D** | 2.1 + 2.4 + 2.9 | **Parallel** (all independent, no shared files) |
| **E** | 2.2 + 2.3 + 2.5 | **Sequential** (2.2 first, then 2.3+2.5 parallel) |
| **F** | 2.6 | Single task (state machine, depends on everything) |
| **G** | 2.7 + 2.8 | **Sequential** (CLI first, then integration tests) |

## Dependency map

```
2.1 stream processor ────────┐
                             ├── 2.6 agent state machine
2.4 tool registry ───────────┘
        │
        ├── 2.2 genai adapter (depends on 2.1 + 2.4)
        ├── 2.3 read tool (depends on 2.4)
        ├── 2.5 AGENTS.md discovery (independent)
                │
                └── 2.7 headless `crow exec` (depends on 2.6)
                        │
                        └── 2.8 integration tests
2.9 nemotron research ────────── (independent, can run anytime)
```

## Tasks (full briefs in `docs/briefs/wave-2/task-N-M.md`)

| # | Task | File | Round |
|---|---|---|---|
| 2.1 | Provider-neutral stream processor | `task-2-1.md` | D |
| 2.2 | `genai` 0.6.5 adapter (real provider) | `task-2-2.md` | E |
| 2.3 | `read` tool | `task-2-3.md` | E |
| 2.4 | Tool registry + schema validation + truncation | `task-2-4.md` | D |
| 2.5 | AGENTS.md discovery + context compiler | `task-2-5.md` | E |
| 2.6 | Agent state machine | `task-2-6.md` | F |
| 2.7 | Headless `crow exec` | `task-2-7.md` | G |
| 2.8 | Integration test suite | `task-2-8.md` | G |
| 2.9 | Nemotron API research (Nemotron Ultra) | `task-2-9.md` | D |

## Review gate

- Spec reviewer (MiniMax M3): did the code match the brief + spec section?
- Quality reviewer (MiniMax M3): did the gate pass? Are the tests real? YAGNI?
- **Both reviewers must run after the implementer** — wave 1 skipped this and the orchestrator had to catch most issues manually.

## Decision log to update

- `docs/decisions/04-rust-toolchain-wave-2.md` — 1.85 → 1.88 (DONE in this prep)
- `docs/decisions/05-context-size-estimation.md` — how we estimate context size for the `context_limit` error (added in 2.6 if needed)
- `docs/decisions/06-tool-event-sink.md` — backpressure semantics of the ToolEventSink (added in 2.4 if needed)
