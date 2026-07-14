### Task 3.4 — Crash recovery

`src/session.rs` (extend) + new `src/recovery.rs`:
- On `read_entries`, if a session file's last line is truncated (no terminating newline) or unparseable, mark that line as `RunInterrupted { active_call: <last seen tool call id, if any> }` and return the rest
- On resume (`crow --resume ID` and on agent startup if the user opts to "continue last"), the agent loop starts a new run with the prior `Conversation` preloaded, and the next durable entry is `RunInterrupted` (if not already written) followed by normal entries
- A small state machine: `{ Idle, Streaming(provider), Streaming(tool), Persisting }` — on session open we look at the last `SessionEntry` and emit a `RunInterrupted` for whatever state the previous run died in

**Spec:** §10, §16, §18 (acceptance criterion 7: "A forced crash during an operation produces a valid recoverable session").
**Acceptance:** 10+ tests — truncate last line at every byte position from 0 to "last line length" inclusive, resume yields correct history, double resume is idempotent, no seq gap, `RunInterrupted` carries the right `active_call` for each state.
