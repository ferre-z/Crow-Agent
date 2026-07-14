### Task 2.8 — Integration test suite

**Files:**
- Create: `tests/agent_loop.rs`
- Create: `tests/sessions_recovery.rs` (basic — full crash recovery lands in wave 3 task 3.4)

**Spec references:** v0 spec §17 (testing strategy), §18 (acceptance criteria).

**Acceptance:**
- 8+ integration tests in `tests/agent_loop.rs`:
  1. scripted text-only response (1 turn, no tools)
  2. read → tool result → final response (uses a temp dir with a file)
  3. multiple sequential tool calls (3 read calls then a final response)
  4. tool failure followed by model recovery (read a non-existent file, get error result, retry, succeed)
  5. cancellation during provider stream (cancels mid-stream, history preserved)
  6. max_turns enforcement (scripted provider emits infinite tool calls, agent stops at max_turns)
  7. max_tool_calls enforcement (scripted provider emits 5 tool calls in one turn, agent stops at limit=3)
  8. context_limit (long history, agent returns ContextLimit error before exceeding model limit)
- 2+ integration tests in `tests/sessions_recovery.rs`:
  1. session file is created on submit
  2. session can be loaded back and history is intact
- All tests use `ScriptedProvider` (no network).
- Gate: clean.

**Forbidden:**
- No real network. No real genai. No real Nemotron.
- No `unwrap`/`expect` in test bodies (use `?` and `Result` returns).
- No live tests in this file.

**Dependencies:** task 2.6 must be merged first.
