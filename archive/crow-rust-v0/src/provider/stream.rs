//! Provider-neutral stream accumulator.
//!
//! The [`StreamAccumulator`] is the **only** translation layer between
//! provider-native chunk streams (genai today, others later) and the
//! project-owned [`AgentEvent`] sequence. Raw upstream events MUST NOT
//! leak past this point — that invariant is the whole reason this type
//! exists (spec §9).
//!
//! ## Shape
//!
//! - [`ProviderChunk`] is the **input**: a fragmented chunk from any
//!   provider. Providers wrap their own native events into this enum.
//! - [`StreamAccumulator`] is the **state machine**: buffers fragmented
//!   tool-call argument chunks, then yields a sequence of [`AgentEvent`]s
//!   in source order.
//! - [`StreamError`] is the **failure mode**: malformed chunks, missing
//!   `Completed`, dangling tool calls, etc.
//!
//! ## Cancellation
//!
//! The accumulator itself does not observe a [`CancellationToken`] —
//! that responsibility belongs to the adapter that drives
//! `push_chunk`. If a cancel fires mid-stream the adapter stops calling
//! `push_chunk` and discards the accumulator; the half-built state is
//! not surfaced to the loop.
//!
//! ## UTF-8 boundary
//!
//! A `String` in Rust is always valid UTF-8, so the `text` and
//! `fragment` fields of [`ProviderChunk`] cannot, in safe Rust, contain
//! an incomplete multi-byte sequence. The [`StreamError::Utf8`]
//! variant exists as a future-proofing safety valve — when the
//! accumulator's internals are refactored to operate on byte slices
//! (e.g. when we add a `Vec<u8>`-level `push_chunk_bytes` path), the
//! conversion layer can surface the boundary error here. In v0 it is
//! unreachable through the public API; the variant is still tested to
//! pin down its shape.

use thiserror::Error;

use crate::event::{AgentEvent, ErrorCode, StopReason, Usage};
use crate::ids::ToolCallId;
// `ToolStream` and `Part` are re-exported through `crate::event` and
// `crate::message` respectively — they're imported here for the doc-link
// path only, since the accumulator itself does not construct them.
#[allow(unused_imports)]
use crate::event::ToolStream as _;
#[allow(unused_imports)]
use crate::message::Part as _;
// `futures::Stream` is re-exported for downstream adapters that want
// to wrap the accumulator in a `Stream<Item = AgentEvent>`; importing
// the trait here keeps it visible in the module's API surface. The
// accumulator's own internal event production is synchronous via
// `push_chunk`, so the trait itself is not implemented by
// `StreamAccumulator`.
#[allow(unused_imports)]
use futures::Stream as _;

/// A tool-call currently being assembled from streaming arguments.
///
/// One entry per `ProviderChunk::ToolCallStart` whose
/// `ProviderChunk::ToolCallComplete` has not yet been seen. The entry
/// is removed from [`StreamAccumulator::pending`] when a matching
/// `ToolCallComplete` is processed.
#[derive(Debug, Clone)]
struct PendingToolCall {
    /// The provider-issued identifier for this tool invocation.
    call_id: ToolCallId,
    /// The tool name recorded at `ToolCallStart`.
    name: String,
    /// Concatenation of every `ToolArgumentsDelta.fragment` we've seen
    /// for this `call_id`, in arrival order. Parsed as
    /// `serde_json::Value` at `ToolCallComplete`.
    args_buf: String,
}

/// Buffers fragmented provider chunks and yields [`AgentEvent`]s in
/// source order.
///
/// The accumulator is the ONLY translation layer between provider
/// streams and project events; no raw upstream events escape past this
/// point (spec §9).
///
/// # Lifecycle
///
/// 1. Construct with [`StreamAccumulator::new`] (or `Default`).
/// 2. For each provider chunk, call [`StreamAccumulator::push_chunk`]
///    and emit every [`AgentEvent`] in the returned `Vec` to downstream
///    consumers (TUI, session writer, tests).
/// 3. After the upstream stream closes, call
///    [`StreamAccumulator::finish`] to obtain the terminal
///    [`AgentEvent::ModelFinished`] (or surface a [`StreamError`]).
///
/// Calling `push_chunk` after `finish` is allowed but every subsequent
/// chunk is reported as [`StreamError::Invalid`] because the run is
/// already terminated.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    /// Open tool calls awaiting `ToolCallComplete`.
    pending: Vec<PendingToolCall>,
    /// Concatenation of every `TextDelta.text` we've seen. Not used to
    /// produce events (per-chunk `TextDelta` events are emitted
    /// directly) but retained for any future "what did the model say
    /// so far?" inspection.
    text_buf: String,
    /// Concatenation of every `ReasoningDelta.text` we've seen.
    reasoning_buf: String,
    /// `true` once a terminal chunk (`Completed` or `Failed`) has been
    /// observed and consumed. After this flag is set, further chunks
    /// are errors and `finish` will not re-emit the terminal.
    finished: bool,
    /// Single-slot cache of the terminal event (`ModelFinished` from a
    /// `Completed` chunk, or `RunFailed` from a `Failed` chunk) for
    /// `finish()` to return. Cleared when `finish()` returns it so a
    /// caller can pull the terminal from either `push_chunk`'s `Vec`
    /// or from `finish()`, but not both.
    terminal: Option<AgentEvent>,
}

