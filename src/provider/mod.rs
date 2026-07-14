//! Provider boundary: the trait the agent loop calls to get streaming
//! model output.
//!
//! This module defines the **shape** of a provider. Real adapters (the
//! `genai` adapter in wave 2) and the test-only [`ScriptedProvider`] in
//! [`crate::provider::mock`] both implement it.
//!
//! The agent loop only ever sees [`ProviderStream`]; raw upstream events
//! MUST NOT leak past the adapter — that invariant is the whole point of
//! the trait (spec §9).
//!
//! # Cancellation
//!
//! Every call to [`Provider::stream`] takes a [`CancellationToken`]. The
//! adapter is responsible for observing it: at minimum, returning
//! [`ProviderError::Cancelled`] if the token is already triggered, and
//! terminating the stream promptly if it fires mid-flight.

pub mod mock;

use async_trait::async_trait;
use serde::Serialize;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::event::AgentEvent;
use crate::message::Message;

/// What the loop sends to a model provider for one invocation.
///
/// `messages` is the full conversation history in order, including any
/// tool results from prior turns. `tools_schema` is a JSON Schema (or
/// whatever the provider needs) describing the tools the model is
/// allowed to call.
#[derive(Debug, Clone, Serialize)]
pub struct ModelRequest {
    /// Conversation history, oldest first.
    pub messages: Vec<Message>,
    /// Provider-specific description of the tool surface available to the
    /// model. We pass it through as opaque JSON so the agent loop can
    /// stay provider-neutral.
    pub tools_schema: serde_json::Value,
}

/// Errors a provider can surface to the agent loop.
#[derive(Error, Debug)]
pub enum ProviderError {
    /// The provider returned malformed or contradictory data (bad
    /// JSONL, missing fields, an unknown event type, etc.).
    #[error("stream invalid: {0}")]
    StreamInvalid(String),
    /// The upstream service returned an error.
    ///
    /// `retryable` lets the loop decide whether to back off and retry
    /// the same request, or surface the error to the user.
    #[error("upstream error: {code} {message}")]
    Upstream {
        /// Stable, machine-readable error code from the upstream.
        code: String,
        /// Human-readable description of the error.
        message: String,
        /// Whether the loop should retry the request.
        retryable: bool,
    },
    /// The cancellation token fired before or during the stream.
    #[error("cancelled")]
    Cancelled,
}

/// A stream of model events.
///
/// The agent loop pulls [`AgentEvent`]s out of this until it sees a
/// terminal variant (`ModelFinished`, `RunFailed`, `RunCancelled`, or
/// `RunFinished`).
///
/// `events` is `Pin<Box<dyn Stream>>` so the trait object stays
/// object-safe regardless of the adapter's internal state.
///
/// **Note on the type path:** the brief writes
/// `tokio::sync::Stream`, which is a typo — the `Stream` trait lives
/// in the `tokio_stream` crate (re-export of `futures::Stream`). The
/// `tokio` crate does not expose a `Stream` in its `sync` module.
pub struct ProviderStream {
    /// The boxed stream.
    pub events: std::pin::Pin<
        Box<dyn tokio_stream::Stream<Item = Result<AgentEvent, ProviderError>> + Send>,
    >,
}

// `Pin<Box<dyn Stream>>` does not implement `Debug` (the trait object
// is opaque), so we hand-roll a placeholder. The contents of the
// stream are visible to the consumer via `poll_next`; we just need a
// name for the `Debug` printout.
impl std::fmt::Debug for ProviderStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderStream").finish_non_exhaustive()
    }
}

/// The contract every model provider must satisfy.
///
/// Implementors are object-safe (no generic methods, no `Self` in
/// arguments) so the loop can hold `Box<dyn Provider>` if it needs to
/// pick the adapter at runtime.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Open a streaming model invocation.
    ///
    /// The returned [`ProviderStream`] is fully owned by the caller; the
    /// adapter must not retain references to `req` or `cancel` after
    /// this method returns.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Cancelled`] if `cancel` is already
    /// triggered at call time. Other pre-flight failures (e.g. invalid
    /// request) are reported as the appropriate variant; the stream
    /// itself produces the per-event errors.
    async fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError>;
}
