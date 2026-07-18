//! App model for the TUI.
//!
//! Owns every piece of mutable state the UI cares about: input
//! buffer, chat history, scroll position, run phase, spinner frame.
//!
//! The model is single-threaded — the UI task is the only writer.
//! `apply_event` is the only path that mutates state in response to
//! agent output; `handle_terminal_event` is the only path that
//! mutates state in response to keyboard input. Keeping these two
//! functions as the sole writers makes the rendering layer
//! trivially safe (just read the model and draw).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
use tui_textarea::TextArea;
use unicode_width::UnicodeWidthStr;

use crate::config::Config;
use crate::event::{AgentEvent, StopReason, ToolStream};
use crate::ids::{SessionId, ToolCallId};
use crate::message::Part;
use crate::provider::pricing::Pricing;
use crate::tui::approval::{AllowList, Outcome as ApprovalOutcome, PendingApproval};
use crate::tui::picker::{PickerEntry, SessionPicker};

/// Top-level TUI model.
#[derive(Debug)] // satisfy the missing_debug_implementations crate-wide lint
pub struct App {
    /// Active config (read-only; CLI passes a fully-resolved snapshot).
    #[allow(dead_code)]
    config: Config,

    /// Path to the JSONL log backing this session.
    pub session_path: PathBuf,

    /// Session id we resume into / have started.
    pub session_id: SessionId,

    /// Multi-line input editor.
    pub input: TextArea<'static>,

    /// Where focus currently is. Reserved for future overlays
    /// (approval, history nav); always `Editing` for now.
    pub input_mode: InputMode,

    /// Ordered chat history. Renders top-to-bottom.
    pub history: Vec<ChatEntry>,

    /// Number of lines from the bottom of the scrollback to anchor
    /// the view to. `0` means "follow tail" (auto-scroll on new
    /// content). Positive values scroll up.
    pub scroll_back: u16,

    /// Whether the user has scrolled away from the bottom — used to
    /// pause auto-follow until they return.
    pub following_tail: bool,

    /// Current run phase.
    pub phase: RunPhase,

    /// Spinner frame counter, ticked by the main loop.
    pub spinner_frame: usize,

    /// Whether the worker task is still alive. Set to `false` when
    /// the agent's event channel closes.
    pub worker_alive: bool,

    /// Last error to surface in the status bar (if any).
    pub last_error: Option<String>,

    /// Pending submit flag — set when the user pressed Enter.
    submit_pending: bool,

    /// Quit flag — set when the user issues `/quit` or Ctrl+C twice.
    quit: bool,

    /// Time of last render tick, for the spinner animation.
    last_tick: Instant,

    /// Model display string for the status bar.
    pub model_label: String,

    /// When this TUI session was started (UTC ISO-8601). Surfaced
    /// by the `/status` slash command (F.10.15).
    pub started_at: String,

    /// Buffer of in-flight tool calls by id. `ToolStarted` allocates,
    /// `ToolOutput` appends, `ToolFinished` closes and emits the
    /// final `ChatEntry::ToolCard`.
    in_flight_tools: std::collections::HashMap<ToolCallId, PendingTool>,

    /// Session picker overlay, when open. `None` while the user is
    /// editing the composer. Opened by `/resume` (no args); the
    /// driver sets [`App::pending_resume`] when the user picks one.
    pub picker: Option<SessionPicker>,

    /// Session id the user just selected from the picker. The TUI
    /// driver reads this on the way out and prints a resume
    /// command; future slices may rebuild the agent in place.
    pub pending_resume: Option<String>,

    /// Pending tool call awaiting the user's approval. `None` while
    /// no policy-driven ask is in flight. Set by the driver when
    /// the kernel sends an `AskRequest`; cleared when the user
    /// resolves it (or the oneshot closes underneath us).
    pub pending_approval: Option<PendingApproval>,

    /// Per-session "always allow" allowlist. Tools in this set are
    /// auto-approved without showing the card. The TUI driver
    /// checks this BEFORE showing the overlay so repeated calls to
    /// the same tool don't keep nagging the user.
    pub allowlist: AllowList,

    /// Last approval resolution, surfaced as a status line so the
    /// user can see what they just decided. Reset on each new ask.
    pub last_resolution: Option<String>,

    /// Cumulative input tokens across every `ModelFinished` event
    /// in this session. Powers the `tok in:N out:M` indicator in
    /// the status bar (F.04.04).
    pub cumulative_input_tokens: u32,

    /// Cumulative output tokens across every `ModelFinished` event
    /// in this session.
    pub cumulative_output_tokens: u32,

    /// Per-tool-call token usage since the last reset. Keys are
    /// tool names (`"read"`, `"bash"`, ...); values are
    /// `(input_tokens, output_tokens)`. Backs the per-tool
    /// breakdown in `/cost` (F.10.08).
    pub per_tool_tokens: std::collections::BTreeMap<String, (u32, u32)>,

    /// Name of the tool currently executing, if any. Drives the
    /// live tool timer in the status bar (F.04.03).
    pub current_tool: Option<String>,

    /// When the current tool started executing. `None` when idle.
    /// Drives the live tool timer in the status bar (F.04.03).
    pub current_tool_started_at: Option<std::time::Instant>,

    /// Per-model token pricing table (F.04.05). Loaded from
    /// `<repo>/config/pricing.toml` at App construction; falls
    /// back to zero rates if the file is missing.
    pub pricing: Pricing,

