//! Cancellation primitives for the Crow v0 kernel.
//!
//! This module re-exports [`tokio_util::sync::CancellationToken`] and
//! provides the [`timeout_or_cancel`] helper used throughout the
//! provider, loop, and tool subsystems.
//!
//! # Cancellation hierarchy
//!
//! Cancellation in Crow flows strictly downward:
//!
//! ```text
//! process -> session -> run -> provider/tool
//! ```
//!
//! Each level holds a [`CancellationToken`]; cancelling a parent
//! propagates to its descendants via [`CancellationToken::child_token`].
//! A child that cancels itself does **not** cancel its parent —
//! cancellation only travels down the tree.
//!
//! # Drop semantics (IMPORTANT)
//!
//! Dropping a [`CancellationToken`] does **not** cancel any children
//! it has produced via [`CancellationToken::child_token`]. This is a
//! deliberate [`tokio_util`] design choice. If you want a parent drop
//! to cancel its children, you must call [`CancellationToken::cancel`]
//! (or [`CancellationToken::cancel_owned`]) explicitly before dropping.
//! This is intentional: it lets a long-lived parent token outlive a
//! short-lived child without prematurely tearing down siblings.
//!
//! Code that relies on a guard pattern ("the moment this token goes
//! out of scope, the work stops") should wrap it in a small RAII
//! helper that calls `cancel()` in `Drop` — Crow does not provide one
//! in v0 to keep the cancellation surface explicit.

pub use tokio_util::sync::CancellationToken;

use std::future::Future;
use std::time::Duration;
use thiserror::Error;

/// Outcome of [`timeout_or_cancel`] when the future did not complete.
#[allow(clippy::module_name_repetitions)] // name is dictated by the brief.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum CancelOutcome {
    /// The cancellation token fired before the future completed or
    /// the timeout elapsed.
    #[error("cancelled")]
    Cancelled,
    /// The timeout elapsed before the future completed and before
    /// the cancellation token fired.
    #[error("timed out")]
    TimedOut,
}

