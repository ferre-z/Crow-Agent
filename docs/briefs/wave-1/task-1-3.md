### Task 1.3 — JSONL session writer

**Files:**
- Create: `src/session.rs`
- Modify: `src/lib.rs` (re-export)

**Spec references:** v0 spec §10 (durable entries), §16 (crash recovery).

**Interfaces:**

```rust
pub struct SessionWriter {
    file: std::fs::File,
    path: std::path::PathBuf,
    seq: u64,
}

pub enum SessionEntry {
    SessionStarted { session_id: SessionId, schema_version: u32, started_at: chrono::DateTime<chrono::Utc>, cwd: std::path::PathBuf },
    UserMessage { id: MessageId, content: String },
    AssistantMessage { id: MessageId, parts: Vec<Part>, usage: Option<Usage>, stop_reason: Option<StopReason> },
    ToolStarted { call_id: ToolCallId, name: String, args: serde_json::Value },
    ToolFinished { call_id: ToolCallId, outcome: ToolOutcome },
    RunFinished { message: String },
    RunInterrupted { active_call: Option<ToolCallId> },
}

impl SessionWriter {
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self, SessionError>;
    pub async fn append(&mut self, entry: SessionEntry) -> Result<(), SessionError>;
    pub async fn finish(&mut self) -> Result<(), SessionError>;
    pub fn path(&self) -> &std::path::Path;
    pub fn seq(&self) -> u64;
}

pub async fn read_entries(path: impl AsRef<std::path::Path>) -> Result<Vec<SessionEntry>, SessionError>;
pub async fn list_sessions(dir: impl AsRef<std::path::Path>) -> Result<Vec<SessionMeta>, SessionError>;
```

**Acceptance:**
- Each `append` writes exactly one JSON object + `\n` + fsync
- `seq` is monotonically increasing
- Crashing mid-write produces a truncated final line; `read_entries` skips lines that don't parse, returns the rest
- `list_sessions` returns newest-first
- Tests: 10+ — round-trip, crash mid-write (truncate file at byte 50, read returns valid prefix), seq invariant, concurrent append fails loudly, file permissions 0600

**Forbidden:** No SQLite. No compaction. No in-memory buffering. No background thread for flush.

---