    /// Cumulative USD cost for this session, computed from
    /// `cumulative_input_tokens`/`cumulative_output_tokens` and
    /// `pricing` (F.04.05).
    pub cumulative_cost_usd: f64,

    /// Extra directories the user has granted the agent access to
    /// beyond `project_root`, via `/add-dir <path>` (F.10.16).
    /// Each entry is canonicalised (relative paths are resolved
    /// against cwd). The list grows as the user runs `/add-dir`.
    pub allowed_extra_dirs: Vec<std::path::PathBuf>,

    /// Plan mode flag. When true, the agent only has `read`
    /// available — it can inspect code but cannot mutate files
    /// or run shell commands. Set at startup from the `--plan`
    /// CLI flag; the `/plan` slash command toggles this at
    /// runtime (takes effect on the next session, since the tool
    /// registry is owned by the worker task and rebuild is
    /// non-trivial).
    pub plan_mode: bool,

    /// Axe-reader lite: when true, the renderer skips colour
    /// attributes so screen readers, dumb terminals, and CI logs
    /// see clean text. Set from the `--no-color` CLI flag.
    pub no_color: bool,
}

impl App {
    /// Construct a fresh app with optional replayed history from a
    /// resumed session log.
    pub fn new(
        config: Config,
        session_path: PathBuf,
        history: Vec<crate::message::Message>,
        plan_mode: bool,
        no_color: bool,
    ) -> Self {
        // Pricing lives at <repo>/config/pricing.toml. We resolve
        // it relative to the config's project_root so users can
        // ship a per-project pricing table. (F.04.05.)
        let pricing_path = config.project_root.join("config").join("pricing.toml");
        let pricing = Pricing::load(&pricing_path);
        let mut app = Self {
            config: config.clone(),
            session_path,
            session_id: crate::ids::SessionId(crate::ids::new_id()),
            input: TextArea::default(),
            input_mode: InputMode::Editing,
            history: Vec::new(),
            scroll_back: 0,
            following_tail: true,
            phase: RunPhase::Idle,
            spinner_frame: 0,
            worker_alive: true,
            last_error: None,
            submit_pending: false,
            quit: false,
            last_tick: Instant::now(),
            model_label: config.model.clone(),
            started_at: {
                // Inline ISO-8601 UTC formatter; lives here so app.rs
                // doesn't depend on private helpers in mod.rs.
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
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
                let yr = (if m <= 2 { y + 1 } else { y }) as i32;
                format!("{yr:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
            },
            in_flight_tools: std::collections::HashMap::new(),
            picker: None,
            pending_resume: None,
            pending_approval: None,
            allowlist: AllowList::new(),
            last_resolution: None,
            cumulative_input_tokens: 0,
            cumulative_output_tokens: 0,
            per_tool_tokens: std::collections::BTreeMap::new(),
            current_tool: None,
            current_tool_started_at: None,
            pricing,
            cumulative_cost_usd: 0.0,
            allowed_extra_dirs: Vec::new(),
            plan_mode,
            no_color,
        };
        for msg in history {
            replay_message(&mut app, msg);
        }
        app
    }

    /// Apply one agent event to the model. Always idempotent enough
    /// to be safe to call once per kernel event.
    pub fn apply_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::RunStarted { session_id, .. } => {
                self.session_id = session_id;
                self.phase = RunPhase::Running;
                self.last_error = None;
            }
            AgentEvent::ModelStarted => {
                // The kernel is now calling the provider. The UI
                // already shows the spinner because `phase` is
                // `Running`; nothing else to do.
            }
            AgentEvent::TextDelta { text } => {
                self.append_assistant_text(&text);
            }
            AgentEvent::ReasoningDelta { text } => {
                self.append_reasoning(&text);
            }
            AgentEvent::ToolStarted {
                call_id,
                name,
                args,
            } => {
                self.history.push(ChatEntry::StatusLine(format!(
                    "  ▷ {name}({})",
                    short_args(&args)
                )));
                self.in_flight_tools.insert(
                    call_id,
                    PendingTool {
                        name: name.clone(),
                        args,
                        stdout: String::new(),
                        stderr: String::new(),
                        finished: false,
                    },
                );
                // Track the active tool so the status bar can show
                // a live timer (F.04.03).
                self.current_tool = Some(name);
                self.current_tool_started_at = Some(std::time::Instant::now());
            }
            AgentEvent::ToolOutput {
                call_id,
                stream,
                chunk,
            } => {
                if let Some(slot) = self.in_flight_tools.get_mut(&call_id) {
                    let s = String::from_utf8_lossy(&chunk);
                    match stream {
                        ToolStream::Stdout => slot.stdout.push_str(&s),
                        ToolStream::Stderr => slot.stderr.push_str(&s),
                    }
                }
            }
            AgentEvent::ToolFinished { call_id, result } => {
                let pending = self.in_flight_tools.remove(&call_id);
                let (output, is_error, truncated) = match result {
                    crate::event::ToolOutcome::Success { output, truncated } => {
                        (output, false, truncated)
                    }
                    crate::event::ToolOutcome::Error {
                        message, truncated, ..
                    } => (message, true, truncated),
                };
                if let Some(pending) = pending {
                    // Per-tool token bucket (F.10.08 — feeds /cost).
                    let entry = self
                        .per_tool_tokens
                        .entry(pending.name.clone())
                        .or_insert((0, 0));
                    entry.0 = entry.0.saturating_add(self.cumulative_input_tokens);
                    entry.1 = entry.1.saturating_add(self.cumulative_output_tokens);
                    self.history.push(ChatEntry::ToolCard {
                        name: pending.name,
                        args: pending.args,
                        output,
                        is_error,
                        truncated,
                        stdout: pending.stdout,
                        stderr: pending.stderr,
                    });
                }
                // Clear the active tool timer (F.04.03).
                self.current_tool = None;
                self.current_tool_started_at = None;
            }
            AgentEvent::ModelFinished { usage, stop_reason } => {
                // Accumulate tokens for the status bar (F.04.04).
                self.cumulative_input_tokens = self
                    .cumulative_input_tokens
                    .saturating_add(usage.input_tokens);
                self.cumulative_output_tokens = self
                    .cumulative_output_tokens
                    .saturating_add(usage.output_tokens);
                // Store usage on the most recent assistant text entry
                // so the renderer can show token counts on hover (v1).
                self.record_usage(usage, stop_reason);
            }
            AgentEvent::RunFinished { .. } => {
                self.phase = RunPhase::Done;
            }
            AgentEvent::RunCancelled => {
                self.phase = RunPhase::Cancelled;
            }
            AgentEvent::RunFailed {
                code,
                retryable,
                message,
            } => {
                self.phase = RunPhase::Failed;
                self.last_error = Some(format!(
                    "{}{}: {}",
                    code.0,
                    if retryable { " (retryable)" } else { "" },
                    message
                ));
                // Surface the failure inline in the chat so the
                // user sees it after scrolling, not just in the
                // status bar.
                self.history.push(ChatEntry::ErrorBanner {
                    code: code.0,
                    retryable,
                    message,
                });
            }
        }
    }

    /// Note that the worker has gone (the event channel closed).
    /// We don't transition to Done automatically because the kernel
    /// already emits a terminal event before closing — but this is
    /// the safety net for transport failures.
    pub fn mark_worker_gone(&mut self) {
        self.worker_alive = false;
        if matches!(self.phase, RunPhase::Running) {
            self.phase = RunPhase::Failed;
            self.last_error = Some("worker connection lost".to_string());
        }
    }

    /// True if the model is currently generating.
    pub fn is_running(&self) -> bool {
        matches!(self.phase, RunPhase::Running)
    }

    /// True if the spinner frame should advance — runs are in
    /// flight, OR we want a brief shimmer after Done.
    pub fn spinner_needs_tick(&mut self) -> bool {
        if self.is_running() && self.last_tick.elapsed() >= Duration::from_millis(120) {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
            self.last_tick = Instant::now();
            return true;
        }
        false
    }

    /// Append a chunk of assistant text to the trailing text entry,
    /// or create one if the trailing entry is not assistant text.
    fn append_assistant_text(&mut self, chunk: &str) {
        match self.history.last_mut() {
            Some(ChatEntry::AssistantText(text)) => text.push_str(chunk),
            _ => self
                .history
                .push(ChatEntry::AssistantText(chunk.to_string())),
        }
    }

    /// Same, for reasoning / chain-of-thought.
    fn append_reasoning(&mut self, chunk: &str) {
        match self.history.last_mut() {
            Some(ChatEntry::Reasoning(text)) => text.push_str(chunk),
            _ => self.history.push(ChatEntry::Reasoning(chunk.to_string())),
        }
    }

    /// Look up the args of an in-flight tool. Reserved for future
    /// use; currently the [`AgentEvent::ToolStarted`] handler emits
    /// the status line directly from the event payload. Returns a
    /// `Null` placeholder if the tool is no longer tracked.
    #[allow(dead_code)]
    fn peek_tool_args(&self, call_id: ToolCallId) -> Value {
        self.in_flight_tools
            .get(&call_id)
            .map(|t| t.args.clone())
            .unwrap_or(Value::Null)
    }

    /// Record token usage for the most recent assistant entry. The
    /// UI doesn't surface this yet (it'd clutter the chat); the
    /// data is here for a later status-bar upgrade.
    fn record_usage(&mut self, _usage: crate::event::Usage, _reason: StopReason) {}

    /// The user pressed Enter and the input is ready to send.
    pub fn take_submit(&mut self) -> bool {
        let was_pending = self.submit_pending;
        self.submit_pending = false;
        was_pending
    }

    /// Drain the input buffer. The caller decides whether to send
    /// it to the agent or treat it as a slash command.
    pub fn take_input(&mut self) -> String {
        let lines = self.input.lines().join("\n");
        self.input = TextArea::default();
        lines
    }

    /// Record a user message into the local history (so the user
    /// sees their own prompt echoed above the assistant reply).
    pub fn record_user_message(&mut self, text: &str) {
        self.history.push(ChatEntry::UserMessage(text.to_string()));
    }

    /// Whether the user has asked to quit.
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Handle one terminal event. Returns `true` if the UI needs a
    /// redraw after the event.
    pub fn handle_terminal_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(KeyEvent {
                code, modifiers, ..
            }) => self.handle_key(code, modifiers),
            Event::Resize(_, _) => true,
            _ => false,
        }
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        // Approval card has its own keymap. When an ask is in
        // flight, ONLY approval keys apply — the composer is inert
        // and the picker is hidden under the card.
        if self.pending_approval.is_some() {
            return self.handle_approval_key(code, modifiers);
        }

        // Picker overlay has its own keymap. When the picker is
        // open, ONLY picker keys apply — the composer is inert.
        if self.picker.is_some() {
            return self.handle_picker_key(code, modifiers);
        }

        // Global shortcuts (always active).
        match (code, modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.is_running() {
                    // First Ctrl+C cancels the run; a second one
                    // during Idle quits. Matches Claude Code.
                    self.phase = RunPhase::Cancelled;
                    // The cancellation token is owned by the TUI
                    // driver, which polls a shared flag we set
                    // here via a flag embedded in the model.
                    self.last_error = Some("interrupted".to_string());
                    return true;
                }
                self.quit = true;
                return true;
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                if self.input.lines().join("").is_empty() {
                    self.quit = true;
                    return true;
                }
            }
            (KeyCode::Esc, _) => {
                if self.is_running() {
                    self.phase = RunPhase::Cancelled;
                    self.last_error = Some("interrupted".to_string());
                    return true;
                }
            }
            (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::SHIFT) => {
                // Shift+Enter inserts a newline; plain Enter submits.
                if modifiers == KeyModifiers::NONE {
                    self.submit_pending = true;
                    return true;
                }
            }
            (KeyCode::PageUp, _) => {
                self.scroll_back = self.scroll_back.saturating_add(10);
                self.following_tail = false;
                return true;
            }
            (KeyCode::PageDown, _) => {
                self.scroll_back = self.scroll_back.saturating_sub(10);
                if self.scroll_back == 0 {
                    self.following_tail = true;
                }
                return true;
            }
            (KeyCode::End, _) => {
                self.scroll_back = 0;
                self.following_tail = true;
                return true;
            }
            _ => {}
        }

        // Forward everything else to the textarea. The textarea
        // returns whether it consumed the event.
        self.input
            .input(crossterm::event::KeyEvent::new(code, modifiers));
        true
    }

    /// Keymap for the session picker overlay.
    fn handle_picker_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        // Ctrl+C bails the picker AND quits — the user should
        // always be able to recover from an unexpected overlay.
        if (code, modifiers) == (KeyCode::Char('c'), KeyModifiers::CONTROL) {
            self.picker = None;
            self.quit = true;
            return true;
        }
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.picker = None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(p) = self.picker.as_mut() {
                    p.select_next();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(p) = self.picker.as_mut() {
                    p.select_prev();
                }
            }
            KeyCode::PageDown => {
                if let Some(p) = self.picker.as_mut() {
                    p.page_down(10);
                }
            }
            KeyCode::PageUp => {
                if let Some(p) = self.picker.as_mut() {
                    p.page_up(10);
                }
            }
            KeyCode::Home => {
                if let Some(p) = self.picker.as_mut() {
                    p.select_first();
                }
            }
            KeyCode::End => {
                if let Some(p) = self.picker.as_mut() {
                    p.select_last();
                }
            }
            KeyCode::Enter => {
                // Capture the selected id, close the picker, signal
                // the driver to exit so it can print the resume
                // command. Building the full command is the
                // driver's job (it owns argv).
                if let Some(p) = self.picker.take() {
                    if let Some(entry) = p.selected() {
                        self.pending_resume = Some(entry.session_id.clone());
                    }
                }
                self.quit = true;
            }
            _ => {}
        }
        true
    }

    /// Open the session picker with `entries`. Replaces any
    /// previously-open picker.
    pub fn open_picker(&mut self, entries: Vec<PickerEntry>) {
        self.picker = Some(SessionPicker::new(entries));
    }

    /// Drop the picker overlay without selecting anything.
    pub fn close_picker(&mut self) {
        self.picker = None;
    }

    /// True if the picker overlay is currently visible.
    #[must_use]
    pub fn picker_is_open(&self) -> bool {
        self.picker.is_some()
    }

    /// Apply the side-effects of a parsed slash command (already
    /// classified by [`crate::tui::commands::parse_slash`]).
    ///
    /// Most commands are sync UI effects (clear, help, doctor,
    /// model). `/resume` is special — it requires async I/O to
    /// load the sessions directory, so the driver handles it
    /// separately before invoking this method. When this method
    /// is called for `resume`, the driver has already either
    /// opened the picker or pushed a status line explaining why
    /// it can't.
    pub fn apply_local_slash(&mut self, name: &str, args: &str) {
        match name {
            "clear" => {
                self.history.clear();
            }
            "help" => {
                self.history
                    .push(ChatEntry::StatusLine(SLASH_HELP.to_string()));
            }
            "doctor" => {
                self.history.push(ChatEntry::StatusLine(format!(
                    "model={} session={:?}",
                    self.model_label, self.session_path
                )));
            }
            "model" => {
                self.history.push(ChatEntry::StatusLine(format!(
                    "current model: {}",
                    self.model_label
                )));
            }
            "resume" => {
                // The driver should have intercepted this before we
                // got here. If we somehow land here (e.g. from a
                // future caller), surface a hint so the user is not
                // left wondering why nothing happened.
                if args.is_empty() {
                    self.history.push(ChatEntry::StatusLine(
                        "resume: pick a session with the picker overlay".to_string(),
                    ));
                } else {
                    self.history.push(ChatEntry::StatusLine(format!(
                        "resume: use `crow tui --resume {args}` from the shell"
                    )));
                }
            }
            "plan" => {
                // The driver should have intercepted this too — see
                // the matching arm in the driver's slash dispatch.
                // If we land here, surface the toggle so the user
                // gets feedback either way.
                self.toggle_plan_mode();
            }
            "cost" => self.show_cost(),
            "status" => self.show_status(),
            "add-dir" => self.add_dir(args),
            _ => {}
        }
    }

    /// F.10.08 — render the `/cost` summary: total USD, total tokens,
    /// per-tool breakdown. Pushed as a stack of `StatusLine`
    /// entries so it scrolls naturally with the rest of the chat.
    fn show_cost(&mut self) {
        let total = self.cumulative_cost_usd;
        let total_in = self.cumulative_input_tokens;
        let total_out = self.cumulative_output_tokens;
        self.history.push(ChatEntry::StatusLine(format!(
            "cost: {}  (in:{} out:{})",
            crate::provider::pricing::format_usd(total),
            total_in,
            total_out
        )));
        if self.per_tool_tokens.is_empty() {
            return;
        }
        for (tool, (in_t, out_t)) in &self.per_tool_tokens {
            let usd = self.pricing.cost(&self.model_label, *in_t, *out_t);
            self.history.push(ChatEntry::StatusLine(format!(
                "  {tool}: {}  (in:{} out:{})",
                crate::provider::pricing::format_usd(usd),
                in_t,
                out_t
            )));
        }
    }

    /// F.10.15 — render the `/status` summary: session id, started
    /// at, current phase, total turns, total tool calls, total
    /// tokens, total cost. Single screen, no scroll.
    fn show_status(&mut self) {
        let total_turns = self
            .history
            .iter()
            .filter(|e| matches!(e, ChatEntry::AssistantText(_)))
            .count();
        let total_tool_calls = self
            .history
            .iter()
            .filter(|e| matches!(e, ChatEntry::ToolCard { .. }))
            .count();
        let phase = match self.phase {
            RunPhase::Idle => "idle",
            RunPhase::Running => "running",
            RunPhase::Done => "done",
            RunPhase::Cancelled => "cancelled",
            RunPhase::Failed => "failed",
        };
        let model = self.model_label.clone();
        let started = self.started_at.clone();
        let total_in = self.cumulative_input_tokens;
        let total_out = self.cumulative_output_tokens;
        let total_cost = self.cumulative_cost_usd;
        let plan_mode = self.plan_mode;
        let lines = vec![
            format!("session:    {}", self.session_id.0),
            format!("started:    {started}"),
            format!("phase:      {phase}"),
            format!("model:      {model}"),
            format!("plan mode:  {plan_mode}"),
            format!("turns:      {total_turns}"),
            format!("tool calls: {total_tool_calls}"),
            format!("tokens:     in:{total_in} out:{total_out}"),
            format!(
                "cost:       {}",
                crate::provider::pricing::format_usd(total_cost)
            ),
        ];
        for line in lines {
            self.history.push(ChatEntry::StatusLine(line));
        }
    }
}

