### Task 1.5 — Cancellation helper

**Files:**
- Create: `src/cancel.rs`
- Modify: `src/lib.rs` (re-export)

**Spec references:** v0 spec §4 (cancellation), §9 (Provider trait takes `CancellationToken`), §11 (cancellation in loop).

**CRITICAL spec alignment:** the v0 spec §9 `Provider` trait takes `tokio_util::sync::CancellationToken` directly. We do NOT invent a wrapper. This task adds two things:
1. Re-exports `tokio_util::sync::CancellationToken` and `tokio_util::sync::CancellationTokenTree` for the rest of the crate.
2. Provides a `timeout_or_cancel(future, token, timeout)` helper.

**Cancellation hierarchy convention** (documented, not implemented in code):
- Process → session → run → provider/tool. Each level holds a `CancellationToken`; cancelling the parent cancels all children. This is `tokio_util`'s built-in `child_token()` behaviour.
- A child that wants to cancel itself does NOT cancel its parent. Cancellation only flows downward.

**Interfaces (exact):**

```rust
// cancel.rs
pub use tokio_util::sync::CancellationToken;

use std::time::Duration;

/// Run `future` to completion, or return whichever happens first:
/// - future completes -> Ok(T)
/// - cancellation token fires -> Err(CancelOutcome::Cancelled)
/// - timeout elapses -> Err(CancelOutcome::TimedOut)
///
/// Implementation: `tokio::select!` between the future and
/// `tokio::time::sleep(timeout)`, with the cancel branch selected
/// when the token fires. On timeout, the future is dropped (NOT polled
/// again) — callers must structure work to be cancellation-safe.
pub async fn timeout_or_cancel<T>(
    token: CancellationToken,
    timeout: Duration,
    fut: impl std::future::Future<Output = T>,
) -> Result<T, CancelOutcome>;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CancelOutcome {
    #[error("cancelled")] Cancelled,
    #[error("timed out")] TimedOut,
}
```

**Acceptance:**
- `CancellationToken` is re-exported; it's the only cancellation primitive in the public API
- `timeout_or_cancel` returns within 50ms of the timeout on a healthy future
- Tests (8+):
  - token cancel before timeout: returns `Cancelled`
  - timeout fires first: returns `TimedOut`
  - future completes before either: returns `Ok`
  - future completes before either but cancel is also fired: returns `Cancelled` (cancel wins ties)
  - nested `child_token()` cancels with parent
  - cancelling a child does NOT cancel its parent
  - double cancel is idempotent (calling cancel twice == calling once)
  - **The "drop token cancels child" test is INVALID — `tokio_util`'s `CancellationToken` does NOT cancel children on drop. Test that instead:** if the parent is dropped, its `cancel()` has not been called explicitly, children do not see a cancellation signal. This is a deliberate tokio_util design choice. Document it in the module docs.
- All public functions have `///` doc comments

**Forbidden:** No `unsafe`. No global state. No `tokio::spawn` from inside `cancel.rs`. No re-implementing `CancellationToken`. No claiming "drop cancels children" — that is false.