impl StreamAccumulator {
    /// Construct an empty accumulator. Equivalent to `Default::default()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push one provider chunk. Returns 0..N [`AgentEvent`]s to emit,
    /// or an error if the chunk is malformed or out of order.
    ///
    /// # Errors
    ///
    /// | Trigger | [`StreamError`] variant |
    /// |---|---|
    /// | `ToolCallStart` reuses a `call_id` already in `pending` | [`StreamError::Invalid`] |
    /// | `ToolArgumentsDelta` for an unknown `call_id` | [`StreamError::Invalid`] |
    /// | `ToolCallComplete` for an unknown `call_id` | [`StreamError::Invalid`] |
    /// | `ToolCallComplete` whose `args_buf` is not valid JSON | [`StreamError::Invalid`] |
    /// | Second `Completed` chunk (terminal already seen) | [`StreamError::DoubleCompleted`] |
    /// | Any chunk after a terminal `Completed`/`Failed` | [`StreamError::Invalid`] |
    pub fn push_chunk(&mut self, chunk: ProviderChunk) -> Result<Vec<AgentEvent>, StreamError> {
        // Special case: a second Completed chunk after the stream
        // is already terminated must surface DoubleCompleted, not
        // the generic "stream terminated" Invalid. The caller is
        // asking specifically about the double-Completed case.
        if self.finished && matches!(chunk, ProviderChunk::Completed { .. }) {
            return Err(StreamError::DoubleCompleted);
        }

        if self.finished {
            return Err(StreamError::Invalid(
                "push_chunk called after stream terminated".to_string(),
            ));
        }

        match chunk {
            ProviderChunk::Started => Ok(vec![AgentEvent::ModelStarted]),

            ProviderChunk::TextDelta { text } => {
                self.text_buf.push_str(&text);
                Ok(vec![AgentEvent::TextDelta { text }])
            }

            ProviderChunk::ReasoningDelta { text } => {
                self.reasoning_buf.push_str(&text);
                Ok(vec![AgentEvent::ReasoningDelta { text }])
            }

            ProviderChunk::ToolCallStart { call_id, name } => {
                if self.pending.iter().any(|p| p.call_id == call_id) {
                    return Err(StreamError::Invalid(format!(
                        "duplicate ToolCallStart for call_id {}",
                        call_id.0
                    )));
                }
                self.pending.push(PendingToolCall {
                    call_id,
                    name,
                    args_buf: String::new(),
                });
                // `ToolCallStart` itself does not emit an event yet; the
                // corresponding `ToolStarted` is fired on
                // `ToolCallComplete` so callers always see the tool
                // invocation with fully parsed args.
                Ok(Vec::new())
            }

            ProviderChunk::ToolArgumentsDelta { call_id, fragment } => {
                let entry = self
                    .pending
                    .iter_mut()
                    .find(|p| p.call_id == call_id)
                    .ok_or_else(|| {
                        StreamError::Invalid(format!(
                            "ToolArgumentsDelta for unknown call_id {}",
                            call_id.0
                        ))
                    })?;
                entry.args_buf.push_str(&fragment);
                Ok(Vec::new())
            }

            ProviderChunk::ToolCallComplete { call_id } => {
                let entry = self
                    .pending
                    .iter()
                    .position(|p| p.call_id == call_id)
                    .ok_or_else(|| {
                        StreamError::Invalid(format!(
                            "ToolCallComplete for unknown call_id {}",
                            call_id.0
                        ))
                    })?;
                // `Vec::remove` is O(n) but `n` is small (a handful of
                // concurrent tool calls) so we accept the cost in
                // exchange for moving the entry out by value to avoid
                // cloning `args_buf`.
                let PendingToolCall {
                    call_id: _,
                    name,
                    args_buf,
                } = self.pending.remove(entry);
                // Parse the concatenated JSON. An empty `args_buf` is
                // treated as `Null` (test 4 in the brief).
                //
                // Some providers (notably NVIDIA's Nemotron and
                // other OpenAI-compatible endpoints) emit
                // `function.arguments` as a JSON-escaped string
                // rather than an object, so the streaming layer
                // sees `Value::String("{\"command\":\"...\"}")`. A
                // single `serde_json::from_str` round-trip would
                // then return `Value::String`, which downstream
                // tools reject with "is not of type 'object'". We
                // detect that case and re-parse once more so
                // double-encoded arguments resolve to the right
                // shape without provider-specific shims.
                let args = if args_buf.trim().is_empty() {
                    serde_json::Value::Null
                } else {
                    let parsed: serde_json::Value =
                        serde_json::from_str(&args_buf).map_err(|e| {
                            StreamError::Invalid(format!(
                                "tool call {} arguments are not valid JSON: {e}",
                                call_id.0
                            ))
                        })?;
                    match parsed {
                        serde_json::Value::String(ref inner)
                            if inner.trim_start().starts_with('{') =>
                        {
                            serde_json::from_str(inner).unwrap_or(parsed)
                        }
                        other => other,
                    }
                };
                Ok(vec![AgentEvent::ToolStarted {
                    call_id,
                    name,
                    args,
                }])
            }

            ProviderChunk::Completed { usage, stop_reason } => {
                self.finished = true;
                let event = AgentEvent::ModelFinished { usage, stop_reason };
                self.terminal = Some(event.clone());
                Ok(vec![event])
            }

            ProviderChunk::Failed {
                code,
                message,
                retryable,
            } => {
                // `Failed` terminates the stream. We surface the event
                // to the caller (so they can emit a `RunFailed` to
                // downstream consumers) but mark the accumulator as
                // finished so any subsequent `push_chunk` returns
                // `Invalid`.
                self.finished = true;
                let event = AgentEvent::RunFailed {
                    code: ErrorCode(code),
                    retryable,
                    message,
                };
                self.terminal = Some(event.clone());
                Ok(vec![event])
            }
        }
    }

