//! Terminal user interface for Crow.
//!
//! Mirrors the Claude Code / opencode `tui` workflow: type a prompt,
//! watch the model stream its answer, follow tool cards inline, send
//! the next prompt when the run finishes. The TUI reuses the same
//! [`Agent`] kernel the headless `crow exec` and the `crow serve`
//! app-server already drive — the kernel stays headless, the TUI is
//! purely a presentation layer.
//!
//! ## Architecture
//!
//! The TUI splits along three concerns:
//!
//! - [`app`] owns the model (history, input buffer, scroll, status).
//!   All state mutations happen here.
//! - [`ui`] is pure rendering: takes the model, draws the frame.
//!   No state changes.
//! - The driver in this module glues the two together with the
//!   kernel's built-in [`ChannelSink`]. A background task owns the
//!   [`Agent`] and runs `submit` on demand; events stream through a
//!   Tokio mpsc to the UI task, which folds them into the model.
//!
//! ## Why a background task
//!
//! [`Agent::submit`] takes `&mut self` and runs the full loop to
//! completion. To keep the UI responsive while a run is in flight,
//! the TUI spawns a worker task that owns the agent, drains a
//! "next prompt" channel, and pushes agent events into a separate
//! "events" channel. The UI task never touches the agent directly.

pub mod app;
pub mod approval;
pub mod commands;
pub mod markdown;
pub mod picker;
pub mod tools;
pub mod ui;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, AgentConfig};
use crate::cli::default_registry;
use crate::config::Config;
use crate::event::ChannelSink;
use crate::ids::MessageId;
use crate::message::{Message, Part, Role};
use crate::provider::Provider;
use crate::session::{self, SessionWriter};

pub use app::{App, InputMode, Overlay, RunPhase};
pub use approval::{AllowList, Outcome as ApprovalOutcome, PendingApproval};
pub use commands::{parse_slash, SlashOutcome};
pub use picker::{PickerEntry, SessionPicker};

/// Entry point invoked from the `crow tui` subcommand.
///
/// Wires the terminal, builds the agent, spawns the worker task,
/// runs the main loop, and restores the terminal on exit (success or
/// error). The function only returns on a fatal error or on `/quit`.
#[allow(clippy::missing_errors_doc)]
pub async fn run(
    config: Config,
    resume: Option<String>,
    _project_root: PathBuf,
    plan_mode: bool,
    no_color: bool,
) -> Result<()> {
    // Set up the terminal. Raw mode + alternate screen so the user's
    // shell history is untouched on exit.
    crossterm::terminal::enable_raw_mode().context("enabling raw mode")?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide,
    )
    .context("entering alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("creating terminal")?;

    let result = run_inner(&mut terminal, config, resume, plan_mode, no_color).await;

    // Always restore the terminal, even on error, so the user is not
    // left staring at a broken prompt.
    let restore = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show,
    );
    restore.context("disabling raw mode")?;
    result
}

