---
type: post-mortem
status: in-progress
wave: 2
date: 2026-07-14
audience: future Ferre + future Hermes sessions
---

# Wave 2 Post-Mortem — running, update as tasks complete

## TL;DR (so far)

Round D (2.1, 2.4, 2.9) dispatched in parallel. 2.9 done by orchestrator (after agent max-turns). 2.1 done by orchestrator (after 30-turn + 10-turn finisher both hit max-turns). 2.4 in progress.

**Wall-clock so far:** ~25 minutes. **Tokens used:** ~1.5M (rough estimate).

## Confirmed wave 1 lessons (re-applied in wave 2)

1. **Max-turns 30 hits mid-task** for non-trivial work. The implementer writes most of the code but doesn't make it to the commit step. A 10-turn finisher also hits max-turns on the same kind of task. The right move is for the orchestrator to finish the work manually.
2. **The implementer needs the post-merge API in its brief.** Task 2.2's brief had a wrong mapping table (genai 0.6.5's actual enum is simpler). The decision doc 05 (research) corrected it, but only AFTER the agent hit max-turns. Future briefs should cite the decision doc up front.
3. **Lib.rs `pub mod` declarations get lost in merges.** Already saw this in wave 1.
4. **Cargo.lock conflicts on parallel branches.** Already saw this.

## New lessons from wave 2 round D

1. **`delegate_task` continues to silently drop results.** Pivoted to `claude --dangerously-skip-permissions -p` via `terminal(background=true, notify_on_complete=true)`. Works.
2. **The orchestrator can finish the work faster than a finisher agent.** After 2 implementer dispatches (30 + 10 turns) failed to commit task 2.1, the orchestrator finished in 8 file edits and ran the gate in 2 minutes. Lesson: when an agent hits max-turns with 80% done, finish manually instead of dispatching a finisher.
3. **The toolchain bump 1.85 → 1.88 was forced by genai's transitive deps.** This is a recurring pattern: spec says "use crate X" and X's transitive deps need a newer compiler. The orchestrator did this work in the wave prep commit, BEFORE any task started, which avoided blocking all 9 wave 2 tasks.
4. **The implementer added `Usage: Copy` and `futures = "0.3"` to Cargo.toml correctly** but missed the Display impl on the ID newtypes, and the implementer also added a manual `Default` impl that conflicted with the derive. The orchestrator's job is to clean up these small details.
5. **Tool 2.4 (registry + read) is the largest single task** — 1845 lines of code. The implementer needs the brief to be tight on what NOT to write (e.g. "no integration tests in src/tool/, put them in tests/tool_registry.rs"). Future briefs should include this.

## Tasks

| # | Status | Commit | Notes |
|---|---|---|---|
| 2.1 stream processor | done by orchestrator | bf3c5f8 | 16 unit tests, 69 total lib tests pass |
| 2.4 tool registry + read tool | in progress (orchestrator dispatched a 15-turn finisher) | | 17 compile errors, 1845 lines |
| 2.9 Nemotron research | done by orchestrator | ac7c337 | decision doc 05 with real genai enum names |
| 2.2 genai adapter | queued (round E) | | brief updated with decision doc reference |
| 2.3 read tool | merged into 2.4 | n/a | |
| 2.5 AGENTS.md discovery | queued (round E) | | |
| 2.6 agent state machine | queued (round F) | | |
| 2.7 headless CLI | queued (round G) | | |
| 2.8 integration tests | queued (round G) | | |

## Process improvements for wave 3

- [ ] **Pre-write a generic "fix the N errors" finisher prompt** so we can dispatch it in 5 turns for small issues.
- [ ] **Cap the implementer brief at 30 turns, no exception** — for tasks bigger than that, split them.
- [ ] **Inline the decision docs in the brief** rather than pointing to them. The agent may not read the linked doc.
- [ ] **Wave 3 task 3.3 (bash) is the biggest** (18 tests, process group, signals). Plan to dispatch with --max-turns 30 and immediately have a finisher prompt ready.

## Failures
(append below as they happen)