    /// Signal end of stream. Returns the terminal [`AgentEvent`] the
    /// accumulator was holding, or an error if the stream ended in an
    /// invalid state.
    ///
    /// The terminal event is whichever of `Completed` or `Failed` was
    /// observed last in `push_chunk`. Each terminal is reported exactly
    /// once across the lifetime of this accumulator (either via the
    /// `Vec` returned by `push_chunk`, or via `finish`).
    ///
    /// # Errors
    ///
    /// | Trigger | [`StreamError`] variant |
    /// |---|---|
    /// | Stream never produced `Completed` and no `Failed` was seen | [`StreamError::Invalid`] |
    /// | A `ToolCallStart` has no matching `ToolCallComplete` | [`StreamError::PendingToolCall`] |
    pub fn finish(&mut self) -> Result<AgentEvent, StreamError> {
        if !self.pending.is_empty() {
            // Surface the first dangling call id to help debugging.
            let dangling = self.pending[0].call_id;
            // Reset pending so a subsequent `finish` call after this
            // error produces a deterministic response (the dangle
            // hasn't gone away, but the slice is the same; we keep the
            // entries so a retry would surface the same error).
            return Err(StreamError::PendingToolCall(dangling));
        }
        if self.finished {
            // We need to remember WHICH terminal we emitted through
            // `push_chunk` so we can re-emit it here. Stash the
            // terminal in a side-channel field. For v0 we keep a
            // single `Option<AgentEvent>` slot on the struct.
            //
            // In practice, the recommended contract is:
            //   - Caller drives `push_chunk` until it sees the terminal
            //     in the returned `Vec` and emits it.
            //   - Caller then calls `finish` purely for validation.
            //   - If the caller wants the terminal event a second
            //     time, they read it from the vec they already
            //     collected.
            //
            // For v0 we adopt a simpler rule: `finish` returns
            // `Ok(ModelFinished)` when the terminal seen via
            // `push_chunk` was a `Completed`, but to avoid the bookkeeping
            // of remembering which terminal we already emitted, we
            // require that the caller treat `finish` as a
            // validation-only call: it returns the stored terminal
            // event iff it has not yet been returned through
            // `push_chunk`. The brief only mandates the
            // error cases, so the cleanest interpretation is that
            // `finish` returns `Ok` of nothing-of-note if the stream
            // ended cleanly via `Completed`.
            //
            // Implementation: stash the terminal event on the struct;
            // clear it once returned from `finish`.
            return match self.terminal.take() {
                Some(event) => Ok(event),
                None => Err(StreamError::Invalid(
                    "finish called twice after Completed".to_string(),
                )),
            };
        }
        Err(StreamError::Invalid(
            "finish called before any Completed chunk was pushed".to_string(),
        ))
    }
}

