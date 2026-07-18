//! Pure rendering for the TUI.
//!
//! [`draw`] is the only entry point: it takes the current [`App`]
//! model and renders the entire frame. No state mutation happens
//! here — the model is the single source of truth and the renderer
//! is a pure projection of it.
//!
//! Layout (top to bottom):
//!
//! ```text
//! ┌─ header ──────────────────────────────────────────────┐
//! │  crow  ▸  <model>                                      │
//! ├─ chat scrollback ──────────────────────────────────────┤
//! │   ❯ your last prompt here                              │
//! │   assistant answer streaming…                          │
//! │   ▷ bash({"cmd": "ls"})                                │
//! │   ─ result ─                                           │
//! │   file1 file2 file3                                    │
//! ├─ input ────────────────────────────────────────────────┤
//! │  > type your message here█                             │
//! ├─ status bar ───────────────────────────────────────────┤
//! │  ◐ running…   session 01HF…   /help for commands       │
//! └────────────────────────────────────────────────────────┘
//! ```

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::app::{App, ChatEntry, RunPhase};

/// Spinner frames used while the agent is running. 8-frame braille
/// pattern — gentle motion without strobing.
const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];

/// Render one frame.
pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header (top + bottom border only)
            Constraint::Min(5),    // chat scrollback
            Constraint::Length(5), // input composer
            Constraint::Length(1), // status bar
        ])
        .split(area);

    draw_header(frame, chunks[0], app);
    draw_chat(frame, chunks[1], app);
    draw_input(frame, chunks[2], app);
    draw_status(frame, chunks[3], app);
}

/// Top header: model name, session path tail. Single-line.
fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let title = Line::from(vec![
        Span::styled(
            " crow ",
            Style::default().bg(Color::Rgb(40, 80, 60)).fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled(
            format!("model: {}", app.model_label),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled(
            format!("session: {}", short_path(&app.session_path)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));
    let p = Paragraph::new(title).block(block);
    frame.render_widget(p, area);
}

/// The chat scrollback. We project each [`ChatEntry`] into one or
/// more `Line`s, then style them. Markdown rendering for assistant
/// text is done in [`super::markdown`]; tool cards are styled here.
fn draw_chat(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let lines = render_history(app);
    // Vertical scroll: ratatui's Paragraph understands a `scroll`
    // offset (lines from the top). We anchor to the bottom by
    // default, and let the user scroll up with PageUp.
    let total = lines.len();
    let viewport = area.height as usize;
    let scroll = compute_scroll(total, viewport, app.scroll_back as usize);

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));
    frame.render_widget(p, area);
}

/// Project the chat history into styled lines.
fn render_history(app: &App) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    for entry in &app.history {
        match entry {
            ChatEntry::UserMessage(text) => {
                out.push(Line::from(vec![
                    Span::styled(" ❯ ", Style::default().fg(Color::Green)),
                    Span::styled(text.clone(), Style::default().fg(Color::White)),
                ]));
                out.push(Line::raw(""));
            }
            ChatEntry::AssistantText(text) => {
                // For v1 we render plain text. Markdown styling is a
                // small follow-up; the structural shell is what
                // matters today.
                for line in text.split('\n') {
                    out.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::White),
                    )));
                }
                out.push(Line::raw(""));
            }
            ChatEntry::Reasoning(text) => {
                out.push(Line::from(Span::styled(
                    format!("  ⌥ {text}"),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
            ChatEntry::ToolCard {
                name,
                args,
                output,
                is_error,
                truncated,
                stdout,
                stderr,
            } => {
                let renderer = super::tools::ToolRenderer::from_name(name);
                out.extend(
                    renderer.render(name, args, output, *is_error, *truncated, stdout, stderr),
                );
            }
            ChatEntry::StatusLine(text) => {
                out.push(Line::from(Span::styled(
                    format!(" {text}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    out
}

/// The composer. A boxed textarea with a green `❯` gutter.
fn draw_input(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    let prompt = if app.is_running() {
        Span::styled(" ⏳ ", Style::default().fg(Color::Yellow))
    } else {
        Span::styled(" ❯ ", Style::default().fg(Color::Green))
    };
    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(block.inner(area))[1];
    frame.render_widget(block, area);
    // Render the prompt glyph manually in the gutter cell.
    let gutter = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area)[0];
    frame.render_widget(Paragraph::new(Line::from(prompt)), gutter);
    frame.render_widget(&app.input, inner);
}

/// Bottom status bar: spinner + run phase + last error + hint.
fn draw_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let phase_label = match app.phase {
        RunPhase::Idle => "idle",
        RunPhase::Running => {
            let s = SPINNER[app.spinner_frame % SPINNER.len()];
            return draw_status_line(
                frame,
                area,
                vec![
                    Span::styled(format!(" {s} "), Style::default().fg(Color::Cyan)),
                    Span::raw("running…"),
                    Span::raw("    "),
                    Span::styled(
                        "/help for commands  ·  Esc to interrupt",
                        Style::default().fg(Color::DarkGray),
                    ),
                ],
            );
        }
        RunPhase::Done => "done",
        RunPhase::Cancelled => "cancelled",
        RunPhase::Failed => "failed",
    };
    let mut spans = vec![Span::styled(
        format!(" {phase_label} "),
        Style::default().fg(color_for_phase(app.phase)),
    )];
    if let Some(err) = &app.last_error {
        spans.push(Span::styled(err.clone(), Style::default().fg(Color::Red)));
    }
    spans.push(Span::raw("    "));
    spans.push(Span::styled(
        format!("session {}", short_session_id(&app.session_id)),
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::raw("    "));
    spans.push(Span::styled(
        "/help for commands",
        Style::default().fg(Color::DarkGray),
    ));
    draw_status_line(frame, area, spans);
}

/// Tiny helper to push a one-line `Line` into the status bar area.
fn draw_status_line(frame: &mut Frame<'_>, area: Rect, spans: Vec<Span<'static>>) {
    let line = Line::from(spans);
    let p = Paragraph::new(line).style(Style::default().bg(Color::Rgb(20, 20, 20)));
    frame.render_widget(p, area);
}

fn color_for_phase(phase: RunPhase) -> Color {
    match phase {
        RunPhase::Idle => Color::DarkGray,
        RunPhase::Running => Color::Cyan,
        RunPhase::Done => Color::Green,
        RunPhase::Cancelled => Color::Yellow,
        RunPhase::Failed => Color::Red,
    }
}

/// Map `(scroll_back, total, viewport)` to the absolute line index
/// to start the Paragraph at. `scroll_back == 0` means "follow
/// tail" — scroll so the last `viewport` lines are visible.
fn compute_scroll(total: usize, viewport: usize, scroll_back: usize) -> usize {
    if total <= viewport {
        return 0;
    }
    let max = total.saturating_sub(viewport);
    // Following tail: anchor the bottom.
    if scroll_back == 0 {
        return max;
    }
    max.saturating_sub(scroll_back).min(max)
}

/// Show only the tail of an absolute path so it fits the header.
fn short_path(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    let max = 48;
    if s.len() <= max {
        s
    } else {
        format!("…{}", &s[s.len() - (max - 1)..])
    }
}

/// Show only the first 8 chars of a session id (ULIDs are 26 chars;
/// 8 is plenty for collision-free identification in the status bar).
fn short_session_id(id: &crate::ids::SessionId) -> String {
    id.0.to_string().chars().take(8).collect()
}