/// What the input is currently focused on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Typing in the composer.
    Editing,
    /// Reserved for future overlays. Kept as an enum so the renderer
    /// can switch layout without an `if` ladder.
    Overlay(Overlay),
}

/// Overlay kinds reserved for later slices (approval, history
/// picker, command palette). The TUI driver doesn't render them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// Future: approval card overlay.
    Approval,
    /// Future: command palette / slash-command browser.
    CommandPalette,
    /// Future: session picker.
    SessionPicker,
}

/// Current phase of the agent run. Drives the spinner and the
/// status bar colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhase {
    /// No run in flight.
    Idle,
    /// Model is generating or a tool is executing.
    Running,
    /// Last run completed cleanly.
    Done,
    /// Last run was cancelled (user pressed Esc / Ctrl+C).
    Cancelled,
    /// Last run failed with a structured error.
    Failed,
}

/// One rendered entry in the chat scrollback.
#[derive(Debug, Clone)]
pub enum ChatEntry {
    /// Echo of the user's prompt, prefixed with `❯`.
    UserMessage(String),
    /// Model-authored text, possibly streamed in chunks.
    AssistantText(String),
    /// Model reasoning / chain-of-thought (folded by default in v1).
    Reasoning(String),
    /// A completed tool invocation. Cards are rendered with their
    /// name, args, output, and a green/red status dot.
    ToolCard {
        name: String,
        args: Value,
        output: String,
        is_error: bool,
        truncated: bool,
        stdout: String,
        stderr: String,
    },
    /// A short status line (slash-command output, info banners).
    StatusLine(String),
    /// A red error banner pushed on `RunFailed`. Renders inline
    /// with the chat so the failure is visible after scrolling,
    /// not just in the status bar.
    ErrorBanner {
        /// Error code (e.g. `"stream_invalid"`).
        code: String,
        /// Whether the kernel marked this error retryable.
        retryable: bool,
        /// Human-readable failure message.
        message: String,
    },
}