async fn run_inner(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    config: Config,
    resume: Option<String>,
    plan_mode: bool,
    no_color: bool,
) -> Result<()> {
    // Build the provider. We do this here (before any TUI drawing)
    // so credential errors surface as a clean stderr message rather
    // than a frozen blank screen.
    let provider = build_provider(&config)?;
    let tools = if plan_mode {
        crate::cli::read_only_registry()
    } else {
        default_registry()
    };
    let sessions_dir = sessions_dir_for(&config);
    tokio::fs::create_dir_all(&sessions_dir)
        .await
        .context("creating sessions directory")?;

    // Resolve session path. `--resume <id>` reuses an existing log;
    // otherwise start a fresh ULID-named file.
    let session_path = if let Some(prefix) = resume.as_deref() {
        resolve_session_id(&sessions_dir, prefix).await?
    } else {
        new_session_path(&sessions_dir)
    };

    // Event channel: worker -> UI. The kernel's `ChannelSink::new`
    // returns both halves; we hand the sender to the sink and keep
    // the receiver in the UI loop.
    let (event_sink, event_rx) = ChannelSink::new(256);

    // Prompt channel: UI -> worker. The worker waits here for the
    // next user message.
    let (prompt_tx, prompt_rx) = mpsc::channel::<String>(8);

    // Cancellation: UI owns the master token and signals the worker
    // when the user interrupts a run (Esc, Ctrl+C).
    let cancel = CancellationToken::new();
    let cancel_for_worker = cancel.clone();

    let writer = SessionWriter::open(&session_path)
        .await
        .with_context(|| format!("opening session log {session_path:?}"))?;
    let mut agent_cfg = AgentConfig::new(
        config.max_turns,
        config.max_tool_calls,
        config.model.clone(),
        config.project_root.clone(),
        writer,
    );

    // Ask channel: kernel -> UI. The agent pauses a tool call
    // when the policy returns `Decision::Ask` and writes an
    // `AskRequest` (with a oneshot response channel) here. The
    // driver reads from this channel and feeds the UI.
    let (ask_tx, ask_rx) = mpsc::channel::<crate::policy::AskRequest>(16);
    agent_cfg = agent_cfg.with_ask_resolver(ask_tx);

    // Build or resume the agent. For v0 the TUI starts a new
    // conversation per session; the `--resume` flag reuses an
    // existing JSONL log via `Agent::resume_into`.
    let (agent, initial_history) = if session_path.exists() && resume.is_some() {
        Agent::resume_into(
            agent_cfg,
            provider,
            tools,
            cancel_for_worker.clone(),
            Arc::new(event_sink),
            &session_path,
        )
        .await
        .context("rebuilding session")?
    } else {
        let agent = Agent::with_sink(
            agent_cfg,
            provider,
            tools,
            cancel_for_worker.clone(),
            Vec::new(),
            Arc::new(event_sink),
        );
        (agent, Vec::new())
    };

    // Worker: owns the agent, runs each prompt to completion.
    let worker_handle = tokio::spawn(worker_loop(agent, prompt_rx, cancel_for_worker));

    // App model + replay resumed history into the chat view.
    let mut app = App::new(
        config.clone(),
        session_path.clone(),
        initial_history.clone(),
        plan_mode,
        no_color,
    );

    // Main loop. We poll three sources:
    //   1. agent events (from the worker)
    //   2. terminal input (keyboard, resize)
    //   3. a 100ms tick that drives the spinner / redraw while idle
    let mut events = event_rx;
    let mut asks = ask_rx;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut term_events = crossterm::event::EventStream::new();
    let mut needs_redraw = true;

    loop {
        if needs_redraw {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
            needs_redraw = false;
        }

        tokio::select! {
            biased;

            maybe_event = events.recv() => {
                match maybe_event {
                    Some(event) => {
                        app.apply_event(event);
                        needs_redraw = true;
                    }
                    None => {
                        // Worker closed the channel — usually means
                        // the agent loop exited. UI keeps going so
                        // the user can read the last state and quit.
                        app.mark_worker_gone();
                        needs_redraw = true;
                    }
                }
            }

            // Policy-driven ask: an `AskRequest` from the agent.
            // Hand it to the App, which auto-approves if the tool is
            // in the session allowlist, otherwise shows the card.
            maybe_ask = asks.recv() => {
                if let Some(req) = maybe_ask {
                    if app.install_pending_approval(req) {
                        needs_redraw = true;
                    }
                }
            }

            term = term_events.next() => {
                if let Some(Ok(evt)) = term {
                    needs_redraw |= app.handle_terminal_event(evt);
                    if app.take_submit() {
                        let prompt = app.take_input();
                        if prompt.trim().is_empty() {
                            continue;
                        }
                        // Slash commands never reach the agent.
                        if let Some(outcome) = parse_slash(&prompt) {
                            match outcome {
                                SlashOutcome::Submit(text) => {
                                    app.record_user_message(&text);
                                    let _ = prompt_tx.send(text).await;
                                }
                                SlashOutcome::Quit => break,
                                SlashOutcome::Local { name, args } => {
                                    // `/resume` is special: it needs
                                    // async I/O to list the sessions
                                    // directory, so the driver loads
                                    // the entries and either opens
                                    // the picker or pushes a status
                                    // line explaining why it can't.
                                    if name == "resume" && args.is_empty() {
                                        if let Err(e) =
                                            open_session_picker(&mut app, &config).await
                                        {
                                            app.apply_local_slash("resume", "");
                                            let _ = e; // apply_local_slash already explained
                                        }
                                    } else {
                                        app.apply_local_slash(&name, &args);
                                    }
                                }
                            }
                            needs_redraw = true;
                            continue;
                        }
                        app.record_user_message(&prompt);
                        let _ = prompt_tx.send(prompt).await;
                        needs_redraw = true;
                    }
                }
            }

            _ = tick.tick() => {
                if app.is_running() || app.spinner_needs_tick() {
                    needs_redraw = true;
                }
            }
        }

        if app.should_quit() {
            break;
        }
    }

    // Cancel any in-flight run and wait for the worker to drain so
    // we don't leave the session log mid-write.
    cancel.cancel();
    drop(prompt_tx);
    let _ = worker_handle.await;

    // If the user picked a session from the overlay, emit a
    // copy-pasteable resume command on stdout. We print AFTER the
    // terminal is restored in `run` so the line lands on the
    // user's normal prompt, not inside the alt-screen.
    if let Some(id) = app.pending_resume.as_deref() {
        println!("Resume with: crow tui --resume {id}");
    }
    Ok(())
}

