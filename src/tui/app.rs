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
}

impl App {
    /// Construct a fresh app with optional replayed history from a
    /// resumed session log.
    pub fn new(
        config: Config,
        session_path: PathBuf,
        history: Vec<crate::message::Message>,
    ) -> Self {
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
            in_flight_tools: std::collections::HashMap::new(),
            picker: None,
            pending_resume: None,
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
                        name,
                        args,
                        stdout: String::new(),
                        stderr: String::new(),
                        finished: false,
                    },
                );
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
            }
            AgentEvent::ModelFinished { usage, stop_reason } => {
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
            _ => {}
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

/// Help text shown by `/help`.
pub const SLASH_HELP: &str = "\
Available commands:
  /help        show this help
  /clear       clear the scrollback
  /model       show the active model
  /doctor      show config snapshot
  /resume <id> resume an existing session (handled by CLI flag for now)
  /quit        exit the TUI (Ctrl+D on empty input also quits)

Shortcuts:
  Enter          submit the current input
  Shift+Enter    insert a newline
  PageUp/PageDown  scroll the chat
  Esc / Ctrl+C   interrupt the current run (twice to quit)
  End            jump to the latest message";

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