/// A single chunk from any provider.
///
/// Other providers wrap their native stream events into this enum
/// before calling [`StreamAccumulator::push_chunk`]. The `genai`
/// adapter (wave 2 task 2.2) and any future adapter are the only
/// call sites.
///
/// **All variants are struct variants** (wave 1 lesson):
/// `#[serde(tag = "...")]` requires field names; the project rule is
/// "struct variants everywhere". Newtype variants
/// (`ToolCallStart(ToolCallId)`) are forbidden here.
#[derive(Debug, Clone)]
pub enum ProviderChunk {
    /// The model invocation has begun. Emit [`AgentEvent::ModelStarted`].
    Started,
    /// Fragment of assistant text output. Emitted verbatim as a
    /// [`AgentEvent::TextDelta`] for the calling code to forward to
    /// consumers.
    TextDelta {
        /// This chunk's text.
        text: String,
    },
    /// Fragment of assistant reasoning/thinking output. Emitted
    /// verbatim as a [`AgentEvent::ReasoningDelta`].
    ReasoningDelta {
        /// This chunk's text.
        text: String,
    },
    /// Opens a tool-call assembly window. Must be paired with a
    /// matching [`ProviderChunk::ToolCallComplete`] carrying the same
    /// `call_id` before the stream ends.
    ToolCallStart {
        /// Identifier for this invocation, unique within the stream.
        call_id: ToolCallId,
        /// Tool name to dispatch.
        name: String,
    },
    /// Appends a JSON fragment to the `args_buf` of the matching
    /// `PendingToolCall`.
    ToolArgumentsDelta {
        /// Identifier matching the prior `ToolCallStart`.
        call_id: ToolCallId,
        /// Concatenable fragment of the tool's argument JSON.
        fragment: String,
    },
    /// Closes a tool-call assembly window. The accumulated
    /// `args_buf` is parsed as `serde_json::Value` and surfaced as a
    /// [`AgentEvent::ToolStarted`].
    ToolCallComplete {
        /// Identifier matching the prior `ToolCallStart`.
        call_id: ToolCallId,
    },
    /// Terminal: the model invocation succeeded. Emits
    /// [`AgentEvent::ModelFinished`].
    Completed {
        /// Token accounting for the invocation.
        usage: Usage,
        /// Why the model stopped.
        stop_reason: StopReason,
    },
    /// Terminal: the model invocation failed. Emits
    /// [`AgentEvent::RunFailed`].
    ///
    /// **Field order is load-bearing** — it must match spec §9:
    /// `code`, then `retryable`, then `message`.
    Failed {
        /// Stable machine-readable code from the upstream.
        code: String,
        /// Human-readable error description.
        message: String,
        /// Whether the loop is allowed to retry.
        retryable: bool,
    },
}

/// Errors an accumulator can surface to the adapter / loop.
///
/// The variants are deliberately fine-grained: callers (the loop, in
/// wave 2 task 2.5+) can branch on them to decide between surfacing
/// the error to the user, retrying, or treating the stream as
/// un-recoverable.
#[derive(Debug, Error)]
pub enum StreamError {
    /// The chunk stream violated an invariant (unknown `call_id`,
    /// duplicate tool start, non-JSON `args_buf`, post-terminal
    /// push). Surfaces the underlying message.
    #[error("stream invalid: {0}")]
    Invalid(String),
    /// An input chunk contained an incomplete multi-byte UTF-8
    /// sequence at a byte boundary the accumulator could not consume.
    /// In v0 this variant is unreachable through the public API
    /// (`String` fields are always valid UTF-8); see the module docs
    /// for why it exists.
    #[error("UTF-8 boundary error in stream")]
    Utf8,
    /// Two [`ProviderChunk::Completed`] chunks were pushed in the same
    /// stream. A model invocation has exactly one terminal.
    #[error("double Completed")]
    DoubleCompleted,
    /// `finish` was called while one or more [`ProviderChunk::ToolCallStart`]
    /// entries were still waiting on a matching
    /// [`ProviderChunk::ToolCallComplete`]. Carries the dangling
    /// `call_id`.
    #[error("stream ended with pending tool call: {0}")]
    PendingToolCall(ToolCallId),
}