/// Run `fut` to completion, or surface whichever happens first:
/// - the future resolves -> `Ok(value)`,
/// - the cancellation token fires -> `Err(CancelOutcome::Cancelled)`,
/// - the timeout elapses -> `Err(CancelOutcome::TimedOut)`.
///
/// Implementation: a [`tokio::select!`] biased toward the cancel
/// branch so that, when the future resolves and the token fires at
/// the same instant, cancellation wins. On either terminal outcome
/// the future is dropped (and not polled again), so callers must
/// structure their work to be cancellation-safe (no leaked locks,
/// no half-written files, etc.).
///
/// The timeout branch is implemented with [`tokio::time::sleep`], not
/// [`tokio::time::timeout`], so that the cancel branch can win ties
/// deterministically without spinning.
///
/// # Errors
///
/// Returns [`CancelOutcome::Cancelled`] when `token` fires before the
/// future resolves and before `timeout` elapses, or
/// [`CancelOutcome::TimedOut`] when `timeout` elapses first. The
/// original future is dropped in either case.
#[allow(clippy::module_name_repetitions)] // name is dictated by the brief.
pub async fn timeout_or_cancel<T>(
    token: CancellationToken,
    timeout: Duration,
    fut: impl Future<Output = T>,
) -> Result<T, CancelOutcome> {
    tokio::select! {
        biased;
        () = token.cancelled() => Err(CancelOutcome::Cancelled),
        () = tokio::time::sleep(timeout) => Err(CancelOutcome::TimedOut),
        result = fut => Ok(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// Token cancels a still-running future -> `Cancelled`.
    #[tokio::test]
    async fn token_cancel_before_timeout_returns_cancelled() {
        let token = CancellationToken::new();
        let handle = token.clone();
        tokio::spawn(async move {
            // Give the main task time to enter `select!`, then cancel.
            tokio::time::sleep(Duration::from_millis(20)).await;
            handle.cancel();
        });

        let started = Instant::now();
        let result: Result<(), CancelOutcome> =
            timeout_or_cancel(token, Duration::from_secs(5), async {
                // Sleep well past the cancel delay so the future is
                // still pending when the cancel arrives.
                tokio::time::sleep(Duration::from_secs(1)).await;
            })
            .await;

        assert_eq!(result, Err(CancelOutcome::Cancelled));
        // Must return promptly after the cancel, not after the timeout.
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "timeout_or_cancel did not return promptly after cancel",
        );
    }

    /// Timeout fires first on a slow future -> `TimedOut`.
    #[tokio::test]
    async fn timeout_fires_first_returns_timed_out() {
        let token = CancellationToken::new();

        let started = Instant::now();
        let result: Result<(), CancelOutcome> =
            timeout_or_cancel(token, Duration::from_millis(50), async {
                tokio::time::sleep(Duration::from_secs(5)).await;
            })
            .await;

        let elapsed = started.elapsed();
        assert_eq!(result, Err(CancelOutcome::TimedOut));
        // Spec: return within ~50ms of the timeout.
        assert!(
            elapsed >= Duration::from_millis(40),
            "returned too early: {elapsed:?}",
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "returned too late: {elapsed:?}",
        );
    }

    /// Future completes before either trigger -> `Ok(value)`.
    #[tokio::test]
    async fn future_completes_first_returns_ok() {
        let token = CancellationToken::new();

        let result: Result<u32, CancelOutcome> =
            timeout_or_cancel(token, Duration::from_secs(5), async { 42 }).await;

        assert_eq!(result, Ok(42));
    }

    /// Both future and cancel resolve at the same instant: cancel wins
    /// (biased select). We deterministically force this by arming the
    /// token before the call so `token.cancelled()` is ready on the
    /// very first poll, and the future also resolves immediately.
    #[tokio::test]
    async fn cancel_wins_ties_against_completed_future() {
        let token = CancellationToken::new();
        token.cancel();

        let result: Result<u32, CancelOutcome> =
            timeout_or_cancel(token, Duration::from_secs(5), async { 7 }).await;

        assert_eq!(result, Err(CancelOutcome::Cancelled));
    }

    /// Cancelling a parent token cancels its child token (via
    /// `child_token`). This is the documented downward-only flow.
    #[tokio::test]
    async fn parent_cancel_propagates_to_child_token() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        assert!(!child.is_cancelled());

        parent.cancel();

        assert!(parent.is_cancelled());
        assert!(
            child.is_cancelled(),
            "child token must observe parent cancellation",
        );
    }

    /// Cancelling a child token must NOT cancel its parent. This is
    /// the "cancellation only flows downward" invariant from the
    /// module docs.
    #[tokio::test]
    async fn child_cancel_does_not_cancel_parent() {
        let parent = CancellationToken::new();
        let child = parent.child_token();

        child.cancel();

        assert!(child.is_cancelled());
        assert!(
            !parent.is_cancelled(),
            "parent token must NOT observe child cancellation",
        );
    }

    /// Calling `cancel()` twice on the same token is a no-op the
    /// second time — cancellation is a sticky, idempotent event.
    #[tokio::test]
    async fn double_cancel_is_idempotent() {
        let token = CancellationToken::new();
        token.cancel();
        let first = token.is_cancelled();

        // A second cancel call must not panic and must not flip the
        // state in any observable way.
        token.cancel();
        let second = token.is_cancelled();

        assert!(first);
        assert!(second);

        // A future awaiting cancellation still resolves after the
        // second call.
        let observed = token.cancelled();
        token.cancel(); // third call for good measure
                        // `cancelled()` returns a future; if already cancelled it
                        // resolves immediately, so we just race against a tiny sleep.
        tokio::select! {
            () = observed => {}
            () = tokio::time::sleep(Duration::from_millis(50)) => {
                panic!("cancelled() future did not resolve after double cancel");
            }
        }
    }

    /// **Important behavioural note.** Dropping a parent token does
    /// NOT cancel its children in `tokio_util`. This test pins that
    /// behaviour down so a future `tokio_util` upgrade that changed
    /// it would surface as a test failure (forcing us to revisit the
    /// docs and any code that relied on the current semantics).
    #[tokio::test]
    async fn dropping_parent_does_not_cancel_child() {
        let parent = CancellationToken::new();
        let child = parent.child_token();

        // Drop the parent without calling `cancel()`.
        drop(parent);

        assert!(
            !child.is_cancelled(),
            "child token must not be cancelled by parent drop",
        );
    }

    /// Bonus coverage: a deeply nested tree cancels atomically.
    /// process -> session -> run; cancelling the root cancels every
    /// descendant, but cancelling a leaf leaves the root and other
    /// branches live.
    #[tokio::test]
    async fn deeply_nested_tree_cancellation() {
        let process = CancellationToken::new();
        let session = process.child_token();
        let run = session.child_token();
        let provider = run.child_token();
        let tool = provider.child_token();

        // Leaf cancel stays local.
        tool.cancel();
        assert!(tool.is_cancelled());
        assert!(!provider.is_cancelled());
        assert!(!run.is_cancelled());
        assert!(!session.is_cancelled());
        assert!(!process.is_cancelled());

        // Root cancel cascades down to every surviving descendant.
        process.cancel();
        assert!(process.is_cancelled());
        assert!(session.is_cancelled());
        assert!(run.is_cancelled());
        assert!(provider.is_cancelled());
        assert!(tool.is_cancelled()); // still cancelled, double-cancel is fine
    }

    /// Bonus coverage: a work future that records a side-effect on
    /// drop is **not** polled to completion when we return
    /// `TimedOut` — confirming the future is dropped, not polled
    /// again. This is the "cancellation-safety" contract from the
    /// `timeout_or_cancel` doc comment.
    #[tokio::test]
    async fn future_is_dropped_on_timeout() {
        struct DropGuard(Arc<AtomicBool>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_inner = dropped.clone();

        let result: Result<(), CancelOutcome> = timeout_or_cancel(
            CancellationToken::new(),
            Duration::from_millis(20),
            async move {
                let _guard = DropGuard(dropped_inner);
                tokio::time::sleep(Duration::from_secs(5)).await;
                // Unreachable: we expect to be dropped on timeout.
            },
        )
        .await;

        assert_eq!(result, Err(CancelOutcome::TimedOut));
        assert!(
            dropped.load(Ordering::SeqCst),
            "future must be dropped when timeout fires",
        );
    }
}
