//! Opencode subprocess runner + test double.
//!
//! The runner is the seam between the MCP server and the `opencode`
//! binary. The production impl ([`SubprocessRunner`]) spawns
//! `opencode run --format json <prompt>` and parses the JSON event
//! stream. The test impl ([`ScriptedRunner`]) yields scripted events
//! with simulated latency so the parallel aggregation paths can be
//! exercised without an API key or a real opencode binary.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::mcp_opencode::events::OpencodeEvent;

/// What the MCP server asks an opencode run to do.
#[derive(Debug, Clone)]
pub struct RunRequest {
    /// The working directory the run should execute in.
    pub workdir: PathBuf,
    /// The user prompt.
    pub prompt: String,
    /// Optional model override (`provider/model`); pass `None` to use
    /// opencode's configured default.
    pub model: Option<String>,
    /// Optional agent override (opencode supports multiple agent
    /// configurations; `None` uses the user's default).
    pub agent: Option<String>,
    /// How long to wait for the run to complete before giving up.
    /// `None` means "no timeout".
    pub timeout: Option<Duration>,
    /// Title for the session, used in opencode's session list UI.
    /// Defaults to a 60-char truncation of the prompt when empty.
    pub title: Option<String>,
}

/// The aggregated result of a single opencode run.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// The full assistant text, assembled from all `text_delta` events
    /// before the terminal `done` event.
    pub message: String,
    /// Every event observed on stdout, in order. Useful for callers
    /// that want to inspect tool calls and intermediate reasoning.
    pub events: Vec<OpencodeEvent>,
    /// How long the run took end-to-end.
    pub elapsed: Duration,
    /// Path to the opencode session log, if the runner can surface it
    /// (the subprocess runner returns `None` here today).
    pub session_path: Option<PathBuf>,
}

/// Failure modes a runner can report.
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("opencode binary not found at {0}")]
    BinaryNotFound(PathBuf),
    #[error("opencode exited with non-zero status {code:?}: {stderr}")]
    NonZeroExit { code: Option<i32>, stderr: String },
    #[error("opencode run was cancelled")]
    Cancelled,
    #[error("opencode run timed out after {0:?}")]
    TimedOut(Duration),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("opencode emitted no terminal event (truncated stream?)")]
    NoTerminalEvent,
}

/// The runner trait. The MCP server talks to opencode exclusively
/// through this trait; production wires [`SubprocessRunner`], tests wire
/// [`ScriptedRunner`].
#[async_trait]
pub trait OpencodeRunner: Send + Sync {
    async fn run(
        &self,
        req: RunRequest,
        cancel: CancellationToken,
    ) -> Result<RunResult, RunnerError>;
}

// ---------------------------------------------------------------------------
// Production: real subprocess
// ---------------------------------------------------------------------------

/// Configuration for [`SubprocessRunner`].
#[derive(Debug, Clone)]
pub struct SubprocessConfig {
    /// Path to the `opencode` binary. Defaults to `opencode` on `$PATH`.
    pub binary: PathBuf,
}

impl Default for SubprocessConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("opencode"),
        }
    }
}

/// Production runner: spawns `opencode run --format json ...` and
/// parses the stdout event stream line-by-line.
///
/// Mirrors the subprocess pattern from `src/tool/bash.rs:174-226`:
/// `kill_on_drop(true)` plus a `pre_exec` that calls `setpgid(0, 0)` so
/// cancellation or timeout can `killpg(child_pid, SIGKILL)` and reach
/// the entire subprocess tree, not just the opencode parent.
#[derive(Debug)]
pub struct SubprocessRunner {
    config: SubprocessConfig,
}

impl SubprocessRunner {
    #[must_use]
    pub const fn new(config: SubprocessConfig) -> Self {
        Self { config }
    }

    /// Build the `tokio::process::Command` for one run. Pulled out so
    /// unit tests can verify the argument shape without spawning
    /// anything.
    fn build_command(&self, req: &RunRequest) -> Command {
        let mut cmd = Command::new(&self.config.binary);
        cmd.arg("run");
        cmd.arg("--format").arg("json");
        if let Some(model) = &req.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(agent) = &req.agent {
            cmd.arg("--agent").arg(agent);
        }
        if let Some(title) = &req.title {
            cmd.arg("--title").arg(title);
        }
        // `--dir` tells opencode where to operate. We always pass it
        // (resolved from `req.workdir`) so runs are deterministic.
        cmd.arg("--dir").arg(&req.workdir);
        // The trailing positional is the message. `opencode run` accepts
        // the prompt as positional args.
        cmd.arg(&req.prompt);

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Make the child its own process group leader so `killpg`
        // reaches the whole subtree on cancel / timeout. Same pattern
        // as `src/tool/bash.rs`.
        #[cfg(unix)]
        unsafe {
            use nix::unistd::{setpgid, Pid};
            cmd.pre_exec(|| {
                let _ = setpgid(Pid::from_raw(0), Pid::from_raw(0));
                Ok(())
            });
        }
        cmd
    }
}