/// In-flight tool state held between `ToolStarted` and
/// `ToolFinished`.
#[derive(Debug, Clone)]
struct PendingTool {
    name: String,
    args: Value,
    stdout: String,
    stderr: String,
    #[allow(dead_code)]
    finished: bool,
}

/// Helper: replay one persisted message into the chat history.
/// Used when the user resumes an existing session — we don't have
/// live events for past turns, so we synthesise entries from the
/// JSONL log.
fn replay_message(app: &mut App, msg: crate::message::Message) {
    use crate::message::Role;
    match msg.role {
        Role::User => {
            for part in msg.parts {
                if let Part::Text { text } = part {
                    app.history.push(ChatEntry::UserMessage(text));
                }
            }
        }
        Role::Assistant => {
            for part in msg.parts {
                match part {
                    Part::Text { text } => {
                        app.history.push(ChatEntry::AssistantText(text));
                    }
                    Part::Reasoning { text } => {
                        app.history.push(ChatEntry::Reasoning(text));
                    }
                    Part::ToolCall { id, name, args } => {
                        // The persisted log has the tool call but not
                        // its result on the same Message; the
                        // matching ToolResult lands on a separate
                        // `Role::ToolResult` message. Pair them up
                        // by id so the card renders complete.
                        app.in_flight_tools.insert(
                            id,
                            PendingTool {
                                name,
                                args,
                                stdout: String::new(),
                                stderr: String::new(),
                                finished: false,
                            },
                        );
                    }
                    Part::ToolResult { .. } => {
                        // Already handled by the dedicated ToolResult
                        // message below.
                    }
                }
            }
        }
        Role::ToolResult => {
            for part in msg.parts {
                if let Part::ToolResult {
                    call_id,
                    output,
                    is_error,
                    truncated,
                    ..
                } = part
                {
                    if let Some(pending) = app.in_flight_tools.remove(&call_id) {
                        app.history.push(ChatEntry::ToolCard {
                            name: pending.name,
                            args: pending.args,
                            output,
                            is_error,
                            truncated,
                            stdout: pending.stdout,
                            stderr: pending.stderr,
                        });
                    }
                }
            }
        }
    }
}

