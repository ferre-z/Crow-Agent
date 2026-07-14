### Task 2.8 — Integration test suite

`tests/agent_loop.rs`:
- scripted text-only response
- read → tool result → final response
- write/edit → bash test → final response (write/edit/bash landed in wave 3, this test will be marked `#[ignore]` until then with a comment)
- multiple sequential tool calls
- tool failure followed by model recovery
- cancellation during provider stream
- timeout and child-process cleanup
- crash-shaped incomplete JSONL followed by resume
- max-turn and max-tool-call enforcement
- terminal resize and panic restoration smoke tests (smoke tests get a real TUI in wave 4)

**Spec:** §17. The bash/write/edit integration tests are placeholders.
**Acceptance:** all tests that aren't `#[ignore]` pass without network. At least 7 tests must be non-ignored in this wave.
