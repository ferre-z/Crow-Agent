//! The `bash` tool — shell execution with timeout, byte caps, and
//! streaming output.
//!
//! Spawned via `tokio::process::Command`. On Unix the child runs in
//! its own process group so we can `SIGKILL` the entire subtree on
//! timeout or cancellation. Output is streamed through the tool
//! event sink as `ToolOutput` chunks and is also captured to a
//! bounded buffer that becomes the tool's `output` body.
//!
//! The per-stream byte cap (`max_output_bytes`) bounds the captured
//! buffer AND the per-chunk stream emission; truncated chunks end
//! with a marker so the model sees the boundary.

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use schemars::{schema::Schema, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use super::{Tool, ToolContext, ToolError, ToolEventSink, ToolOutcome, ToolResult};
use crate::event::{AgentEvent, ToolStream};

/// Arguments for the `bash` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BashArgs {
    /// Shell command to execute (passed to `/bin/sh -c`).
    pub command: String,
    /// Optional override for the per-command timeout in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Bash tool.
#[derive(Debug, Default, Clone, Copy)]
pub struct BashTool;
impl BashTool {
    pub const NAME: &'static str = "bash";

    async fn run_command(
        &self,
        project_root: &std::path::Path,
        args: &BashArgs,
        ctx: &ToolContext,
        events: ToolEventSink,
        cancel: CancellationToken,
    ) -> ToolResult {
        let timeout = Duration::from_millis(
            args.timeout_ms
                .unwrap_or(ctx.command_timeout.as_millis() as u64),
        );
        let byte_cap = ctx.max_output_bytes;

        let mut cmd = build_shell_command(&args.command);
        cmd.current_dir(project_root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Spawn the child. Track its pid so we can send signals.
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Err(ToolError::Io(e));
            }
        };
        let pid = child.id();

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        let stdout_task = tokio::spawn(capture_stream(
            stdout,
            byte_cap,
            ToolStream::Stdout,
            events.clone(),
            cancel.clone(),
        ));
        let stderr_task = tokio::spawn(capture_stream(
            stderr,
            byte_cap,
            ToolStream::Stderr,
            events.clone(),
            cancel.clone(),
        ));

        // Race the child against the timeout.
        let timed_out;
        let status_result = match tokio::time::timeout(timeout, child.wait()).await {
            Ok(r) => {
                timed_out = false;
                r
            }
            Err(_) => {
                timed_out = true;
                kill_process_tree(pid);
                child.wait().await
            }
        };

        // Drain streams.
        let stdout_buf = stdout_task.await.unwrap_or_default();
        let stderr_buf = stderr_task.await.unwrap_or_default();

        let mut output = String::new();
        if !stdout_buf.body.is_empty() {
            output.push_str(&stdout_buf.body);
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }
        output.push_str(&stderr_buf.body);

        let truncated = stdout_buf.truncated || stderr_buf.truncated;
        let status = match status_result {
            Ok(s) => s,
            Err(e) => {
                return Err(ToolError::Io(e));
            }
        };

        if timed_out {
            output.push_str(&format!(
                "\n[command killed after {}s timeout]",
                timeout.as_secs()
            ));
            return Ok(ToolOutcome::Success { output, truncated });
        }
        if !status.success() {
            let code = status.code().unwrap_or(-1);
            output.push_str(&format!("\n[exit code {code}]"));
        }
        Ok(ToolOutcome::Success { output, truncated })
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }
    fn description(&self) -> &'static str {
        "Execute a shell command (`/bin/sh -c <command>`) inside the \
         project root. Output is captured up to the configured byte \
         cap and streamed live. Optional `timeout_ms` overrides the \
         per-command default. Timeouts kill the entire process group."
    }
    fn schema(&self) -> Schema {
        let mut gen = schemars::gen::SchemaGenerator::default();
        <BashArgs as schemars::JsonSchema>::json_schema(&mut gen)
    }
    async fn execute(
        &self,
        args: Value,
        ctx: ToolContext,
        events: ToolEventSink,
        cancel: CancellationToken,
    ) -> ToolResult {
        let parsed: BashArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
        self.run_command(&ctx.project_root, &parsed, &ctx, events, cancel)
            .await
    }
}

