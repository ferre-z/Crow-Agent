### Task 3.4 — Crash recovery

**Files:**
- Modify: `src/session.rs` (extend `read_entries` to handle truncated last line)
- Create: `src/recovery.rs`
- Modify: `src/agent.rs` (add `resume_into` constructor that prepends recovered history)

**Spec references:** v0 spec §10, §16 (crash during streaming/tool), §18 (acceptance criterion 7: "A forced crash during an operation produces a valid recoverable session").

**Why this exists:** the spec's acceptance criterion 7 says force-killing the process at every recorded state must produce a valid recoverable session. We achieve this by:
1. On `read_entries`, if the last line is truncated or unparseable, mark that line as `RunInterrupted { active_call: <last seen tool call id, if any> }` and return the rest.
2. On resume (`crow --resume ID`), the agent loop starts a new run with the prior `Conversation` preloaded, and the next durable entry is `RunInterrupted` (if not already written) followed by normal entries.

**Interfaces (exact):**

```rust
// src/session.rs (extend)
pub async fn read_entries_with_recovery(
    path: impl AsRef<Path>,
) -> Result<(Vec<SessionEntry>, Option<RunInterrupted>), SessionError> {
    // Like read_entries, but if the last line is malformed or truncated,
    // return it as Option<RunInterrupted>:
    //   - if the malformed line's bytes could be parsed as a partial
    //     JSON, extract the last seen "call_id" (heuristic: look for
    //     the last `"call_id"` key) and report it as active_call.
    //   - otherwise, report active_call: None.
    todo!()
}

// src/recovery.rs
use crate::session_entry::SessionEntry;
use crate::ids::ToolCallId;

#[derive(Debug, Clone)]
pub struct RecoveryState {
    pub recovered_entries: Vec<SessionEntry>,
    pub interrupted: Option<RunInterrupted>,
}

pub fn derive_active_call_from_partial(partial_bytes: &[u8]) -> Option<ToolCallId> {
    // Heuristic: find the last "call_id":"<ulid>" pattern in the bytes
    // and parse the ULID. Best-effort; None if no match.
    todo!()
}
```

**Acceptance:**
- 10+ unit tests:
  1. truncate the last line at byte 0 → no entries, RunInterrupted with active_call: None
  2. truncate the last line at byte 50 (mid-line) → previous entries returned, RunInterrupted with active_call: None
  3. truncate a `ToolStarted` entry mid-JSON → RunInterrupted with the partial call_id
  4. valid JSONL with no truncation → no RunInterrupted
  5. empty file → no RunInterrupted, empty entries
  6. file with only one truncated line → RunInterrupted with active_call: None
  7. file with multiple truncated lines → only the last RunInterrupted is returned
  8. resume on a recovered session: the agent's history contains the recovered entries plus a new RunInterrupted
  9. double resume: re-loading a recovered session is idempotent (does not append a duplicate RunInterrupted)
  10. seq numbers are monotonic across recovery
  11. recovery preserves the timestamp of the recovered entries

**Forbidden:**
- No silent recovery (always emit a RunInterrupted if any line is malformed).
- No re-writing the session file during recovery (read-only).
- No `panic!` on partial JSON — return typed error.

**Dependencies:** `ulid` already in deps.