/// Load every session in the project's sessions directory and
/// open the picker overlay on `app`.
///
/// On any error (missing dir, I/O failure) the caller falls back
/// to `apply_local_slash("resume", "")` which surfaces a
/// human-readable status line. We deliberately do NOT propagate
/// the error — picker failure is a UX hiccup, not a crash.
async fn open_session_picker(app: &mut App, config: &crate::config::Config) -> anyhow::Result<()> {
    let dir = sessions_dir_for(config);
    if !dir.exists() {
        anyhow::bail!("sessions directory does not exist");
    }
    let metas = session::list_sessions(&dir).await?;
    if metas.is_empty() {
        anyhow::bail!("no sessions to resume");
    }
    let entries: Vec<PickerEntry> = metas
        .into_iter()
        .take(50) // cap the list so a thousand-session repo stays snappy
        .map(|m| PickerEntry {
            session_id: m.session_id.0.to_string(),
            started_at: format_timestamp(m.started_at),
            path_tail: short_path_for_picker(&m.path),
        })
        .collect();
    app.open_picker(entries);
    Ok(())
}

/// Shorten a session log path so it fits inside the picker row.
/// Mirrors `ui::short_path` but lives here to avoid a UI dep on
/// the driver module's private helpers.
fn short_path_for_picker(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    let max = 40;
    if s.len() <= max {
        s
    } else {
        format!("…{}", &s[s.len() - (max - 1)..])
    }
}

/// Format a [`Timestamp`] as `YYYY-MM-DDTHH:MM:SSZ` for the picker
/// row. Mirrors `cli::format_timestamp` but is private to the TUI
/// module so we don't leak CLI plumbing through the public API.
fn format_timestamp(ts: crate::ids::Timestamp) -> String {
    let dur =
        ts.0.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
    epoch_to_civil(dur.as_secs())
}

/// Howard Hinnant's date algorithm. Duplicated from `cli.rs` so the
/// TUI can format timestamps without depending on the CLI module.
fn epoch_to_civil(secs: u64) -> String {
    let (year, month, day, hour, minute, second) = {
        let days = (secs / 86_400) as i64;
        let rem = secs % 86_400;
        let hour = (rem / 3600) as u32;
        let minute = ((rem % 3600) / 60) as u32;
        let second = (rem % 60) as u32;
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (if m <= 2 { y + 1 } else { y }) as i32;
        (year, m, d, hour, minute, second)
    };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Worker task: owns the agent and runs the loop.
///
/// Each iteration waits for a prompt, then calls `agent.submit` to
/// completion. Cancellation is via the shared token; the loop
/// refuses to start a new run if the token is set.
async fn worker_loop(
    mut agent: Agent,
    mut prompt_rx: mpsc::Receiver<String>,
    cancel: CancellationToken,
) {
    while let Some(prompt) = prompt_rx.recv().await {
        if cancel.is_cancelled() {
            break;
        }
        let user_msg = Message {
            id: MessageId(crate::ids::new_id()),
            role: Role::User,
            parts: vec![Part::Text { text: prompt }],
        };
        // Every event of interest has already been pushed to the UI
        // via the sink; we deliberately drop the returned terminal
        // event here. Errors become `AgentEvent::RunFailed` events,
        // so the UI still sees them.
        let _ = agent.submit(user_msg).await;
    }
}

/// Build a provider using the same rules as the headless CLI:
/// genai if an API key is set, otherwise the scripted mock (which
/// produces no events on its own — useful for `crow tui` smoke runs
/// without credentials).
fn build_provider(config: &Config) -> Result<Arc<dyn Provider>> {
    let key = secrecy::ExposeSecret::expose_secret(&config.api_key);
    if !key.is_empty() {
        let provider = crate::provider::genai::GenaiProvider::with_api_key(
            &config.base_url,
            &config.model,
            key.to_string(),
        );
        return Ok(Arc::new(provider));
    }
    tracing::warn!("no API key configured; TUI runs will see no model output");
    Ok(Arc::new(
        crate::provider::mock::ScriptedProvider::from_events(Vec::new()),
    ))
}

/// Compute the sessions directory under `<project_root>/.crow/sessions`.
fn sessions_dir_for(config: &Config) -> PathBuf {
    config.project_root.join(".crow").join("sessions")
}

/// Build a fresh session log path (ULID-named JSONL file).
fn new_session_path(dir: &std::path::Path) -> PathBuf {
    dir.join(format!("{}.jsonl", ulid::Ulid::new()))
}

/// Resolve a session id (full or prefix) to its JSONL path. Mirrors
/// the helper in `src/cli.rs`; duplicated here so the TUI driver
/// doesn't depend on internal CLI plumbing.
async fn resolve_session_id(dir: &std::path::Path, id_or_prefix: &str) -> Result<PathBuf> {
    let metas = session::list_sessions(dir).await?;
    let matches: Vec<&session::SessionMeta> = metas
        .iter()
        .filter(|m| m.session_id.0.to_string().starts_with(id_or_prefix))
        .collect();
    match matches.len() {
        1 => Ok(matches[0].path.clone()),
        n if n > 1 => {
            anyhow::bail!(
                "session id prefix {id_or_prefix:?} matched {n} sessions; be more specific"
            )
        }
        _ => anyhow::bail!("no session matched {id_or_prefix:?}"),
    }
}
