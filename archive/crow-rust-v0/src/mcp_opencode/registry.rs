//! In-flight task registry.
//!
//! Each `opencode_delegate*` call records a [`TaskHandle`] in this
//! map, keyed by its [`TaskId`]. The handle carries the cancellation
//! token (so `opencode_cancel` can reach it). The tool handler that
//! submits the task owns the `oneshot` receiver for the run's result;
//! once it has the result it calls [`TaskRegistry::remove`] to evict
//! the entry.
//!
//! Keeping the result channel out of the registry avoids a "registry
//! always holds the result" leak if a caller is dropped before
//! awaiting; the result is dropped alongside the receiver and the
//! handle on the sender side, exactly like any other oneshot pattern.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Unique id for one delegated opencode run. ULID-backed for
/// sortability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub ulid::Ulid);

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TaskId {
    #[must_use]
    pub fn new() -> Self {
        Self(ulid::Ulid::new())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-task state held by the registry. The result receiver lives with
/// the caller (the tool handler), not here.
#[derive(Debug)]
pub struct TaskHandle {
    /// Cancels the underlying opencode run (process group SIGKILL).
    pub cancel: CancellationToken,
    /// What the caller submitted. Kept so `opencode_status` can echo it
    /// back without round-tripping through the caller.
    pub request: crate::mcp_opencode::runner::RunRequest,
    /// Wall-clock instant the task was submitted. Used to report
    /// "running for N seconds" on status calls.
    pub submitted_at: std::time::Instant,
}

/// Snapshot of a task's state, safe to return over the MCP boundary.
#[derive(Debug, Clone, Serialize)]
pub struct TaskStatus {
    pub id: TaskId,
    pub running: bool,
    pub cancelled: bool,
    pub prompt: String,
    pub workdir: String,
    pub elapsed_ms: u64,
}

/// Concurrent map of in-flight tasks. New tasks are registered on
/// submission and removed when the tool handler finishes awaiting
/// the run.
///
/// Cheap to clone: the inner map lives behind an [`Arc`] so a clone
/// shares the same state. The protocol loop clones the registry on
/// every tool dispatch so handlers can hold it across `.await`s
/// without borrowing from the loop.
#[derive(Clone, Default, Debug)]
pub struct TaskRegistry {
    inner: Arc<Mutex<HashMap<TaskId, TaskHandle>>>,
}

impl TaskRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a fresh task. Returns the id and a clone of the
    /// cancellation token so the caller can hand the token to its
    /// runner task.
    pub async fn register(
        &self,
        request: crate::mcp_opencode::runner::RunRequest,
    ) -> (TaskId, CancellationToken) {
        let id = TaskId::new();
        let cancel = CancellationToken::new();
        let handle = TaskHandle {
            cancel: cancel.clone(),
            submitted_at: std::time::Instant::now(),
            request,
        };
        self.inner.lock().await.insert(id, handle);
        (id, cancel)
    }

    /// Look up a task by id and return its public status. Returns
    /// `None` if no such task is registered.
    pub async fn get(&self, id: TaskId) -> Option<TaskStatus> {
        let g = self.inner.lock().await;
        g.get(&id).map(|h| TaskStatus {
            id,
            running: !h.cancel.is_cancelled(),
            cancelled: h.cancel.is_cancelled(),
            prompt: h.request.prompt.clone(),
            workdir: h.request.workdir.display().to_string(),
            elapsed_ms: h.submitted_at.elapsed().as_millis() as u64,
        })
    }

    /// Cancel a task by id. Returns `true` if a task was found and
    /// cancelled, `false` if no such task is registered.
    pub async fn cancel(&self, id: TaskId) -> bool {
        let g = self.inner.lock().await;
        match g.get(&id) {
            Some(h) => {
                h.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Remove a task from the registry. Idempotent: removing a
    /// non-existent task is a no-op. Called by the tool handler
    /// once it has finished awaiting the run.
    pub async fn remove(&self, id: TaskId) {
        self.inner.lock().await.remove(&id);
    }

    /// Current number of in-flight tasks. Useful for tests and for
    /// exposing diagnostics to MCP clients in the future.
    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Whether the registry has any in-flight tasks. Companion to
    /// [`Self::len`] required by `clippy::len_without_is_empty`.
    pub async fn is_empty(&self) -> bool {
        self.inner.lock().await.is_empty()
    }
}
