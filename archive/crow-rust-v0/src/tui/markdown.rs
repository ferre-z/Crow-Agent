//! Minimal markdown -> ratatui [`Line`] conversion.
//!
//! For v1 we render the high-signal subset that actually shows up in
//! agent output: bold, italic, inline code, fenced code, and bullet
//! lists. Headings, tables, and links are passed through as plain
//! text — the structure is rare in agent replies and a fancier
//! renderer can land in a follow-up slice without changing the
//! public interface.
//!
//! The renderer takes `&str` and returns owned `Line<'static>` so
//! callers don't have to thread lifetimes through their state.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Render a markdown source string into a list of styled lines.
///
/// Trailing blank lines are trimmed so the caller can append the
/// next entry without an extra gap.
#[must_use]
pub fn render(source: &str) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(source, options);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block: Option<String> = None;
    let mut list_indent: usize = 0;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { .. } => {
                    push_style(
                        &mut style_stack,
                        Style::default().add_modifier(Modifier::BOLD),
                    );
                }
                Tag::Strong => {
                    let base = current_style(&style_stack);
                    push_style(&mut style_stack, base.add_modifier(Modifier::BOLD));
                }
                Tag::Emphasis => {
                    let base = current_style(&style_stack);
                    push_style(&mut style_stack, base.add_modifier(Modifier::ITALIC));
                }
                Tag::CodeBlock(kind) => {
                    flush(&mut out, &mut current_line, list_indent);
                    let lang = match kind {
                        CodeBlockKind::Indented => String::new(),
                        CodeBlockKind::Fenced(s) => s.to_string(),
                    };
                    in_code_block = Some(lang);
                }
                Tag::List(_) => {
                    flush(&mut out, &mut current_line, list_indent);
                    list_indent += 2;
                }
                Tag::Item => {
                    current_line.push(Span::styled(
                        "• ".to_string(),
                        Style::default().fg(Color::Cyan),
                    ));
                }
                Tag::Link { .. } => {
                    let base = current_style(&style_stack);
                    push_style(&mut style_stack, base.fg(Color::Blue));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    flush(&mut out, &mut current_line, list_indent);
                }
                TagEnd::Heading { .. } => {
                    flush(&mut out, &mut current_line, list_indent);
                    style_stack.pop();
                }
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link => {
                    style_stack.pop();
                }
                TagEnd::CodeBlock => {
                    in_code_block = None;
                }
                TagEnd::List(_) => {
                    list_indent = list_indent.saturating_sub(2);
                }
                _ => {}
            },
            Event::Text(t) => {
                let style = current_style(&style_stack);
                if in_code_block.is_some() {
                    flush_code(&mut out, &t);
                } else {
                    current_line.push(Span::styled(t.to_string(), style));
                }
            }
            Event::Code(t) => {
                current_line.push(Span::styled(
                    t.to_string(),
                    Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Cyan),
                ));
            }
            Event::SoftBreak => {
                flush(&mut out, &mut current_line, list_indent);
            }
            Event::HardBreak => {
                flush(&mut out, &mut current_line, list_indent);
            }
            _ => {}
        }
    }
    flush(&mut out, &mut current_line, list_indent);
    while out.last().is_some_and(|l| l.spans.is_empty()) {
        out.pop();
    }
    out
}

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}

fn push_style(stack: &mut Vec<Style>, style: Style) {
    stack.push(style);
}

fn flush(out: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>, indent: usize) {
    if current.is_empty() {
        out.push(Line::raw(""));
        return;
    }
    let mut spans = Vec::with_capacity(current.len() + 1);
    if indent > 0 {
        spans.push(Span::raw(" ".repeat(indent)));
    }
    spans.append(current);
    out.push(Line::from(spans));
    current.clear();
}

fn flush_code(out: &mut Vec<Line<'static>>, code: &pulldown_cmark::CowStr<'_>) {
    for line in code.split('\n') {
        out.push(Line::from(Span::styled(
            format!("  │ {line}"),
            Style::default().fg(Color::Cyan),
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_no_lines() {
        let lines = render("");
        assert!(lines.is_empty() || lines.iter().all(|l| l.spans.is_empty()));
    }

    #[test]
    fn plain_text_renders() {
        let lines = render("hello world");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "hello world");
    }

    #[test]
    fn bold_text_renders_with_bold_modifier() {
        let lines = render("**bold**");
        assert!(!lines.is_empty());
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn inline_code_has_background() {
        let lines = render("`code`");
        assert!(!lines.is_empty());
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.style.bg == Some(Color::Rgb(40, 40, 40))));
    }

    #[test]
    fn fenced_code_block_flushes_per_line() {
        let lines = render("```\nfoo\nbar\n```");
        assert!(lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("foo"))));
        assert!(lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("bar"))));
    }

    #[test]
    fn list_item_has_bullet() {
        let lines = render("- one\n- two");
        let found = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.starts_with('•')));
        assert!(found);
    }
}
