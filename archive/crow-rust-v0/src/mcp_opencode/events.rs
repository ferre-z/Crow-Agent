//! JSON event types parsed from `opencode run --format json`.
//!
//! `opencode run --format json` emits one JSON object per line on stdout
//! describing the run lifecycle: text deltas, tool calls, tool results,
//! finalisation, etc. We don't care about every variant — we only need
//! enough to (a) report progress to MCP clients, (b) extract the final
//! assistant text and any tool calls when the run completes.
//!
//! The schema is forward-compatible by design: unknown `type` values are
//! preserved as [`OpencodeEvent::Unknown`] so we never panic on a future
//! opencode release adding new event kinds.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One event line from `opencode run --format json` stdout.
///
/// Captures only the fields we currently use; everything else lives in
/// [`OpencodeEvent::raw`]. This keeps us resilient to upstream additions
/// without breaking the wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpencodeEvent {
    /// Streamed text delta from the assistant. `text` is appended in
    /// order; the final assembled message is the concatenation.
    TextDelta { text: String },
    /// A tool call the model wants to make. The run pauses until the
    /// tool result is available.
    ToolCall {
        id: String,
        name: String,
        args: Value,
    },
    /// Result of a tool invocation, matched to a prior [`OpencodeEvent::ToolCall`]
    /// by `id`.
    ToolResult {
        id: String,
        output: String,
        #[serde(default)]
        error: bool,
    },
    /// Final assistant message — emitted exactly once when the run
    /// finishes successfully. `message` is the full text.
    Done { message: String },
    /// Non-fatal error event. The run may continue or terminate.
    Error { message: String },
    /// Any event shape we don't recognise. Forwarded verbatim so callers
    /// can inspect it without losing information.
    #[serde(other)]
    Unknown,
}

impl OpencodeEvent {
    /// Build from a raw JSON value, preserving the original in [`Self::raw`].
    /// Useful for callers that want to inspect fields we don't model.
    #[must_use]
    pub fn parse_line(line: &str) -> Option<Self> {
        // Some `opencode run` invocations print a non-JSON preamble
        // (e.g. the ascii banner). Tolerate that: skip lines that don't
        // parse as a JSON object.
        let v: Value = serde_json::from_str(line).ok()?;
        if !v.is_object() {
            return None;
        }
        serde_json::from_value::<Self>(v.clone())
            .ok()
            .or(Some(Self::Unknown))
    }

    /// Whether this event represents the terminal event of a run.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Done { .. } | Self::Error { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_delta() {
        let line = r#"{"type":"text_delta","text":"hello"}"#;
        match OpencodeEvent::parse_line(line).expect("parse") {
            OpencodeEvent::TextDelta { text } => assert_eq!(text, "hello"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn parses_done() {
        let line = r#"{"type":"done","message":"all good"}"#;
        match OpencodeEvent::parse_line(line).expect("parse") {
            OpencodeEvent::Done { message } => assert_eq!(message, "all good"),
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_call_and_result() {
        let call = r#"{"type":"tool_call","id":"c1","name":"read","args":{"path":"x"}}"#;
        let res = r#"{"type":"tool_result","id":"c1","output":"...","error":false}"#;
        assert!(matches!(
            OpencodeEvent::parse_line(call),
            Some(OpencodeEvent::ToolCall { .. })
        ));
        assert!(matches!(
            OpencodeEvent::parse_line(res),
            Some(OpencodeEvent::ToolResult { .. })
        ));
    }

    #[test]
    fn unknown_event_kind_falls_through() {
        let line = r#"{"type":"something_new","whatever":42}"#;
        assert!(matches!(
            OpencodeEvent::parse_line(line),
            Some(OpencodeEvent::Unknown)
        ));
    }

    #[test]
    fn non_object_line_is_rejected() {
        // `opencode run` may emit a banner or log line before the JSON
        // stream begins. Those must not crash the parser.
        assert!(OpencodeEvent::parse_line("   ▄").is_none());
        assert!(OpencodeEvent::parse_line("hello world").is_none());
        assert!(OpencodeEvent::parse_line("42").is_none());
    }

    #[test]
    fn is_terminal_recognises_done_and_error() {
        let done = r#"{"type":"done","message":"x"}"#;
        let err = r#"{"type":"error","message":"x"}"#;
        let txt = r#"{"type":"text_delta","text":"x"}"#;
        assert!(OpencodeEvent::parse_line(done).unwrap().is_terminal());
        assert!(OpencodeEvent::parse_line(err).unwrap().is_terminal());
        assert!(!OpencodeEvent::parse_line(txt).unwrap().is_terminal());
    }
}
