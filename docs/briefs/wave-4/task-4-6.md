### Task 4.6 — Session replay

**Files:**
- Modify: `src/server/handlers.rs` (the `handle_session_load` already does this; we're formalizing it)
- Create: `src/replay.rs` (utility for serialising events to the wire format)

**Why this exists:** when the desktop opens a past session, it needs the full event history to render the chat. The server's `SessionLoad` handler is the entry point; the underlying `read_entries` from wave 1 already returns the events.

**Interface (exact):**

```rust
// src/replay.rs
use crate::session_entry::SessionEntry;

/// A session replay is a serialised list of events in chronological
/// order, with metadata about the session (id, started_at, project_root).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionReplay {
    pub session_id: crate::ids::SessionId,
    pub project_root: std::path::PathBuf,
    pub started_at: crate::ids::Timestamp,
    pub entries: Vec<SessionEntry>,
    /// Total number of events (including those that were streamed live
    /// and may not have been persisted, e.g. TextDeltas).
    pub live_event_count: u64,
}

pub fn build_replay(entries: Vec<SessionEntry>) -> Result<SessionReplay, ReplayError> {
    // Take the SessionStarted entry as the metadata source.
    // Return all entries in order.
    // live_event_count = entries.iter().filter(|e| matches!(e, SessionEntry::RunInterrupted { .. })).count() + entries.iter().filter(|e| matches!(e, SessionEntry::AssistantMessage { .. })).count();
}
```

**Procedure:**
1. Implement `build_replay`.
2. The `handle_session_load` handler in 4.2 returns `Ok(serde_json::to_value(replay)?)`.
3. Tests:
   - Empty session (only `SessionStarted`) builds a replay with 1 entry.
   - Long session (5+ entries) builds a replay with the right `live_event_count`.
   - Round-trip via `serde_json`.

**Acceptance:**
- 4+ unit tests in `replay.rs`.
- The existing `handle_session_load` integration test (from 4.2) still passes.
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` clean.

**Forbidden:**
- No new dependencies.
- No `unwrap`/`expect`.

**Dependency:** none.
