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

/// Strip foreground/background colour from a style. Used when the
/// user passes `--no-color` so screen readers, dumb terminals, and
/// CI logs see clean text without ANSI escapes. Modifier (bold,
/// italic) is preserved so the structural hierarchy stays
/// readable in plain mode.
fn strip_color(style: Style) -> Style {
    Style::default()
        .add_modifier(style.add_modifier)
        .remove_modifier(style.sub_modifier)
}

/// Apply a style under the `no_color` axe-reader setting: when
/// `no_color` is on, every coloured style is reduced to a
/// colourless one with the same modifiers.
fn apply(style: Style, no_color: bool) -> Style {
    if no_color {
        strip_color(style)
    } else {
        style
    }
}

/// Drop foreground / background colour from every span in a line.
/// Used at draw time so the renderer doesn't need to thread
/// `no_color` through every `Span::styled` call.
fn strip_line_colors(line: Line<'static>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|s| Span::styled(s.content, strip_color(s.style)))
            .collect::<Vec<_>>(),
    )
}

/// Strip colours from a slice of lines in place. Cheap O(n) pass
/// over the line count; we only do this once per frame.
fn strip_lines(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    lines.into_iter().map(strip_line_colors).collect()
}

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

    // Overlays paint last so they sit on top of the regular layout.
    if app.approval_is_open() {
        draw_approval_card(frame, area, app);
    } else if app.picker_is_open() {
        draw_session_picker(frame, area, app);
    }
}

/// Top header: model name, plan-mode badge, session path tail.
fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(
            " crow ",
            apply(
                Style::default().bg(Color::Rgb(40, 80, 60)).fg(Color::White),
                app.no_color,
            ),
        ),
        Span::raw("  "),
        Span::styled(
            format!("model: {}", app.model_label),
            apply(Style::default().fg(Color::Cyan), app.no_color),
        ),
    ];
    if app.plan_mode {
        // Bright-yellow badge so the user can see at a glance that
        // they're in the read-only sandbox.
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " PLAN ",
            apply(
                Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                app.no_color,
            ),
        ));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        format!("session: {}", short_path(&app.session_path)),
        apply(Style::default().fg(Color::DarkGray), app.no_color),
    ));
    let title = Line::from(spans);
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
    let mut lines = render_history(app);
    if app.no_color {
        lines = strip_lines(lines);
    }
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
                // Markdown rendering: bold/italic/inline-code/fenced
                // code/lists come through our `markdown` module.
                // Plain text falls through unchanged.
                out.extend(super::markdown::render(text));
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
            ChatEntry::ErrorBanner {
                code,
                retryable,
                message,
            } => {
                // Red banner: visible after scrolling, easy to find
                // when reviewing a failed run.
                let retry = if *retryable { " (retryable)" } else { "" };
                out.push(Line::from(Span::styled(
                    format!(" ✗ {code}{retry}: {message} "),
                    Style::default()
                        .bg(Color::Red)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )));
                out.push(Line::raw(""));
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
    // F.04.03 — live tool timer.
    if let (Some(name), Some(started)) = (&app.current_tool, app.current_tool_started_at) {
        let elapsed = started.elapsed().as_secs();
        spans.push(Span::raw("    "));
        spans.push(Span::styled(
            format!("{name} {elapsed}s"),
            Style::default().fg(Color::Yellow),
        ));
    }
    // F.04.04 — cumulative token counts.
    spans.push(Span::raw("    "));
    spans.push(Span::styled(
        format!(
            "tok in:{} out:{}",
            app.cumulative_input_tokens, app.cumulative_output_tokens
        ),
        Style::default().fg(Color::DarkGray),
    ));
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

/// Render the session picker overlay as a centered modal.
fn draw_session_picker(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let picker = match app.picker.as_mut() {
        Some(p) => p,
        None => return,
    };

    // Modal sizing: ~70% width, up to ~60% height, minimum 40x12 so
    // the title and footer always fit.
    let popup_w = (area.width as u32 * 7 / 10).max(40) as u16;
    let popup_h = (area.height as u32 * 6 / 10).max(12) as u16;
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );

    let block = Block::default()
        .title(" Resume a session ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header line
            Constraint::Min(1),    // session list
            Constraint::Length(1), // footer / hints
        ])
        .split(inner);

    // Header: count of sessions.
    let header_line = Line::from(vec![
        Span::styled(
            format!(" {} session(s) ", picker.len()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("    "),
        Span::styled(
            "↑/↓ move · Enter select · Esc cancel",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(header_line), rows[0]);

    // Body: list of sessions, highlight the selected row, scroll
    // so the highlight is in view.
    let viewport = rows[1].height as usize;
    picker.ensure_visible(viewport);
    let scroll = picker.scroll();
    let selected = picker.selected_index();
    let mut body: Vec<Line<'static>> = Vec::new();
    let end = (scroll + viewport).min(picker.len());
    for (row_index, entry_index) in (scroll..end).enumerate() {
        let Some(entry) = picker.get(entry_index) else {
            continue;
        };
        let is_selected = entry_index == selected;
        let marker = if is_selected { " ▸ " } else { "   " };
        let marker_style = if is_selected {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let id_style = if is_selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let mut line_spans = vec![
            Span::styled(marker.to_string(), marker_style),
            Span::styled(entry.session_id.clone(), id_style),
            Span::styled(
                format!("  {} ", entry.started_at),
                Style::default().fg(Color::DarkGray),
            ),
        ];
        if !entry.path_tail.is_empty() {
            line_spans.push(Span::styled(
                entry.path_tail.clone(),
                Style::default().fg(Color::DarkGray),
            ));
        }
        // Highlight the entire row by tinting the background when
        // selected. ratatui's Paragraph doesn't per-line background,
        // so we approximate with a leading bullet and bolding.
        body.push(Line::from(line_spans));
        let _ = row_index; // silence unused
    }
    let list = Paragraph::new(body);
    frame.render_widget(list, rows[1]);

    // Footer: hint that the picker exits on selection.
    let footer = Line::from(vec![Span::styled(
        " Enter prints `crow tui --resume <id>` and exits ",
        Style::default().fg(Color::DarkGray),
    )]);
    frame.render_widget(Paragraph::new(footer), rows[2]);
}

/// Render the approval card as a centered modal. Shows the tool
/// name + a JSON-pretty view of the args + a y/n/a keymap hint.
fn draw_approval_card(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let pending = match app.pending_approval.as_ref() {
        Some(p) => p,
        None => return,
    };

    let popup_w = (area.width as u32 * 7 / 10).max(50) as u16;
    let popup_h = (area.height as u32 / 2).max(11) as u16;
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    // Dim background.
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );

    let block = Block::default()
        .title(format!(" Allow {} ? ", pending.tool_name()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let header = Line::from(vec![
        Span::styled(
            format!(" ask: {} ", pending.ask_id),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("    "),
        Span::styled(
            "The agent wants to run:",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(header), rows[0]);

    let args_text =
        serde_json::to_string_pretty(pending.args()).unwrap_or_else(|_| pending.args().to_string());
    let body = Paragraph::new(args_text)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });
    frame.render_widget(body, rows[1]);

    let footer = Line::from(vec![
        Span::styled(
            " [y] allow ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  [a] allow always (session)  ",
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            "  [n] deny ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(footer), rows[2]);
}
