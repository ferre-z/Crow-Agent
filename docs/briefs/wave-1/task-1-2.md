### Task 1.2 — ID + event-envelope + message types

**Files:**
- Create: `src/ids.rs`, `src/event.rs`, `src/message.rs`, `src/session_entry.rs`
- Modify: `src/lib.rs` (re-export public types)

**Spec references:** v0 spec §9 (provider events), §10 (message + event model + durable entries).

**CRITICAL spec constraints (reviewer-flagged, all must be addressed):**
- ALL public types derive `Serialize, Deserialize, Debug, Clone, PartialEq` (spec §10)
- `RunFailed { code, retryable, message }` — field order matches spec §9 EXACTLY
- `Role` is `User | Assistant | ToolResult` only — NO `System` variant
- `Part::ToolResult` carries `truncated: bool` and `display: Option<DisplayDetails>` per spec §10
- `SessionEntry` is the durable envelope (per spec §10), not just `AgentEvent`
- `ErrorCode` is `pub struct ErrorCode(pub String)` and serializes as a plain string (`#[serde(transparent)]`)
- **All `AgentEvent` and `SessionEntry` variants use struct variants** (not newtype variants) because `#[serde(tag = "type")]` requires struct variants. The fixture format from 1.4 will be adjusted to use struct form.
- `Timestamp` is the newtype from task 1.1 — use it everywhere instead of `chrono::DateTime<Utc>`.

**Updated fixture format (1.4 will use this):**
```json
{"type":"ModelStarted"}
{"type":"TextDelta","text":"Hello"}
{"type":"TextDelta","text":" world"}
{"type":"ModelFinished","usage":{"input_tokens":5,"output_tokens":2},"stop_reason":"EndTurn"}
{"type":"RunFinished","message":"done"}
```

Note `TextDelta` is `TextDelta { text: String }` (struct variant), not `TextDelta(String)`.

**Interfaces (exact):**

```rust
// ids.rs
pub use ulid::Ulid;
pub fn new_id() -> Ulid;
pub struct SessionId(pub Ulid);
pub struct RunId(pub Ulid);
pub struct MessageId(pub Ulid);
pub struct ToolCallId(pub Ulid);
pub struct ToolResultId(pub Ulid);
// All derive Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash

/// Project-owned timestamp. Stores `SystemTime` as Unix milliseconds
/// in JSON. No `chrono` dependency.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp(#[serde(with = "timestamp_serde")] pub std::time::SystemTime);

mod timestamp_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};
    pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let ms = t.duration_since(UNIX_EPOCH).map_err(serde::ser::Error::custom)?
            .as_millis() as u64;
        s.serialize_u64(ms)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + std::time::Duration::from_millis(ms))
    }
}

impl Timestamp {
    pub fn now() -> Self { Self(SystemTime::now()) }
}
```

```rust
// event.rs
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum AgentEvent {
    RunStarted { run_id: RunId, session_id: SessionId, started_at: Timestamp },
    ModelStarted,
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolStarted { call_id: ToolCallId, name: String, args: serde_json::Value },
    ToolOutput { call_id: ToolCallId, stream: ToolStream, chunk: Vec<u8> },
    ToolFinished { call_id: ToolCallId, result: ToolOutcome },
    ModelFinished { usage: Usage, stop_reason: StopReason },
    RunFinished { message: String },
    RunCancelled,
    RunFailed { code: ErrorCode, retryable: bool, message: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ToolStream { Stdout, Stderr }

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    Success { output: String, truncated: bool },
    Error { code: ErrorCode, message: String, truncated: bool },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Usage { pub input_tokens: u32, pub output_tokens: u32 }

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum StopReason { EndTurn, ToolUse, MaxTokens, Cancellation, Error }

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(transparent)]
pub struct ErrorCode(pub String);
```

```rust
// message.rs
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role { User, Assistant, ToolResult }

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Message {
    pub id: MessageId,
    pub role: Role,
    pub parts: Vec<Part>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "PascalCase")]
pub enum Part {
    Text { text: String },
    Reasoning { text: String },
    ToolCall { id: ToolCallId, name: String, args: serde_json::Value },
    ToolResult { call_id: ToolCallId, output: String, is_error: bool, truncated: bool, display: Option<DisplayDetails> },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DisplayDetails { pub path: Option<std::path::PathBuf>, pub line_count: Option<u32>, pub byte_size: Option<u64> }
```

```rust
// session_entry.rs (durable envelope, spec §10)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum SessionEntry {
    SessionStarted {
        schema_version: u32,
        session_id: SessionId,
        started_at: Timestamp,
        cwd: std::path::PathBuf,
    },
    UserMessage { id: MessageId, content: String, timestamp: Timestamp },
    AssistantMessage { id: MessageId, parts: Vec<Part>, usage: Option<Usage>, stop_reason: Option<StopReason>, timestamp: Timestamp },
    ToolStarted { call_id: ToolCallId, name: String, args: serde_json::Value, timestamp: Timestamp },
    ToolFinished { call_id: ToolCallId, outcome: ToolOutcome, timestamp: Timestamp },
    RunFinished { message: String, timestamp: Timestamp },
    RunInterrupted { active_call: Option<ToolCallId>, timestamp: Timestamp },
}
```

**Acceptance:**
- All types `Serialize/Deserialize` to JSON and round-trip
- A `SessionEntry` round-trip test (this is the actual durability contract)
- `SCHEMA_VERSION == 1`
- `ErrorCode("stream_invalid")` serializes to `"stream_invalid"` (transparent)
- `AgentEvent::RunFailed` field order matches spec §9 exactly: `code, retryable, message`
- `Role` has no `System` variant
- `Part::ToolResult` has `truncated` and `display` fields
- `Timestamp::now()` round-trips to within 10ms
- Tests cover (15+):
  - ID uniqueness
  - Message round-trip
  - SessionEntry round-trip (one test per variant)
  - Event ordering invariant
  - ErrorCode transparent serialization
  - Every AgentEvent variant round-trips
  - Every SessionEntry variant round-trips
  - Timestamp Serialize/Deserialize round-trip
  - ToolStream case-insensitive rename
- `cargo test` exits 0
- All public types have `///` doc comments

**Forbidden:** No provider-specific types. No `genai` import yet. No `async_trait` in types. No `Display` impls that leak internals. No `System` role. No `chrono`.