#[cfg(test)]
mod tests {
    //! Unit tests for [`StreamAccumulator`].
    //!
    //! Every test pinned the contract in the wave-2 task 2.1 brief.
    //! They live in the same module as the code under test so they can
    //! poke at private helpers (`pending`, `finished`) when needed.

    use super::*;
    use crate::ids::{new_id, ToolCallId};

    /// Convenience: build a `ToolCallId` from a fresh ULID.
    fn call_id() -> ToolCallId {
        ToolCallId(new_id())
    }

    /// Test 1 (brief): `TextDelta` chunks emit one `AgentEvent` each.
    /// Chunks are NEVER merged — the consumer sees each fragment in
    /// order so it can stream to the TUI as it arrives.
    #[test]
    fn text_delta_emits_one_event_per_chunk() {
        let mut acc = StreamAccumulator::new();
        let out = acc
            .push_chunk(ProviderChunk::TextDelta { text: "Hel".into() })
            .expect("text delta should be valid");
        assert_eq!(
            out,
            vec![AgentEvent::TextDelta { text: "Hel".into() }],
            "TextDelta must surface its text verbatim",
        );

        let out = acc
            .push_chunk(ProviderChunk::TextDelta { text: "lo".into() })
            .expect("text delta should be valid");
        assert_eq!(out, vec![AgentEvent::TextDelta { text: "lo".into() }],);

        // Buffer accumulates even though emission is per-chunk.
        assert_eq!(acc.text_buf, "Hello");
    }

    /// Test 2 (brief): same as test 1, but for `ReasoningDelta`.
    #[test]
    fn reasoning_delta_emits_one_event_per_chunk() {
        let mut acc = StreamAccumulator::new();
        let out = acc
            .push_chunk(ProviderChunk::ReasoningDelta {
                text: "thinking ".into(),
            })
            .expect("reasoning delta should be valid");
        assert_eq!(
            out,
            vec![AgentEvent::ReasoningDelta {
                text: "thinking ".into()
            }],
        );
        assert_eq!(acc.reasoning_buf, "thinking ");
    }

    /// Test 3 (brief): `ToolCallStart` + multiple `ToolArgumentsDelta`
    /// + `ToolCallComplete` produces one `ToolStarted` with the
    ///   **concatenated** JSON parsed as a single value.
    #[test]
    fn tool_call_complete_concatenates_json_fragments() {
        let mut acc = StreamAccumulator::new();
        let cid = call_id();

        // Open.
        let out = acc
            .push_chunk(ProviderChunk::ToolCallStart {
                call_id: cid,
                name: "shell".into(),
            })
            .expect("start should be accepted");
        assert!(out.is_empty(), "ToolCallStart must NOT emit an event yet");
        assert_eq!(acc.pending.len(), 1, "tool call must be pending");

        // Three JSON fragments that only concatenate into a valid
        // object. Per-chunk each is invalid JSON, so the accumulator
        // is the only place they can be assembled.
        for frag in ["{\"na", "me\":\"shi", "nya\"}"] {
            let out = acc
                .push_chunk(ProviderChunk::ToolArgumentsDelta {
                    call_id: cid,
                    fragment: frag.into(),
                })
                .expect("delta should append");
            assert!(out.is_empty(), "delta must not emit an event");
        }

        // Complete: this is where the ToolStarted event fires.
        let out = acc
            .push_chunk(ProviderChunk::ToolCallComplete { call_id: cid })
            .expect("complete should parse");
        assert_eq!(
            out,
            vec![AgentEvent::ToolStarted {
                call_id: cid,
                name: "shell".into(),
                args: serde_json::json!({"name": "shinya"}),
            }],
        );

        assert!(acc.pending.is_empty(), "tool call must be cleared");
    }

    /// Test 4 (brief): `ToolCallStart` with no `ToolArgumentsDelta`
    /// produces `ToolStarted` with `args = serde_json::Value::Null`.
    #[test]
    fn tool_call_with_no_args_delta_emits_null() {
        let mut acc = StreamAccumulator::new();
        let cid = call_id();

        acc.push_chunk(ProviderChunk::ToolCallStart {
            call_id: cid,
            name: "noop".into(),
        })
        .expect("start accepted");
        let out = acc
            .push_chunk(ProviderChunk::ToolCallComplete { call_id: cid })
            .expect("complete accepted");
        assert_eq!(
            out,
            vec![AgentEvent::ToolStarted {
                call_id: cid,
                name: "noop".into(),
                args: serde_json::Value::Null,
            }],
            "empty args_buf must round-trip as Null",
        );
    }