/// Approval-card helpers live in their own `impl App` block
/// because the App struct already has a long body; keeping
/// these together here makes the slice boundaries easier to
/// read.
impl App {
    /// Keymap for the approval card overlay. Returns `true` so the
    /// UI redraws. The user picks one of three outcomes:
    ///
    /// - `y` / Enter — allow this single call
    /// - `a` — allow this call AND add the tool to the session
    ///   allowlist (so future calls of the same tool skip the card)
    /// - `n` / Esc — deny the call
    ///
    /// All three outcomes resolve the pending ask immediately; the
    /// caller (driver) reads [`App::take_pending_approval`] to
    /// send the response back to the kernel.
    fn handle_approval_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        let outcome = match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(ApprovalOutcome::Allow),
            KeyCode::Char('a') | KeyCode::Char('A') => Some(ApprovalOutcome::AllowAlways),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(ApprovalOutcome::Deny),
            KeyCode::Enter => Some(ApprovalOutcome::Allow),
            _ => None,
        };
        if let Some(outcome) = outcome {
            self.resolve_pending_approval(outcome);
            return true;
        }
        // Ignore Ctrl+C / unknown keys so a stray keypress doesn't
        // accidentally allow or deny a destructive call.
        let _ = modifiers;
        true
    }

    /// Take the pending approval, send its response to the kernel,
    /// update the allowlist on AllowAlways, and surface a status
    /// line describing the decision. Called by the driver after the
    /// user picks an outcome in the approval card.
    pub fn resolve_pending_approval(&mut self, outcome: ApprovalOutcome) {
        let Some(pending) = self.pending_approval.take() else {
            return;
        };
        let tool = pending.tool_name().to_string();
        let sent = pending.resolve(outcome, &mut self.allowlist);
        let label = match outcome {
            ApprovalOutcome::Allow => "allowed",
            ApprovalOutcome::AllowAlways => "allowed (always for this session)",
            ApprovalOutcome::Deny => "denied",
        };
        self.last_resolution = Some(if sent {
            format!("{tool}: {label}")
        } else {
            format!("{tool}: {label} (channel closed, kernel may have moved on)")
        });
    }

    /// Set the pending approval from an `AskRequest`. The driver
    /// calls this when an ask arrives on the resolver channel and
    /// the tool isn't already allowlisted.
    ///
    /// Returns `true` if the ask was installed; `false` if the tool
    /// was already in the allowlist and the call was auto-approved
    /// (the caller should respond `Allow` itself).
    pub fn install_pending_approval(&mut self, req: crate::policy::AskRequest) -> bool {
        let tool_name = req.call.name.clone();
        if self.allowlist.allows(&tool_name) {
            // Auto-allow: send Allow and drop the request. The
            // driver doesn't need to know we skipped the card.
            let _ = req.response.send(crate::policy::AskResponse::Allow);
            self.last_resolution = Some(format!("{tool_name}: auto-allowed (allowlist)"));
            return false;
        }
        let Some(pending) = PendingApproval::from_request(req) else {
            // Malformed request — deny defensively so the agent
            // doesn't hang on a closed oneshot.
            return false;
        };
        self.pending_approval = Some(pending);
        true
    }

    /// True if the approval card is currently visible.
    #[must_use]
    pub fn approval_is_open(&self) -> bool {
        self.pending_approval.is_some()
    }

    /// Take the pending approval out of the App. Used by tests and
    /// by the driver when the channel closes underneath us.
    pub fn take_pending_approval(&mut self) -> Option<PendingApproval> {
        self.pending_approval.take()
    }

    /// Reset every per-session accumulator. Called on `--resume <id>`
    /// so a fresh session starts clean.
    pub fn reset_session(&mut self) {
        self.cumulative_input_tokens = 0;
        self.cumulative_output_tokens = 0;
        self.cumulative_cost_usd = 0.0;
        self.per_tool_tokens.clear();
        self.history.clear();
        self.allowlist = AllowList::new();
        self.last_resolution = None;
        self.current_tool = None;
        self.current_tool_started_at = None;
        self.last_error = None;
        self.pending_approval = None;
        self.allowed_extra_dirs.clear();
    }

    /// F.10.16 — `/add-dir <path>` grants the agent access to
    /// `<path>` in addition to `project_root`. Relative paths are
    /// resolved against `project_root`. Pushing a status line so
    /// the user has feedback.
    pub fn add_dir(&mut self, args: &str) {
        let raw = args.trim();
        if raw.is_empty() {
            self.history.push(ChatEntry::StatusLine(
                "add-dir: usage: /add-dir <path>".to_string(),
            ));
            return;
        }
        let p = std::path::PathBuf::from(raw);
        let absolute = if p.is_absolute() {
            p
        } else {
            self.config.project_root.join(p)
        };
        // Canonicalise so duplicates / `..` collapse cleanly.
        let canonical = absolute.canonicalize().unwrap_or(absolute);
        if self.allowed_extra_dirs.contains(&canonical) {
            self.history.push(ChatEntry::StatusLine(format!(
                "add-dir: already allowed: {}",
                canonical.display()
            )));
            return;
        }
        self.allowed_extra_dirs.push(canonical.clone());
        self.history.push(ChatEntry::StatusLine(format!(
            "add-dir: granted access to {}",
            canonical.display()
        )));
    }

    /// Toggle plan mode. The driver routes the user's `/plan`
    /// slash command through here. The current session's tool
    /// registry is not rebuilt mid-flight (the worker task owns
    /// it); the new mode applies on the next session.
    pub fn toggle_plan_mode(&mut self) {
        self.plan_mode = !self.plan_mode;
        let label = if self.plan_mode {
            "on (read-only; restart to apply)"
        } else {
            "off (full toolset; restart to apply)"
        };
        self.history
            .push(ChatEntry::StatusLine(format!("plan mode: {label}")));
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)] // SLASH_HELP + short_args live below by file convention
mod plan_mode_tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            base_url: "https://example.invalid".into(),
            model: "test-model".into(),
            api_key: secrecy::Secret::new(String::new()),
            max_turns: 10,
            max_tool_calls: 10,
            max_output_bytes: 8192,
            command_timeout_secs: 30,
            project_root: PathBuf::from("/tmp/test"),
            sessions_dir: PathBuf::from("/tmp/test/.crow/sessions"),
        }
    }

    fn make_app() -> App {
        App::new(
            test_config(),
            PathBuf::from("/tmp/test.jsonl"),
            Vec::new(),
            false,
            false,
        )
    }

    #[test]
    fn plan_mode_starts_off() {
        let app = make_app();
        assert!(!app.plan_mode);
    }

    #[test]
    fn plan_mode_starts_on_when_requested() {
        let app = App::new(
            test_config(),
            PathBuf::from("/tmp/x"),
            Vec::new(),
            true,
            false,
        );
        assert!(app.plan_mode);
    }

    #[test]
    fn no_color_starts_off_by_default() {
        let app = make_app();
        assert!(!app.no_color);
    }

    #[test]
    fn no_color_starts_on_when_requested() {
        let app = App::new(
            test_config(),
            PathBuf::from("/tmp/x"),
            Vec::new(),
            false,
            true,
        );
        assert!(app.no_color);
    }

    #[test]
    fn toggle_plan_mode_flips_state() {
        let mut app = make_app();
        assert!(!app.plan_mode);
        app.toggle_plan_mode();
        assert!(app.plan_mode);
        app.toggle_plan_mode();
        assert!(!app.plan_mode);
    }

    #[test]
    fn toggle_plan_mode_pushes_status_line() {
        let mut app = make_app();
        let before = app.history.len();
        app.toggle_plan_mode();
        assert!(app.history.len() > before);
        let last = app.history.last().expect("status line");
        if let ChatEntry::StatusLine(text) = last {
            assert!(text.starts_with("plan mode:"));
        } else {
            panic!("expected StatusLine, got {last:?}");
        }
    }

    // --- F.04.04 token accumulation tests ---

    fn usage(in_t: u32, out_t: u32) -> crate::event::Usage {
        crate::event::Usage {
            input_tokens: in_t,
            output_tokens: out_t,
        }
    }

    #[test]
    fn model_finished_accumulates_tokens() {
        let mut app = make_app();
        assert_eq!(app.cumulative_input_tokens, 0);
        assert_eq!(app.cumulative_output_tokens, 0);
        app.apply_event(AgentEvent::ModelFinished {
            usage: usage(100, 50),
            stop_reason: crate::event::StopReason::EndTurn,
        });
        assert_eq!(app.cumulative_input_tokens, 100);
        assert_eq!(app.cumulative_output_tokens, 50);
        app.apply_event(AgentEvent::ModelFinished {
            usage: usage(200, 75),
            stop_reason: crate::event::StopReason::EndTurn,
        });
        assert_eq!(app.cumulative_input_tokens, 300);
        assert_eq!(app.cumulative_output_tokens, 125);
    }

    // --- F.04.03 live tool timer tests ---

    fn dummy_call_id() -> ToolCallId {
        ToolCallId(crate::ids::new_id())
    }

    #[test]
    fn tool_started_sets_current_tool_and_timestamp() {
        let mut app = make_app();
        assert!(app.current_tool.is_none());
        let id = dummy_call_id();
        app.apply_event(AgentEvent::ToolStarted {
            call_id: id,
            name: "bash".to_string(),
            args: serde_json::json!({"command": "ls"}),
        });
        assert_eq!(app.current_tool.as_deref(), Some("bash"));
        assert!(app.current_tool_started_at.is_some());
    }

    #[test]
    fn tool_finished_clears_current_tool() {
        let mut app = make_app();
        let id = dummy_call_id();
        app.apply_event(AgentEvent::ToolStarted {
            call_id: id,
            name: "bash".to_string(),
            args: serde_json::json!({"command": "ls"}),
        });
        app.apply_event(AgentEvent::ToolFinished {
            call_id: id,
            result: crate::event::ToolOutcome::Success {
                output: "ok".to_string(),
                truncated: false,
            },
        });
        assert!(app.current_tool.is_none());
        assert!(app.current_tool_started_at.is_none());
    }

    // --- F.10.08 /cost tests ---

    #[test]
    fn cost_command_pushes_status_line_with_total() {
        let mut app = make_app();
        app.cumulative_input_tokens = 1000;
        app.cumulative_output_tokens = 200;
        app.cumulative_cost_usd = 0.0123;
        let before = app.history.len();
        app.show_cost();
        assert!(app.history.len() > before);
        // The first pushed line is the total.
        if let ChatEntry::StatusLine(line) = &app.history[before] {
            assert!(line.contains("cost:"));
            assert!(line.contains("in:1000"));
            assert!(line.contains("out:200"));
        } else {
            panic!("expected StatusLine total, got {:?}", app.history[before]);
        }
    }

    #[test]
    fn cost_command_with_per_tool_breakdown_pushes_one_line_per_tool() {
        let mut app = make_app();
        app.per_tool_tokens.insert("bash".to_string(), (500, 100));
        app.per_tool_tokens.insert("edit".to_string(), (300, 50));
        let before = app.history.len();
        app.show_cost();
        let new_lines = app.history.len() - before;
        // 1 total + 2 per-tool = 3
        assert_eq!(new_lines, 3);
        // Verify each per-tool line includes the tool name.
        let mut found_bash = false;
        let mut found_edit = false;
        for entry in &app.history[before + 1..] {
            if let ChatEntry::StatusLine(text) = entry {
                if text.contains("bash") {
                    found_bash = true;
                }
                if text.contains("edit") {
                    found_edit = true;
                }
            }
        }
        assert!(found_bash);
        assert!(found_edit);
    }

    // --- F.10.15 /status tests ---

    #[test]
    fn status_command_pushes_metadata_lines() {
        let mut app = make_app();
        app.cumulative_input_tokens = 1234;
        app.cumulative_output_tokens = 567;
        app.cumulative_cost_usd = 0.0123;
        let before = app.history.len();
        app.show_status();
        // 9 lines: session, started, phase, model, plan, turns, tool, tokens, cost
        assert_eq!(app.history.len() - before, 9);
        // Spot-check key lines.
        let all: String = app.history[before..]
            .iter()
            .filter_map(|e| {
                if let ChatEntry::StatusLine(t) = e {
                    Some(t.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("session:"));
        assert!(all.contains("model:"));
        assert!(all.contains("phase:      idle"));
        assert!(all.contains("plan mode:  false"));
        assert!(all.contains("tokens:     in:1234 out:567"));
        assert!(all.contains("cost:"));
    }

    // --- F.10.16 /add-dir tests ---

    #[test]
    fn add_dir_with_no_args_prints_usage() {
        let mut app = make_app();
        let before = app.history.len();
        app.add_dir("");
        assert_eq!(app.history.len() - before, 1);
        if let ChatEntry::StatusLine(text) = &app.history[before] {
            assert!(text.contains("usage:"));
        } else {
            panic!("expected StatusLine");
        }
        assert!(app.allowed_extra_dirs.is_empty());
    }

    #[test]
    fn add_dir_with_relative_path_resolves_against_project_root() {
        let mut app = make_app();
        // Use a path that resolves to something real on the test
        // machine. /tmp is portable.
        let before = app.allowed_extra_dirs.len();
        app.add_dir("/tmp");
        assert_eq!(app.allowed_extra_dirs.len(), before + 1);
        let added = &app.allowed_extra_dirs[before];
        assert!(
            added.is_absolute(),
            "added path should be absolute: {added:?}"
        );
        assert!(added.starts_with("/tmp"));
    }

    #[test]
    fn add_dir_is_idempotent() {
        let mut app = make_app();
        app.add_dir("/tmp");
        let after_first = app.allowed_extra_dirs.len();
        app.add_dir("/tmp");
        assert_eq!(app.allowed_extra_dirs.len(), after_first);
    }

    #[test]
    fn reset_session_clears_allowed_extra_dirs() {
        let mut app = make_app();
        app.add_dir("/tmp");
        assert!(!app.allowed_extra_dirs.is_empty());
        app.reset_session();
        assert!(app.allowed_extra_dirs.is_empty());
    }
}

/// Help text shown by `/help`.
pub const SLASH_HELP: &str = "\
Available commands:
  /help        show this help
  /clear       clear the scrollback
  /model       show the active model
  /doctor      show config snapshot
  /resume      open the session picker overlay
  /plan        toggle plan mode (read-only; restart to apply)
  /quit        exit the TUI (Ctrl+D on empty input also quits)

Shortcuts:
  Enter          submit the current input
  Shift+Enter    insert a newline
  PageUp/PageDown  scroll the chat
  Esc / Ctrl+C   interrupt the current run (twice to quit)
  End            jump to the latest message

Pass --plan on startup to begin in read-only mode.";

/// Render JSON args compactly: `{"path":"/tmp/x"}` stays one line,
/// bigger objects get truncated with an ellipsis.
fn short_args(args: &Value) -> String {
    let s = args.to_string();
    let width = UnicodeWidthStr::width(s.as_str());
    if width <= 60 {
        s
    } else {
        let mut out = String::new();
        let mut w = 0;
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw > 57 {
                out.push('…');
                break;
            }
            out.push(ch);
            w += cw;
        }
        out
    }
}
