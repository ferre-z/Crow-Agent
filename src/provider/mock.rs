//! A test-only provider that replays a fixed sequence of events.
//!
//! In wave 1 this is the only provider. The real `genai` adapter lands
//! in wave 2. Tests use [`ScriptedProvider::from_fixture`] to load
//! JSONL fixtures and assert that downstream code consumes them
//! correctly.
//!
//! ## Fixture format
//!
//! One [`AgentEvent`] per line, serialised with `#[serde(tag = "type",
//! rename_all = "PascalCase")]` (see [`crate::event::AgentEvent`]). A
//! minimal example:
//!
//! ```text
//! {"type":"ModelStarted"}
//! {"type":"TextDelta","text":"Hello"}
//! {"type":"TextDelta","text":" world"}
//! {"type":"ModelFinished","usage":{"input_tokens":5,"output_tokens":2},"stop_reason":"EndTurn"}
//! {"type":"RunFinished","message":"done"}
//! ```
//!
//! Loading fails loudly (with a 1-based line number) on:
//! - empty or whitespace-only files,
//! - malformed JSON,
//! - an unknown event `type` tag.

use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use tokio_stream::Stream;
use tokio_util::sync::CancellationToken;

use crate::event::AgentEvent;
use crate::provider::{ModelRequest, Provider, ProviderError, ProviderStream};

/// A provider that replays a pre-loaded sequence of [`AgentEvent`]s.
#[derive(Debug, Clone)]
pub struct ScriptedProvider {
    events: Vec<AgentEvent>,
}

