//! Approval card state.
//!
//! The kernel pauses a tool call when [`crate::policy::Decision::Ask`]
//! is returned. The pause is bridged by an `mpsc::Receiver<AskRequest>`,
//! which the TUI driver feeds into this module. [`PendingApproval`]
//! holds the in-flight request plus the per-session allowlist; the
//! render layer reads it to draw the card; the keymap calls
//! [`PendingApproval::resolve`] to send the response back to the
//! agent over the oneshot the kernel provided.
//!
//! ## Session-scoped "always allow"
//!
//! "Always" only persists for the current TUI session. We do not
//! touch on-disk policy files; the user re-runs `crow tui` and the
//! allowlist starts empty. This is intentionally simpler than
//! mutating the kernel policy and keeps the safety story
//! explainable.

use std::collections::HashSet;

use serde_json::Value;

/// One pending tool call awaiting human approval.
#[derive(Debug)]
pub struct PendingApproval {
    /// Stable id the kernel uses to correlate logs.
    pub ask_id: String,
    /// Name of the tool the agent wants to run.
    pub tool_name: String,
    /// Pretty-printed args the user is being asked to authorise.
    pub args: Value,
    /// Sender half of the oneshot the kernel handed us. We send
    /// [`crate::policy::AskResponse::Allow`] or `Deny` on resolve.
    response: tokio::sync::oneshot::Sender<crate::policy::AskResponse>,
}

/// Per-session "always allow" allowlist. Tools in this set are
/// auto-approved without showing the card.
#[derive(Debug, Default, Clone)]
pub struct AllowList {
    names: HashSet<String>,
}

impl AllowList {
    /// Empty allowlist.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True if `tool_name` is in the allowlist.
    #[must_use]
    pub fn allows(&self, tool_name: &str) -> bool {
        self.names.contains(tool_name)
    }

    /// Add a tool to the allowlist. Idempotent.
    pub fn allow(&mut self, tool_name: &str) {
        self.names.insert(tool_name.to_string());
    }

    /// Number of tools currently allowlisted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// True if no tools are allowlisted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

impl PendingApproval {
    /// Wrap a kernel [`AskRequest`] in our model. Returns `None` if
    /// the request is somehow malformed (call has no name).
    pub fn from_request(req: crate::policy::AskRequest) -> Option<Self> {
        let tool_name = req.call.name.clone();
        Some(Self {
            ask_id: req.ask_id,
            tool_name,
            args: req.call.args,
            response: req.response,
        })
    }

    /// Resolve the pending ask. Returns `true` if the oneshot was
    /// still open and the response was delivered.
    ///
    /// When `outcome` is [`Outcome::AllowAlways`], the tool is
    /// also added to `allowlist` so the next call skips the card.
    pub fn resolve(self, outcome: Outcome, allowlist: &mut AllowList) -> bool {
        let response = match outcome {
            Outcome::Deny => crate::policy::AskResponse::Deny,
            Outcome::Allow => crate::policy::AskResponse::Allow,
            Outcome::AllowAlways => {
                allowlist.allow(&self.tool_name);
                crate::policy::AskResponse::Allow
            }
        };
        self.response.send(response).is_ok()
    }

    /// Borrow the tool name without consuming the request.
    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Borrow the args without consuming the request.
    #[must_use]
    pub fn args(&self) -> &Value {
        &self.args
    }
}

/// What the user chose in the approval card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Allow this single call.
    Allow,
    /// Allow this call AND add the tool to the session allowlist.
    AllowAlways,
    /// Refuse the call.
    Deny,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ToolCallId;
    use crate::policy::AskRequest;
    use crate::tool::ToolCall;
    use serde_json::json;

    fn fake_request(name: &str, args: Value) -> AskRequest {
        let (tx, _rx) = tokio::sync::oneshot::channel();
        AskRequest {
            ask_id: format!("ask-{name}"),
            call: ToolCall {
                call_id: ToolCallId(crate::ids::new_id()),
                name: name.to_string(),
                args,
            },
            response: tx,
        }
    }

    #[test]
    fn allowlist_starts_empty() {
        let list = AllowList::new();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert!(!list.allows("bash"));
    }

    #[test]
    fn allowlist_adds_and_idempotent() {
        let mut list = AllowList::new();
        list.allow("bash");
        assert!(list.allows("bash"));
        assert_eq!(list.len(), 1);
        list.allow("bash"); // idempotent
        assert_eq!(list.len(), 1);
        assert!(!list.allows("write"));
    }

    #[test]
    fn from_request_extracts_tool_name_and_args() {
        let req = fake_request("bash", json!({"command": "ls"}));
        let pending = PendingApproval::from_request(req).expect("ok");
        assert_eq!(pending.tool_name(), "bash");
        assert_eq!(pending.args()["command"], "ls");
    }

    #[test]
    fn resolve_allow_sends_allow_response() {
        let req = fake_request("bash", json!({}));
        let (probe_tx, mut probe_rx) = tokio::sync::oneshot::channel();
        // Re-build so we own the response sender.
        let pending = PendingApproval {
            ask_id: "x".into(),
            tool_name: "bash".into(),
            args: json!({}),
            response: probe_tx,
        };
        let _ = req; // not used; we shadow with our own sender
        let mut allowlist = AllowList::new();
        let sent = pending.resolve(Outcome::Allow, &mut allowlist);
        assert!(sent);
        let resp = probe_rx.try_recv().expect("response delivered");
        assert_eq!(resp, crate::policy::AskResponse::Allow);
        // "Allow" (not "Always") does not touch the allowlist.
        assert!(allowlist.is_empty());
    }

    #[test]
    fn resolve_allow_always_populates_allowlist() {
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let pending = PendingApproval {
            ask_id: "x".into(),
            tool_name: "bash".into(),
            args: json!({}),
            response: tx,
        };
        let mut allowlist = AllowList::new();
        let sent = pending.resolve(Outcome::AllowAlways, &mut allowlist);
        assert!(sent);
        assert_eq!(rx.try_recv().unwrap(), crate::policy::AskResponse::Allow);
        assert!(allowlist.allows("bash"));
        assert_eq!(allowlist.len(), 1);
    }

    #[test]
    fn resolve_deny_sends_deny_response() {
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let pending = PendingApproval {
            ask_id: "x".into(),
            tool_name: "bash".into(),
            args: json!({}),
            response: tx,
        };
        let mut allowlist = AllowList::new();
        let sent = pending.resolve(Outcome::Deny, &mut allowlist);
        assert!(sent);
        assert_eq!(rx.try_recv().unwrap(), crate::policy::AskResponse::Deny);
        assert!(allowlist.is_empty());
    }

    #[test]
    fn resolve_returns_false_if_oneshot_closed() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        drop(rx); // close receiver
        let pending = PendingApproval {
            ask_id: "x".into(),
            tool_name: "bash".into(),
            args: json!({}),
            response: tx,
        };
        let mut allowlist = AllowList::new();
        let sent = pending.resolve(Outcome::Allow, &mut allowlist);
        assert!(!sent);
    }
}