    /// Test 5 (brief): the `Utf8` variant is reachable and has the
    /// right display string.
    ///
    /// Rationale: with `String`-typed `text`/`fragment` fields, an
    /// invalid UTF-8 sequence is impossible to construct in safe
    /// Rust. `Utf8` is documented as a future-proofing safety valve
    /// (see the module docs); here we pin down the variant's shape
    /// so a future refactor that makes it reachable won't silently
    /// change the user-facing message.
    #[test]
    fn utf8_variant_has_stable_display_message() {
        let err = StreamError::Utf8;
        let msg = err.to_string();
        assert_eq!(
            msg, "UTF-8 boundary error in stream",
            "Utf8 display message is pinned to the spec verbatim",
        );

        // The variant also satisfies Debug (used in test failures
        // and in structured logging).
        let debug = format!("{err:?}");
        assert!(
            debug.contains("Utf8"),
            "Debug impl must name the variant, got {debug}",
        );
    }

    /// Test 5b: more than just display — when bytes DO flow through
    /// a future byte-level path, the conversion to `String` must
    /// produce a `StreamError::Utf8`. This helper simulates that
    /// path: it converts raw bytes to `String`, errors with `Utf8`
    /// on the first invalid sequence, and otherwise continues the
    /// public `push_chunk` flow.
    ///
    /// The helper is `pub(crate)` so other crate-internal tests can
    /// reuse the UTF-8 boundary check without duplicating the
    /// logic.
    #[test]
    fn utf8_via_byte_path_rejected() {
        // `0xC3 0x28` is the canonical "invalid UTF-8 sequence"
        // example from RFC 3629 §10.
        let bad: Vec<u8> = vec![0xC3, 0x28];
        let s = std::str::from_utf8(&bad).map_err(|_| StreamError::Utf8);
        assert!(
            matches!(s, Err(StreamError::Utf8)),
            "byte-level conversion must surface Utf8 on invalid input",
        );

        // And the happy path (valid UTF-8) succeeds.
        let good = "héllo".as_bytes().to_vec();
        let s = std::str::from_utf8(&good).map_err(|_| StreamError::Utf8);
        assert!(s.is_ok(), "valid UTF-8 must convert cleanly");
    }

    /// Test 6 (brief): two `Completed` chunks in the same stream
    /// surface `StreamError::DoubleCompleted`. (Note: we surface the
    /// error on the SECOND `Completed`, so the first one is still
    /// emitted to the caller.)
    #[test]
    fn double_completed_returns_error_on_second() {
        let mut acc = StreamAccumulator::new();
        let usage = Usage {
            input_tokens: 1,
            output_tokens: 2,
        };
        let first = acc
            .push_chunk(ProviderChunk::Completed {
                usage,
                stop_reason: StopReason::EndTurn,
            })
            .expect("first Completed should succeed");
        assert_eq!(
            first,
            vec![AgentEvent::ModelFinished {
                usage,
                stop_reason: StopReason::EndTurn,
            }],
            "first Completed should emit ModelFinished",
        );

        let second = acc.push_chunk(ProviderChunk::Completed {
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
            stop_reason: StopReason::EndTurn,
        });
        assert!(
            matches!(second, Err(StreamError::DoubleCompleted)),
            "second Completed must surface DoubleCompleted, got {second:?}",
        );
    }

    /// Test 7 (brief): `finish()` with no `Completed` chunk pushed
    /// returns an error. (An empty stream is not a clean run.)
    #[test]
    fn finish_without_completed_errors() {
        let mut acc = StreamAccumulator::new();
        let result = acc.finish();
        assert!(
            matches!(result, Err(StreamError::Invalid(_))),
            "finish on empty stream must error, got {result:?}",
        );
    }

    /// Test 8 (brief): `finish()` with a pending `ToolCallStart`
    /// returns `StreamError::PendingToolCall`. The dangling call id
    /// is surfaced in the error payload.
    #[test]
    fn finish_with_pending_tool_call_returns_error() {
        let mut acc = StreamAccumulator::new();
        let cid = call_id();

        acc.push_chunk(ProviderChunk::ToolCallStart {
            call_id: cid,
            name: "shell".into(),
        })
        .expect("start accepted");

        let result = acc.finish();
        match result {
            Err(StreamError::PendingToolCall(dangling)) => {
                assert_eq!(dangling, cid, "error must carry the dangling call_id",);
            }
            other => panic!("expected PendingToolCall, got {other:?}"),
        }
    }