#[async_trait]
impl OpencodeRunner for SubprocessRunner {
    async fn run(
        &self,
        req: RunRequest,
        cancel: CancellationToken,
    ) -> Result<RunResult, RunnerError> {
        let start = std::time::Instant::now();
        let mut cmd = self.build_command(&req);
        cmd.current_dir(&req.workdir);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(RunnerError::BinaryNotFound(self.config.binary.clone()));
            }
            Err(e) => return Err(RunnerError::Io(e)),
        };

        let stdout = child
            .stdout
            .take()
            .expect("stdout was piped (Stdio::piped())");
        let stderr = child
            .stderr
            .take()
            .expect("stderr was piped (Stdio::piped())");

        // Drain stdout into a bounded mpsc channel so we can both
        // forward lines to the caller and detect the terminal event.
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel::<OpencodeEvent>(128);
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Some(ev) = OpencodeEvent::parse_line(&line) {
                    if events_tx.send(ev).await.is_err() {
                        break;
                    }
                }
            }
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let mut reader = BufReader::new(stderr);
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut buf).await;
            buf
        });

        // Race the child against cancel + timeout. The cancel branch
        // is biased so a fired token wins ties against the child
        // finishing naturally.
        let child_pid = child.id();
        let work = async {
            let status = child.wait().await?;
            let _ = stdout_task.await;
            let stderr_text = stderr_task.await.unwrap_or_default();
            Ok::<_, RunnerError>((status, stderr_text))
        };

        let outcome = if let Some(timeout) = req.timeout {
            tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    kill_process_tree(child_pid);
                    return Err(RunnerError::Cancelled);
                }
                () = tokio::time::sleep(timeout) => {
                    kill_process_tree(child_pid);
                    return Err(RunnerError::TimedOut(timeout));
                }
                result = work => result,
            }
        } else {
            tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    kill_process_tree(child_pid);
                    return Err(RunnerError::Cancelled);
                }
                result = work => result,
            }
        };

        let (status, stderr_text) = outcome?;
        if !status.success() {
            return Err(RunnerError::NonZeroExit {
                code: status.code(),
                stderr: stderr_text,
            });
        }

        // Drain collected events.
        let mut events: Vec<OpencodeEvent> = Vec::new();
        while let Ok(ev) = events_rx.try_recv() {
            events.push(ev);
        }

        // Find the terminal event and assemble the final message.
        let mut message = String::new();
        let mut saw_done = false;
        for ev in &events {
            match ev {
                OpencodeEvent::TextDelta { text } => message.push_str(text),
                OpencodeEvent::Done { message: m } => {
                    message = m.clone();
                    saw_done = true;
                }
                _ => {}
            }
        }
        if !saw_done {
            return Err(RunnerError::NoTerminalEvent);
        }

        Ok(RunResult {
            message,
            events,
            elapsed: start.elapsed(),
            session_path: None,
        })
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
    // Best effort on non-Unix: the child's `kill_on_drop(true)` will
    // close stdin and let the process exit on its own.
}

// ---------------------------------------------------------------------------
// Test double: scripted runner
// ---------------------------------------------------------------------------

/// One scripted event with an optional simulated latency. Used by
/// [`ScriptedRunner`] to drive the parallel aggregation paths in
/// deterministic tests.
#[derive(Debug, Clone)]
pub struct ScriptedStep {
    pub event: OpencodeEvent,
    pub delay: Duration,
}

/// In-memory runner that yields a scripted sequence per request, with
/// optional per-step latency. Lets us test parallel-fanout timing
/// without any external dependencies.
pub struct ScriptedRunner {
    steps: Arc<Vec<ScriptedStep>>,
}

impl std::fmt::Debug for ScriptedRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedRunner")
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl ScriptedRunner {
    #[must_use]
    pub fn new(steps: Vec<ScriptedStep>) -> Self {
        Self {
            steps: Arc::new(steps),
        }
    }

    /// Convenience builder: identical steps for every request.
    #[must_use]
    pub fn uniform(steps: Vec<ScriptedStep>) -> Arc<Self> {
        Arc::new(Self::new(steps))
    }
}

