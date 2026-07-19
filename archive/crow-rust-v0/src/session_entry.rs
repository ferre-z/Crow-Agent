//! Durable, JSONL-persisted record of a session.
//!
//! [`SessionEntry`] is the **on-disk** shape: each line of a session's
//! JSONL log is one of these. It is distinct from [`crate::AgentEvent`],
//! which is the transient in-memory stream pushed to consumers. The
//! two share variants but the durable envelope carries a `timestamp`
//! on every record so we can reconstruct a timeline after a crash.
//!
//! Spec §10 fixes this shape.

use serde::{Deserialize, Serialize};

use crate::event::{ToolOutcome, Usage};
use crate::ids::{MessageId, SessionId, Timestamp, ToolCallId};
use crate::message::Part;

/// One line in a session's JSONL log.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum SessionEntry {
    /// First line of a new session.
    SessionStarted {
        /// Schema version this log was written under.
        schema_version: u32,
        /// Identifier of the session being started.
        session_id: SessionId,
        /// Wall-clock start time.
        started_at: Timestamp,
        /// Working directory the session was launched from.
        cwd: std::path::PathBuf,
    },
    /// A user-typed message.
    UserMessage {
        /// Identifier of the new message.
        id: MessageId,
        /// Plain-text content the user sent.
        content: String,
        /// Wall-clock time the message was recorded.
        timestamp: Timestamp,
    },
    /// An assistant response.
    AssistantMessage {
        /// Identifier of the new message.
        id: MessageId,
        /// Ordered parts making up the assistant reply.
        parts: Vec<Part>,
        /// Token usage, if reported by the provider.
        usage: Option<Usage>,
        /// Why the model stopped, if known.
        stop_reason: Option<crate::event::StopReason>,
        /// Wall-clock time the message was recorded.
        timestamp: Timestamp,
    },
    /// A tool invocation was dispatched.
    ToolStarted {
        /// Identifier of the new tool call.
        call_id: ToolCallId,
        /// Tool name.
        name: String,
        /// Arguments as a JSON value.
        args: serde_json::Value,
        /// Wall-clock time the call was dispatched.
        timestamp: Timestamp,
    },
    /// A tool invocation finished.
    ToolFinished {
        /// Identifier of the tool call that finished.
        call_id: ToolCallId,
        /// Terminal outcome of the call.
        outcome: ToolOutcome,
        /// Wall-clock time the call finished.
        timestamp: Timestamp,
    },
    /// The run reached a terminal success state.
    RunFinished {
        /// Human-readable summary message.
        message: String,
        /// Wall-clock time the run finished.
        timestamp: Timestamp,
    },
    /// The run was interrupted (e.g. crash, kill, lost connection).
    RunInterrupted {
        /// The tool call that was in flight at the time, if any.
        active_call: Option<ToolCallId>,
        /// Wall-clock time the interruption was recorded.
        timestamp: Timestamp,
    },
    /// The run terminated with a structured failure.
    ///
    /// Distinct from [`SessionEntry::RunInterrupted`] (cooperative
    /// cancellation / kill) and [`SessionEntry::RunFinished`] (clean
    /// success). Recorded whenever the agent loop gives up due to a
    /// provider error, a context-limit hit, or a typed error from the
    /// policy layer.
    RunFailed {
        /// Stable, machine-readable error code (see
        /// [`crate::event::ErrorCode`]).
        code: crate::event::ErrorCode,
        /// Whether the failure is expected to be transient (the
        /// supervisor may retry the same request).
        retryable: bool,
        /// Human-readable failure message.
        message: String,
        /// Wall-clock time the failure was recorded.
        timestamp: Timestamp,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::SCHEMA_VERSION;
    use crate::ids::new_id;

    #[test]
    fn session_started_round_trips() {
        let entry = SessionEntry::SessionStarted {
            schema_version: SCHEMA_VERSION,
            session_id: SessionId(new_id()),
            started_at: Timestamp::now(),
            cwd: std::path::PathBuf::from("/tmp/proj"),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn user_message_round_trips() {
        let entry = SessionEntry::UserMessage {
            id: MessageId(new_id()),
            content: "hello".into(),
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn assistant_message_round_trips() {
        let entry = SessionEntry::AssistantMessage {
            id: MessageId(new_id()),
            parts: vec![Part::Text { text: "hi".into() }],
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 2,
            }),
            stop_reason: Some(crate::event::StopReason::EndTurn),
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn tool_started_round_trips() {
        let entry = SessionEntry::ToolStarted {
            call_id: ToolCallId(new_id()),
            name: "shell".into(),
            args: serde_json::json!({"cmd": "ls"}),
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn tool_finished_round_trips() {
        let entry = SessionEntry::ToolFinished {
            call_id: ToolCallId(new_id()),
            outcome: ToolOutcome::Success {
                output: "ok".into(),
                truncated: false,
            },
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn run_finished_round_trips() {
        let entry = SessionEntry::RunFinished {
            message: "done".into(),
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn run_interrupted_round_trips() {
        let entry = SessionEntry::RunInterrupted {
            active_call: Some(ToolCallId(new_id())),
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn run_interrupted_without_active_call_round_trips() {
        let entry = SessionEntry::RunInterrupted {
            active_call: None,
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn run_failed_round_trips() {
        let entry = SessionEntry::RunFailed {
            code: crate::event::ErrorCode("provider_error".into()),
            retryable: false,
            message: "stream invalid: malformed JSON".into(),
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: SessionEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn event_ordering_invariant_holds() {
        // Spec §10: a well-formed JSONL log interleaves SessionStarted,
        // messages, and tool records in a single monotonic timeline.
        let t0 = Timestamp::now();
        // Sleep a hair to guarantee monotonicity even on coarse clocks.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let t1 = Timestamp::now();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let t2 = Timestamp::now();

        let entries = vec![
            SessionEntry::SessionStarted {
                schema_version: SCHEMA_VERSION,
                session_id: SessionId(new_id()),
                started_at: t0,
                cwd: std::path::PathBuf::from("/tmp"),
            },
            SessionEntry::UserMessage {
                id: MessageId(new_id()),
                content: "go".into(),
                timestamp: t1,
            },
            SessionEntry::RunFinished {
                message: "ok".into(),
                timestamp: t2,
            },
        ];

        // Round-trip the whole log via JSON Lines to confirm we can
        // reconstruct an in-order timeline.
        let mut buffer = String::new();
        for entry in &entries {
            buffer.push_str(&serde_json::to_string(entry).expect("serialize"));
            buffer.push('\n');
        }
        let recovered: Vec<SessionEntry> = buffer
            .lines()
            .map(|line| serde_json::from_str(line).expect("deserialize"))
            .collect();
        assert_eq!(recovered, entries);
        // And the timestamps are still monotone.
        for window in recovered.windows(2) {
            let a = timestamp_of(&window[0]);
            let b = timestamp_of(&window[1]);
            assert!(a <= b, "timestamps must be monotone: {a:?} > {b:?}");
        }
    }

    /// Extract the relevant `Timestamp` from a [`SessionEntry`] for
    /// the ordering check above.
    fn timestamp_of(entry: &SessionEntry) -> Timestamp {
        match entry {
            SessionEntry::SessionStarted { started_at, .. } => *started_at,
            SessionEntry::UserMessage { timestamp, .. } => *timestamp,
            SessionEntry::AssistantMessage { timestamp, .. } => *timestamp,
            SessionEntry::ToolStarted { timestamp, .. } => *timestamp,
            SessionEntry::ToolFinished { timestamp, .. } => *timestamp,
            SessionEntry::RunFinished { timestamp, .. } => *timestamp,
            SessionEntry::RunInterrupted { timestamp, .. } => *timestamp,
            SessionEntry::RunFailed { timestamp, .. } => *timestamp,
        }
    }
}