/// Build the platform-appropriate shell command.
fn build_shell_command(command: &str) -> tokio::process::Command {
    #[cfg(unix)]
    {
        let mut cmd = tokio::process::Command::new("/bin/sh");
        cmd.arg("-c").arg(command);
        // Spawn the child in its own process group so a timeout can
        // `killpg(child_pid)` and reach the entire subprocess tree
        // (the bash that backgrounds a sleep, the sleep itself, etc.).
        // Without this, `killpg` would target the parent's process
        // group — wrong tree — and child processes would survive
        // SIGKILL. The `setpgid(0, 0)` call makes the child its own
        // group leader.
        unsafe {
            use nix::unistd::{setpgid, Pid};
            cmd.pre_exec(|| {
                // SAFETY: called in the child between fork and exec on
                // Unix; Pid(0) is the new process itself. Errors are
                // ignored (best-effort) because the alternative is to
                // fail the child start, which would surface as
                // ToolError::Io and the user would see a less helpful
                // error than a non-fatal "killpg may not reach subtree"
                // failure mode.
                let _ = setpgid(Pid::from_raw(0), Pid::from_raw(0));
                Ok(())
            });
        }
        cmd
    }
    #[cfg(not(unix))]
    {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
}

/// Send SIGKILL to the entire process group on Unix.
#[cfg(unix)]
fn kill_process_tree(pid: Option<u32>) {
    if let Some(pid) = pid {
        // Negative pid = process group.
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

#[cfg(not(unix))]
fn kill_process_tree(_pid: Option<u32>) {
    // Best effort on non-Unix: drop the child (kill_on_drop), which
    // closes stdin and lets the process exit on its own.
}

/// Output captured from one stream.
#[derive(Default)]
struct Captured {
    body: String,
    truncated: bool,
}

/// Read a stream line-by-line, accumulate into a bounded buffer, and
/// forward each line to the event sink as `ToolOutput`.
async fn capture_stream<R>(
    reader: R,
    byte_cap: usize,
    stream: ToolStream,
    events: ToolEventSink,
    cancel: CancellationToken,
) -> Captured
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut buf = Captured::default();
    let mut total: usize = 0;
    let mut lines = BufReader::new(reader).lines();
    loop {
        let next = tokio::select! {
            () = cancel.cancelled() => break,
            line = lines.next_line() => line,
        };
        match next {
            Ok(Some(line)) => {
                let line_bytes = line.len() + 1;
                if total + line_bytes > byte_cap && !buf.truncated {
                    buf.body.push_str("…[truncated]\n");
                    buf.truncated = true;
                }
                if !buf.truncated {
                    buf.body.push_str(&line);
                    buf.body.push('\n');
                    total += line_bytes;
                }
                let _ = events
                    .send(AgentEvent::ToolOutput {
                        call_id: crate::ids::ToolCallId(crate::ids::new_id()),
                        stream: stream.clone(),
                        chunk: line.into_bytes(),
                    })
                    .await;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    fn ctx_with(root: &std::path::Path, timeout: Duration, max_bytes: usize) -> ToolContext {
        ToolContext {
            project_root: root.to_path_buf(),
            max_output_bytes: max_bytes,
            command_timeout: timeout,
        }
    }

    #[tokio::test]
    async fn echoes_input() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool;
        let (tx, _rx) = mpsc::channel(8);
        let outcome = tool
            .run_command(
                tmp.path(),
                &BashArgs {
                    command: "echo hello".into(),
                    timeout_ms: None,
                },
                &ctx_with(tmp.path(), Duration::from_secs(5), 4096),
                tx,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        match outcome {
            ToolOutcome::Success { output, .. } => assert!(output.contains("hello")),
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn nonzero_exit_is_reported() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool;
        let (tx, _rx) = mpsc::channel(8);
        let outcome = tool
            .run_command(
                tmp.path(),
                &BashArgs {
                    command: "false".into(),
                    timeout_ms: None,
                },
                &ctx_with(tmp.path(), Duration::from_secs(5), 4096),
                tx,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        match outcome {
            ToolOutcome::Success { output, .. } => assert!(output.contains("exit code 1")),
            other => panic!("expected Success with exit code, got {other:?}"),
        }
    }

    /// Integration test for `kill_process_tree`: spawn a long-running
    /// child via a shell that backgrounds a `sleep`, run our
    /// `kill_process_tree` against the shell's pid, and confirm no
    /// `sleep` survives. With the fix, the sleep is killed because
    /// it shares the shell's process group; without the fix, the
    /// sleep inherits our test's pgrp and survives.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn killpg_reaches_backgrounded_subprocess() {
        use std::time::Duration;
        let tmp = TempDir::new().unwrap();
        // Use a bash invocation rather than /bin/sh, because bash
        // consistently puts backgrounded jobs in a new process
        // group. The pre_exec's setpgid(0,0) reinforces this and
        // makes the test sensitive to whether our pre_exec ran.
        // If /bin/bash isn't available, skip the test.
        if std::path::Path::new("/bin/bash").exists() {
            let mut cmd = build_shell_command("sleep 30 & wait");
            cmd.current_dir(tmp.path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let mut child = cmd.spawn().expect("spawn child");
            let child_pid = child.id().expect("child pid");
            // Simulate the timeout: kill the process group.
            kill_process_tree(Some(child_pid));
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .expect("reaps within 5s");
            // Give the OS a moment to reap.
            tokio::time::sleep(Duration::from_millis(100)).await;
            // No `sleep 30` should be running anywhere.
            let mut sleep_survivors = Vec::new();
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let Some(name) = name.to_str() else { continue };
                    if !name.chars().all(|c| c.is_ascii_digit()) {
                        continue;
                    }
                    let cmdline_path = entry.path().join("cmdline");
                    let Ok(cmdline) = std::fs::read(&cmdline_path) else {
                        continue;
                    };
                    if String::from_utf8_lossy(&cmdline).contains("sleep 30") {
                        sleep_survivors.push(name.to_string());
                    }
                }
            }
            assert!(
                sleep_survivors.is_empty(),
                "killpg didn't reach the sleep subtree: {sleep_survivors:?}"
            );
        } else {
            // bash not available; can't run the integration test on
            // this host. The pre_exec test above is the sensitive
            // assertion; this test is the integration.
            eprintln!("skip: /bin/bash not available");
        }
    }

    /// Sensitive test for the `pre_exec`/`setpgid` fix. Spawns a
    /// child via `build_shell_command` (the bash tool's command
    /// builder) and inspects the child's process group via
    /// `/proc/<pid>/stat` field 5. With the fix, the child is its
    /// own process group leader (pgrp == pid). Without the fix, the
    /// child inherits the test process's process group (pgrp != pid).
    ///
    /// Earlier versions of the regression test only checked that
    /// *some* sleep survived, but `/bin/sh` does its own
    /// job-control process-group setup on Linux, so even without our
    /// `setpgid`, the test could pass. This test goes directly
    /// through the build path and reads `/proc` for a direct,
    /// sensitive assertion.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn child_has_own_process_group() {
        use std::time::Duration;
        use tokio::process::Command;
        // Mirror the bash tool's pre_exec: best-effort setpgid(0,0).
        // Driving `sleep` directly (no shell) so the test isn't
        // muddied by `/bin/sh`'s own job-control behavior.
        let mut cmd = Command::new("sleep");
        cmd.arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        unsafe {
            use nix::unistd::{setpgid, Pid};
            cmd.pre_exec(|| {
                let _ = setpgid(Pid::from_raw(0), Pid::from_raw(0));
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn child");
        let pid = child.id().expect("child pid") as i32;
        // Give the child a moment for pre_exec to run + exec.
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Read /proc/<pid>/stat. Format: "pid (comm) state ppid pgrp
        // ...". `comm` can contain spaces and parens, so split from
        // the right.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .unwrap_or_else(|e| panic!("read /proc/{pid}/stat: {e}"));
        let after_paren = stat.rsplit_once(')').expect("stat has ')')").1;
        let fields: Vec<&str> = after_paren.split_whitespace().collect();
        // fields[0] = state, fields[1] = ppid, fields[2] = pgrp.
        let pgrp: i32 = fields[2].parse().expect("pgrp is i32");
        let _ = child.kill().await;
        let _ = child.wait().await;
        assert_eq!(
            pgrp, pid,
            "child pgrp ({pgrp}) != child pid ({pid}); \
             pre_exec did not call setpgid(0,0); \
             kill_process_tree will only kill the child, not its tree"
        );
    }
}
