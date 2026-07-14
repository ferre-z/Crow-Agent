### Task 3.3 — `bash` tool

**Files:**
- Create: `src/tool/bash.rs`

**Spec references:** v0 spec §13 bash tool, §4 (process-group termination, bounded output), §3.2 (no sandboxing).

**Why this exists:** the bash tool is the only way the model can run shell commands. The trust model is: bash runs with the same privileges as the agent process. The tool's job is to enforce resource limits (timeout, output cap) and respond to cancellation, NOT to contain a malicious shell (spec §3.2).

**Interfaces (exact):**

```rust
// src/tool/bash.rs
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use crate::tool::{Tool, ToolContext, ToolEventSink, ToolError, ToolResult, ToolOutcome};
use crate::event::{AgentEvent, ErrorCode, ToolCallId, ToolStream};

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct BashArgs {
    /// Shell command to run.
    pub command: String,
    /// Optional timeout in seconds. If absent, uses ctx.command_timeout.
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
}

pub struct BashTool;

#[async_trait::async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str { "bash" }
    fn description(&self) -> &'static str { "Run a shell command in the project directory. Streams stdout and stderr as events. Honors timeout and cancellation via SIGTERM then SIGKILL on the process group." }
    fn schema(&self) -> schemars::Schema { schemars::schema_for!(BashArgs) }
    async fn execute(&self, args: serde_json::Value, ctx: ToolContext, events: ToolEventSink, cancel: CancellationToken) -> ToolResult {
        // 1. Parse args. Resolve $SHELL or fall back to /bin/sh.
        // 2. Spawn the command with stdio piped and `setsid` (new process group).
        // 3. Spawn two tokio tasks: one reading stdout, one reading stderr.
        //    Each task sends AgentEvent::ToolOutput { call_id, stream, chunk } for each line read.
        //    Each task accumulates into a bounded Vec<u8> capped at ctx.max_output_bytes.
        // 4. tokio::select! over the wait + the cancel token + a timeout.
        // 5. On cancel/timeout: killpg(Pid::from_raw(pid), SIGTERM), wait 5s, then SIGKILL.
        // 6. Return ToolOutcome::Success { output: <stdout + stderr, truncated flag>, truncated: bool }
        //    (or ToolOutcome::Error with the exit code).
        todo!()
    }
}

pub fn default_shell() -> std::path::PathBuf;
```

**`ToolOutput` streaming semantics:**
- Each line of stdout/stderr emits one `AgentEvent::ToolOutput { call_id, stream: Stdout|Stderr, chunk: <bytes> }` event.
- If the channel is full, drop the chunk and increment a counter. After the process exits, if the counter > 0, append a final `ToolOutput` event with the dropped count.

**Acceptance:**
- 18+ unit tests:
  1. command exit 0 → ToolOutcome::Success
  2. command exit 1 → ToolOutcome::Error
  3. command not found → ToolOutcome::Error with code "not_found"
  4. command hangs (e.g. `sleep 100`) → timeout fires, returns ToolOutcome::Error with code "timeout"
  5. command spawns a subprocess (`bash -c 'sleep 100 &'`) → cancel kills the parent AND the child
  6. command runs forever (timeout=1s) → timeout fires within 1.5s
  7. command output > 1MB → truncated flag is true
  8. command output that fits in 1MB → no truncation
  9. SIGTERM grace period is 5s, then SIGKILL
  10. `$SHELL` is honored; fallback to /bin/sh works
  11. `setpgid` is set so the new process has its own pgroup
  12. cancel from cancel token kills the pgroup
  13. cancel via ToolEventSink close kills the pgroup
  14. exit code > 128 (e.g. 137 = SIGKILL) → ToolOutcome::Error with code "killed"
  15. command that writes 100MB → captured is capped at 1MB
  16. `cd` to a path that doesn't exist → shell returns nonzero
  17. multi-line command (`bash -c 'echo a; echo b'`) → both lines appear in output
  18. command with environment variables → they propagate
  19. cancellation token already cancelled at start → no process spawned, immediate error

**Forbidden:**
- No `unsafe`. Use `nix` crate for the signal/process-group calls.
- No shell-string parsing or quoting. The shell handles that.
- No `unwrap`/`expect` in library code.
- No dropping a ToolOutput event silently without a counter (so the UI knows data was lost).

**Dependencies:** `nix` already in Cargo.toml. `tokio::process` enabled.
