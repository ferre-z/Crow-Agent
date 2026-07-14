### Task 4.4 — Approval policy

**Files:**
- Create: `src/policy.rs`
- Modify: `src/agent.rs` (the agent loop checks the policy before each tool execution)
- Modify: `src/lib.rs` (`pub mod policy;`)

**Why this exists:** the v0 spec says the agent is "autonomous by default" with no permission prompts. That's correct for the headless CLI. But the desktop wants to gate tool calls behind user approval. The policy module is the seam: the kernel doesn't know about approvals, but it checks the policy before each tool call. A `NoOp` policy preserves the spec's behaviour; an `Ask` policy enables the desktop UX.

**Spec references:** v0 spec §3.2 (no permission engine in v0) — we are adding the *seam*, not enforcing approvals. The kernel stays autonomous; the policy is opt-in per session.

**Interfaces (exact):**

```rust
// src/policy.rs
use async_trait::async_trait;
use std::sync::Arc;
use serde_json::Value;
use crate::tool::{ToolContext, ToolError, ToolOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow { reason: String },
    Deny { reason: String },
    /// "Ask" is what the kernel reports to the policy. The policy
    /// itself decides whether to ask the user, allow, or deny. In v0
    /// we resolve "Ask" to "Allow" (preserving the spec's autonomous
    /// behaviour) unless the policy overrides.
    Ask,
}

#[async_trait]
pub trait ApprovalPolicy: Send + Sync {
    /// Called by the agent loop BEFORE each tool execution.
    /// The `tool_name`, `args`, and `context` are passed in.
    /// Returns the decision. The agent loop honors `Allow` and `Deny`.
    /// For `Ask`, the loop calls `ask_user` (which is a no-op in v0).
    async fn evaluate(
        &self,
        tool_name: &str,
        args: &Value,
        context: &ToolContext,
    ) -> Decision;

    /// Resolve an "Ask" decision. In v0 this returns `Allow` for
    /// the spec's autonomous behaviour. A future desktop policy can
    /// prompt the user.
    async fn ask_user(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Decision {
        Decision::Allow { reason: "autonomous (no user prompt in v0)".into() }
    }
}

/// The default policy: no approvals. Equivalent to v0 spec §4.
pub struct NoOpPolicy;

#[async_trait]
impl ApprovalPolicy for NoOpPolicy {
    async fn evaluate(&self, _tool: &str, _args: &Value, _ctx: &ToolContext) -> Decision {
        Decision::Allow { reason: "no policy (v0 default)".into() }
    }
}

/// Wrap a policy in an Arc so the kernel can share it across
/// tool calls without copying.
pub type SharedPolicy = Arc<dyn ApprovalPolicy>;
```

**`Agent` change:** `Agent::new()` and `Agent::submit()` get a new parameter: `policy: SharedPolicy`. Before each tool execution (inside the loop in spec §11), the loop calls `policy.evaluate(...)` and branches on the result.

**Procedure:**
1. Implement `policy.rs` per the interfaces above. Plus unit tests for `NoOpPolicy` (always allows).
2. Modify `src/agent.rs`:
   - Add `policy: SharedPolicy` to `AgentConfig`
   - In the tool execution branch, call `policy.evaluate(tool_name, &call.args, &ctx)`. If `Deny`, emit a `ToolResult { is_error: true, output: "denied by policy" }` and continue (the model can retry or change approach). If `Ask`, call `policy.ask_user` and branch on the result.
3. Update the wave-1-3 tests to construct a `NoOpPolicy` and pass it.
4. Add 6+ new unit tests for the policy integration:
   - NoOp policy always allows
   - Deny policy emits a denied ToolResult
   - Ask policy resolves through `ask_user` (which in v0 allows)
   - Policy.evaluate receives the right tool_name and args
   - Policy error is not a panic — the agent loop survives a broken policy

**Acceptance:**
- 8+ unit tests in `policy.rs`
- 4+ tests for the agent loop's policy integration
- All existing tests still pass (with `NoOpPolicy` injected)
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` clean

**Forbidden:**
- No `unwrap`/`expect` in library code.
- No user prompts in `NoOpPolicy` or `ask_user`'s default impl — the spec is explicit that v0 is autonomous.
- No global policy state. Policy is per-`Agent` via the `Arc<dyn ApprovalPolicy>` in `AgentConfig`.
- No `panic!` if the policy errors — the loop turns it into a deny + emit an event.

**Dependency:** none new. `async-trait`, `serde_json` are already in.