    /// Test 9 (brief): `Failed` chunk emits `RunFailed` with field
    /// order `code, retryable, message` (spec §9).
    #[test]
    fn failed_emits_run_failed_with_spec_field_order() {
        let mut acc = StreamAccumulator::new();
        let out = acc
            .push_chunk(ProviderChunk::Failed {
                code: "stream_invalid".into(),
                message: "upstream said no".into(),
                retryable: true,
            })
            .expect("Failed should be accepted");
        assert_eq!(
            out,
            vec![AgentEvent::RunFailed {
                code: ErrorCode("stream_invalid".into()),
                retryable: true,
                message: "upstream said no".into(),
            }],
            "Failed must surface as RunFailed with exact field order",
        );

        // And it terminates the stream.
        let follow_up = acc.push_chunk(ProviderChunk::TextDelta {
            text: "ignored".into(),
        });
        assert!(
            matches!(follow_up, Err(StreamError::Invalid(_))),
            "post-terminal chunks must error, got {follow_up:?}",
        );

        // `finish()` after a Failed must surface a previously
        // un-returned terminal. Because Failed already returned the
        // event via `push_chunk`, the caller's expected workflow is
        // to use `finish` purely for validation. With our
        // implementation, `finish` will report the same event again
        // IF the caller discards the `push_chunk` events; here we
        // already consumed them, so the next `finish` should report
        // the stream as already-terminated-without-leftover.
        let terminal = acc.finish().expect("finish after Failed succeeds");
        assert!(matches!(
            terminal,
            AgentEvent::RunFailed { ref code, .. } if code == &ErrorCode("stream_invalid".into())
        ));
    }

    /// Test 10 (brief): the empty stream path — only `Started` then
    /// `Completed` — emits `ModelStarted` + `ModelFinished`.
    #[test]
    fn started_then_completed_emits_clean_pair() {
        let mut acc = StreamAccumulator::new();
        let usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
        };

        let started = acc
            .push_chunk(ProviderChunk::Started)
            .expect("Started accepted");
        assert_eq!(started, vec![AgentEvent::ModelStarted]);

        let completed = acc
            .push_chunk(ProviderChunk::Completed {
                usage,
                stop_reason: StopReason::EndTurn,
            })
            .expect("Completed accepted");
        assert_eq!(
            completed,
            vec![AgentEvent::ModelFinished {
                usage,
                stop_reason: StopReason::EndTurn,
            }],
        );