impl ScriptedProvider {
    /// Load a script from a JSONL fixture file. Each non-blank line must
    /// be a serialised [`AgentEvent`].
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::StreamInvalid`] when:
    /// - the file cannot be read (the error message includes the path),
    /// - the file is empty or contains only blank lines,
    /// - a line is malformed JSON, or
    /// - a line carries an unknown event `type` tag.
    ///
    /// Parse errors include the 1-based line number.
    pub fn from_fixture(path: impl AsRef<std::path::Path>) -> Result<Self, ProviderError> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            ProviderError::StreamInvalid(format!(
                "could not read fixture {}: {}",
                path.as_ref().display(),
                e,
            ))
        })?;
        let events = parse_jsonl(&content)?;
        Ok(Self { events })
    }

    /// Build a provider from an in-memory sequence of events. Useful
    /// for tests that synthesise events directly rather than loading a
    /// fixture file.
    #[must_use]
    pub fn from_events(events: Vec<AgentEvent>) -> Self {
        Self { events }
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    async fn stream(
        &self,
        _req: ModelRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError> {
        // Pre-flight cancellation: if the token is already fired, we
        // never even build the stream. The agent loop will see a clean
        // `Cancelled` and unwind without touching any event payloads.
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }

        let stream = ScriptedStream {
            iter: self.events.clone().into_iter(),
            cancel,
            finished: false,
        };

        Ok(ProviderStream {
            events: Box::pin(stream),
        })
    }
}

/// Inner stream that walks a fixed event list and observes a cancel
/// token between events.
///
/// We don't register a waker with the cancel token (the
/// `tokio_util::sync::CancellationToken::cancelled()` future does that
/// for a `select!`-based driver, but a polled-by-hand stream needs its
/// own registration). For the scripted fixture this is fine: a
/// consumer that drives the stream to completion will see cancellation
/// between events, and a consumer that hits a pre-cancelled token
/// short-circuits in [`ScriptedProvider::stream`].
struct ScriptedStream {
    iter: std::vec::IntoIter<AgentEvent>,
    cancel: CancellationToken,
    /// Latched to `true` after we surface a terminal item (cancel error
    /// or stream end) so subsequent polls return `Ready(None)`.
    finished: bool,
}

impl Stream for ScriptedStream {
    type Item = Result<AgentEvent, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        if self.cancel.is_cancelled() {
            self.finished = true;
            return Poll::Ready(Some(Err(ProviderError::Cancelled)));
        }
        match self.iter.next() {
            Some(event) => Poll::Ready(Some(Ok(event))),
            None => {
                self.finished = true;
                Poll::Ready(None)
            }
        }
    }
}

/// Parse JSONL content into a sequence of [`AgentEvent`]s.
///
/// # Errors
///
/// See [`ScriptedProvider::from_fixture`] for the full list of failure
/// modes. Every parse error includes the 1-based line number where the
/// problem was detected.
fn parse_jsonl(content: &str) -> Result<Vec<AgentEvent>, ProviderError> {
    if content.trim().is_empty() {
        return Err(ProviderError::StreamInvalid(
            "fixture file is empty".to_string(),
        ));
    }

    let mut events = Vec::new();
    let mut saw_event = false;
    for (idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        saw_event = true;
        let line_no = idx + 1;
        let event: AgentEvent = serde_json::from_str(line)
            .map_err(|e| ProviderError::StreamInvalid(format!("fixture line {line_no}: {e}")))?;
        events.push(event);
    }

    if !saw_event {
        // Belt and braces: if the file is all blank lines we treat it
        // the same as an empty file.
        return Err(ProviderError::StreamInvalid(
            "fixture file is empty".to_string(),
        ));
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    //! Unit tests for the scripted provider. All of these live in the
    //! same module as the code under test so they can poke at private
    //! helpers (`parse_jsonl`) when needed.
    //!
    //! Fixture-based tests use the committed `tests/fixtures/*.jsonl`
    //! files; the "loud failure" tests synthesise broken content via
    //! `tempfile` so we can assert on exact error messages.

    use super::*;
    use crate::ids::new_id;
    use crate::ids::{RunId, SessionId, Timestamp, ToolCallId};
    use std::io::Write;
    use std::path::PathBuf;

    /// Path to the committed JSONL fixture with the given file name.
    fn fixture_path(name: &str) -> PathBuf {
        // `CARGO_MANIFEST_DIR` is set by Cargo at build time and points
        // at the crate root, which is exactly where `tests/fixtures/`
        // lives.
        let manifest =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
        PathBuf::from(manifest).join("tests/fixtures").join(name)
    }

    /// Write `content` to a fresh temp file and return the path.
    fn write_fixture(content: &str) -> PathBuf {
        let mut tmp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .expect("create temp fixture");
        tmp.write_all(content.as_bytes()).expect("write fixture");
        tmp.into_temp_path().to_path_buf()
    }

    /// Drain a `ProviderStream` into a `Vec<AgentEvent>`. Any per-item
    /// error fails the test loudly.
    async fn collect_ok(stream: ProviderStream) -> Vec<AgentEvent> {
        use tokio_stream::StreamExt;
        let mut out = Vec::new();
        let mut s = stream.events;
        while let Some(item) = s.next().await {
            out.push(item.expect("stream yielded an error"));
        }
        out
    }

    /// A trivial `ModelRequest` for tests that don't care about
    /// request contents.
    fn dummy_request() -> ModelRequest {
        ModelRequest {
            messages: Vec::new(),
            tools_schema: serde_json::Value::Null,
        }
    }

    /// `scripted_text_only.jsonl` must round-trip to the exact event
    /// sequence committed in the fixture file.
    #[tokio::test]
    async fn text_only_fixture_replays_to_identical_events() {
        let provider = ScriptedProvider::from_fixture(fixture_path("scripted_text_only.jsonl"))
            .expect("fixture should load");
        let cancel = CancellationToken::new();
        let stream = provider
            .stream(dummy_request(), cancel)
            .await
            .expect("stream should open");
        let events = collect_ok(stream).await;

        let expected = vec![
            AgentEvent::ModelStarted,
            AgentEvent::TextDelta {
                text: "Hello".to_string(),
            },
            AgentEvent::TextDelta {
                text: " world".to_string(),
            },
            AgentEvent::ModelFinished {
                usage: crate::event::Usage {
                    input_tokens: 5,
                    output_tokens: 2,
                },
                stop_reason: crate::event::StopReason::EndTurn,
            },
            AgentEvent::RunFinished {
                message: "done".to_string(),
            },
        ];

        assert_eq!(events, expected);
    }

    /// `scripted_text_plus_tool_call.jsonl` covers the tool path:
    /// `ToolStarted` -> `ToolOutput` -> `ToolFinished`. The result is
    /// the exact sequence committed in the fixture.
    #[tokio::test]
    async fn text_plus_tool_call_fixture_replays() {
        let provider =
            ScriptedProvider::from_fixture(fixture_path("scripted_text_plus_tool_call.jsonl"))
                .expect("fixture should load");
        let cancel = CancellationToken::new();
        let stream = provider
            .stream(dummy_request(), cancel)
            .await
            .expect("stream should open");
        let events = collect_ok(stream).await;

        // Round-trip the fixture through `AgentEvent`'s deserialiser
        // ourselves, then assert byte-for-byte equality. This is
        // stricter than reconstructing expected events by hand: it
        // would catch a fixture that accidentally encoded a wrong
        // `call_id` or `chunk` bytes.
        let raw = std::fs::read_to_string(fixture_path("scripted_text_plus_tool_call.jsonl"))
            .expect("read fixture");
        let expected: Vec<AgentEvent> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("line should parse"))
            .collect();

        assert_eq!(events, expected);
    }

    /// `scripted_two_turns.jsonl` covers a multi-invocation script.
    /// Two `ModelStarted`...`ModelFinished` cycles back to back, then a
    /// terminal `RunFinished`.
    #[tokio::test]
    async fn two_turns_fixture_replays() {
        let provider = ScriptedProvider::from_fixture(fixture_path("scripted_two_turns.jsonl"))
            .expect("fixture should load");
        let cancel = CancellationToken::new();
        let stream = provider
            .stream(dummy_request(), cancel)
            .await
            .expect("stream should open");
        let events = collect_ok(stream).await;

        // Same byte-for-byte round-trip trick: a typo in the fixture
        // would show up as a deserialisation mismatch.
        let raw = std::fs::read_to_string(fixture_path("scripted_two_turns.jsonl"))
            .expect("read fixture");
        let expected: Vec<AgentEvent> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("line should parse"))
            .collect();

        assert_eq!(events, expected);
        // Sanity check: confirm we really got two model invocations.
        let model_starts = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ModelStarted))
            .count();
        let model_finishes = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ModelFinished { .. }))
            .count();
        assert_eq!(model_starts, 2, "fixture should drive two turns");
        assert_eq!(model_finishes, 2, "fixture should finish two turns");
    }

    /// An event `type` that doesn't exist in the enum must fail at
    /// fixture load, not silently decode to something else.
    #[tokio::test]
    async fn unknown_event_type_fails_loudly() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("fixture.jsonl");
        std::fs::write(&path, "{\"type\":\"QuantumFluxDelta\",\"text\":\"wat\"}\n")
            .expect("write fixture");
        let err = ScriptedProvider::from_fixture(&path).expect_err("unknown type should error");
        match err {
            ProviderError::StreamInvalid(msg) => {
                assert!(
                    msg.contains("QuantumFluxDelta"),
                    "error should name the unknown type, got: {msg}",
                );
                assert!(
                    msg.contains("line 1"),
                    "error should cite line 1, got: {msg}",
                );
            }
            other => panic!("expected StreamInvalid, got {other:?}"),
        }
    }

    /// A completely empty file is a user error, not a valid (but
    /// uneventful) script.
    #[tokio::test]
    async fn empty_file_fails_loudly() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("fixture.jsonl");
        std::fs::write(&path, "").expect("write fixture");
        let err = ScriptedProvider::from_fixture(&path).expect_err("empty file should error");
        match err {
            ProviderError::StreamInvalid(msg) => {
                assert!(
                    msg.contains("empty"),
                    "error should mention the file is empty, got: {msg}",
                );
            }
            other => panic!("expected StreamInvalid, got {other:?}"),
        }
    }

    /// A file with only blank lines is treated the same as empty:
    /// loud failure.
    #[tokio::test]
    async fn whitespace_only_file_fails_loudly() {
        let path = write_fixture("\n   \n\t\n");
        let err =
            ScriptedProvider::from_fixture(&path).expect_err("whitespace-only file should error");
        assert!(matches!(err, ProviderError::StreamInvalid(_)));
    }

    /// A bad JSON line must fail at fixture load, and the error must
    /// name the line number so a human can find the bug.
    #[tokio::test]
    async fn malformed_line_fails_with_line_number() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("fixture.jsonl");
        std::fs::write(&path, "not json\n").expect("write fixture");
        let err = ScriptedProvider::from_fixture(&path).expect_err("malformed JSON should error");
        match err {
            ProviderError::StreamInvalid(msg) => {
                assert!(
                    msg.contains("line 1"),
                    "error should cite line 1, got: {msg}",
                );
            }
            other => panic!("expected StreamInvalid, got {other:?}"),
        }
    }

    /// `from_events` is the in-memory constructor. The stream it
    /// produces must replay the input vector verbatim.
    #[tokio::test]
    async fn from_events_replays_in_memory_list() {
        let run_id = RunId(new_id());
        let session_id = SessionId(new_id());
        let started_at = Timestamp::now();
        let call_id = ToolCallId(new_id());
        let events = vec![
            AgentEvent::RunStarted {
                run_id,
                session_id,
                started_at,
            },
            AgentEvent::ModelStarted,
            AgentEvent::ToolStarted {
                call_id,
                name: "shell".to_string(),
                args: serde_json::json!({"cmd": "ls"}),
            },
            AgentEvent::RunFinished {
                message: "ok".to_string(),
            },
        ];
        let expected = events.clone();

        let provider = ScriptedProvider::from_events(events);
        let stream = provider
            .stream(dummy_request(), CancellationToken::new())
            .await
            .expect("stream should open");
        let actual = collect_ok(stream).await;

        assert_eq!(actual, expected);
    }

    /// A pre-cancelled token must short-circuit `stream()` itself: no
    /// events are yielded, and the error is `ProviderError::Cancelled`.
    #[tokio::test]
    async fn pre_cancelled_token_returns_cancelled_error() {
        let provider = ScriptedProvider::from_events(vec![AgentEvent::ModelStarted]);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let err = provider
            .stream(dummy_request(), cancel)
            .await
            .expect_err("cancelled stream should error");
        assert!(
            matches!(err, ProviderError::Cancelled),
            "expected Cancelled, got {err:?}",
        );
    }
}