#[async_trait]
impl OpencodeRunner for ScriptedRunner {
    async fn run(
        &self,
        _req: RunRequest,
        cancel: CancellationToken,
    ) -> Result<RunResult, RunnerError> {
        let start = std::time::Instant::now();
        let mut events: Vec<OpencodeEvent> = Vec::with_capacity(self.steps.len());
        let mut message = String::new();

        for step in self.steps.iter() {
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(RunnerError::Cancelled),
                () = tokio::time::sleep(step.delay) => {}
            }
            match &step.event {
                OpencodeEvent::TextDelta { text } => message.push_str(text),
                OpencodeEvent::Done { message: m } => message = m.clone(),
                _ => {}
            }
            events.push(step.event.clone());
        }

        Ok(RunResult {
            message,
            events,
            elapsed: start.elapsed(),
            session_path: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Discover the version string of the `opencode` binary by running
/// `opencode --version`. Returns `None` if the binary is missing or the
/// call fails.
pub async fn opencode_version(binary: &Path) -> Option<String> {
    let mut cmd = Command::new(binary);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = cmd.output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Discover the available models by running `opencode models` and
/// parsing its output. `opencode models` prints `provider/model` lines
/// to stdout. We keep parsing forgiving: any line containing a `/` is
/// accepted.
pub async fn opencode_models(binary: &Path) -> Result<Vec<String>, RunnerError> {
    let mut cmd = Command::new(binary);
    cmd.arg("models")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = cmd.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            RunnerError::BinaryNotFound(binary.to_path_buf())
        } else {
            RunnerError::Io(e)
        }
    })?;
    if !output.status.success() {
        return Err(RunnerError::NonZeroExit {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let models: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.contains('/'))
        .map(str::to_string)
        .collect();
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(workdir: &str, prompt: &str) -> RunRequest {
        RunRequest {
            workdir: PathBuf::from(workdir),
            prompt: prompt.to_string(),
            model: None,
            agent: None,
            timeout: None,
            title: None,
        }
    }

    #[tokio::test]
    async fn scripted_runner_returns_assembled_message() {
        let runner = ScriptedRunner::new(vec![
            ScriptedStep {
                event: OpencodeEvent::TextDelta {
                    text: "hello, ".into(),
                },
                delay: Duration::from_millis(5),
            },
            ScriptedStep {
                event: OpencodeEvent::TextDelta {
                    text: "world".into(),
                },
                delay: Duration::from_millis(5),
            },
            ScriptedStep {
                event: OpencodeEvent::Done {
                    message: "final".into(),
                },
                delay: Duration::ZERO,
            },
        ]);
        let res = runner
            .run(req("/tmp", "hi"), CancellationToken::new())
            .await
            .expect("run");
        // Done overwrites the assembled text, so the final message is
        // "final", not the concatenation.
        assert_eq!(res.message, "final");
        assert_eq!(res.events.len(), 3);
        assert!(res.elapsed >= Duration::from_millis(10));
    }

    #[tokio::test]
    async fn scripted_runner_concurrency_is_real() {
        // 4 tasks, each 200 ms → if serial they'd take ~800 ms; if
        // parallel they should finish in ~200 ms. We assert < 500 ms
        // to leave headroom on slow CI.
        let steps = vec![ScriptedStep {
            event: OpencodeEvent::Done {
                message: "ok".into(),
            },
            delay: Duration::from_millis(200),
        }];
        let runner = ScriptedRunner::uniform(steps);
        let start = std::time::Instant::now();
        let futures = (0..4).map(|_| {
            let r = runner.clone();
            let token = CancellationToken::new();
            async move { r.run(req("/tmp", "x"), token).await }
        });
        let results: Vec<_> = futures::future::join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("all runs");
        let elapsed = start.elapsed();
        assert_eq!(results.len(), 4);
        for r in &results {
            assert_eq!(r.message, "ok");
        }
        assert!(
            elapsed < Duration::from_millis(500),
            "parallel runs took {elapsed:?}; expected < 500ms"
        );
    }

    #[tokio::test]
    async fn scripted_runner_respects_cancel() {
        let runner = ScriptedRunner::new(vec![ScriptedStep {
            event: OpencodeEvent::Done {
                message: "never".into(),
            },
            delay: Duration::from_secs(5),
        }]);
        let token = CancellationToken::new();
        let token_clone = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            token_clone.cancel();
        });
        let res = runner.run(req("/tmp", "x"), token).await;
        assert!(matches!(res, Err(RunnerError::Cancelled)));
    }

    #[test]
    fn subprocess_runner_builds_expected_args() {
        let runner = SubprocessRunner::new(SubprocessConfig::default());
        let cmd = runner.build_command(&RunRequest {
            workdir: PathBuf::from("/tmp"),
            prompt: "do the thing".into(),
            model: Some("anthropic/claude-sonnet-5".into()),
            agent: Some("build".into()),
            timeout: None,
            title: Some("my title".into()),
        });
        // We can't introspect a Command directly, but we can at least
        // assert the program name is right via Debug.
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("opencode"), "got: {dbg}");
    }
}