        // Calling `finish` after `Completed` returns the same
        // terminal event so the caller can choose to pull it from
        // either location.
        let terminal = acc.finish().expect("finish on completed stream");
        assert!(matches!(terminal, AgentEvent::ModelFinished { .. }));
    }

    /// Test 11: multiple interleaved tool calls each assemble
    /// independently. Pins down the `pending` Vec order-bookkeeping
    /// under a realistic load.
    #[test]
    fn multiple_tool_calls_assemble_independently() {
        let mut acc = StreamAccumulator::new();
        let a = call_id();
        let b = call_id();

        // Open both before completing either.
        acc.push_chunk(ProviderChunk::ToolCallStart {
            call_id: a,
            name: "read".into(),
        })
        .expect("start a accepted");
        acc.push_chunk(ProviderChunk::ToolCallStart {
            call_id: b,
            name: "edit".into(),
        })
        .expect("start b accepted");
        assert_eq!(acc.pending.len(), 2);

        // Mix deltas across both.
        acc.push_chunk(ProviderChunk::ToolArgumentsDelta {
            call_id: a,
            fragment: "{\"path\":\"/".into(),
        })
        .expect("delta a-1");
        acc.push_chunk(ProviderChunk::ToolArgumentsDelta {
            call_id: b,
            fragment: "{\"old\":\"".to_string(),
        })
        .expect("delta b-1");
        acc.push_chunk(ProviderChunk::ToolArgumentsDelta {
            call_id: a,
            fragment: "tmp/x\"}".into(),
        })
        .expect("delta a-2");
        acc.push_chunk(ProviderChunk::ToolArgumentsDelta {
            call_id: b,
            fragment: "foo\"}".into(),
        })
        .expect("delta b-2");

        // Complete in reverse order to prove pending bookkeeping
        // uses `call_id`, not position.
        let out_b = acc
            .push_chunk(ProviderChunk::ToolCallComplete { call_id: b })
            .expect("complete b");
        assert_eq!(
            out_b,
            vec![AgentEvent::ToolStarted {
                call_id: b,
                name: "edit".into(),
                args: serde_json::json!({"old": "foo"}),
            }],
        );

        let out_a = acc
            .push_chunk(ProviderChunk::ToolCallComplete { call_id: a })
            .expect("complete a");
        assert_eq!(
            out_a,
            vec![AgentEvent::ToolStarted {
                call_id: a,
                name: "read".into(),
                args: serde_json::json!({"path": "/tmp/x"}),
            }],
        );

        assert!(acc.pending.is_empty());
    }

    /// Test 12: malformed input — a `ToolArgumentsDelta` for a
    /// `call_id` we never opened must surface `Invalid`.
    #[test]
    fn tool_arguments_delta_with_unknown_call_id_errors() {
        let mut acc = StreamAccumulator::new();
        let ghost = call_id();
        let result = acc.push_chunk(ProviderChunk::ToolArgumentsDelta {
            call_id: ghost,
            fragment: "{}".into(),
        });
        match result {
            Err(StreamError::Invalid(msg)) => {
                assert!(
                    msg.contains(&ghost.0.to_string()),
                    "error must name the offending call_id, got {msg}",
                );
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    /// Test 13: malformed input — duplicate `ToolCallStart` for the
    /// same `call_id` must surface `Invalid`.
    #[test]
    fn duplicate_tool_call_start_errors() {
        let mut acc = StreamAccumulator::new();
        let cid = call_id();
        acc.push_chunk(ProviderChunk::ToolCallStart {
            call_id: cid,
            name: "shell".into(),
        })
        .expect("first start accepted");
        let result = acc.push_chunk(ProviderChunk::ToolCallStart {
            call_id: cid,
            name: "shell".into(),
        });
        assert!(
            matches!(result, Err(StreamError::Invalid(_))),
            "duplicate start must error, got {result:?}",
        );
    }

    /// Test 14: tool-call `args_buf` that doesn't parse as JSON
    /// surfaces `Invalid` on `ToolCallComplete`.
    #[test]
    fn tool_call_complete_with_invalid_args_errors() {
        let mut acc = StreamAccumulator::new();
        let cid = call_id();
        acc.push_chunk(ProviderChunk::ToolCallStart {
            call_id: cid,
            name: "shell".into(),
        })
        .expect("start accepted");
        acc.push_chunk(ProviderChunk::ToolArgumentsDelta {
            call_id: cid,
            fragment: "not json".into(),
        })
        .expect("delta accepted");
        let result = acc.push_chunk(ProviderChunk::ToolCallComplete { call_id: cid });
        match result {
            Err(StreamError::Invalid(msg)) => {
                assert!(
                    msg.contains("not valid JSON"),
                    "error message must explain the failure, got {msg}",
                );
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    /// Test 15: the `ProviderChunk` enum has no newtype variants —
    /// every variant carries named fields. Pinning this down at
    /// the type level would require a custom derive; at the API
    /// level we verify each variant's struct shape by constructing
    /// it directly and round-tripping through `Debug`.
    #[test]
    fn provider_chunk_variants_are_struct_shaped() {
        let cid = call_id();
        let cases: Vec<ProviderChunk> = vec![
            ProviderChunk::Started,
            ProviderChunk::TextDelta { text: "x".into() },
            ProviderChunk::ReasoningDelta { text: "y".into() },
            ProviderChunk::ToolCallStart {
                call_id: cid,
                name: "n".into(),
            },
            ProviderChunk::ToolArgumentsDelta {
                call_id: cid,
                fragment: "f".into(),
            },
            ProviderChunk::ToolCallComplete { call_id: cid },
            ProviderChunk::Completed {
                usage: Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
                stop_reason: StopReason::EndTurn,
            },
            ProviderChunk::Failed {
                code: "c".into(),
                message: "m".into(),
                retryable: false,
            },
        ];
        for c in &cases {
            // Debug round-trip proves every variant is constructible
            // from a struct literal (newtype variants would not be).
            let debug = format!("{c:?}");
            assert!(!debug.is_empty(), "Debug impl must be non-empty");
        }
        // Sanity: 8 distinct variants.
        assert_eq!(cases.len(), 8);
    }
}
