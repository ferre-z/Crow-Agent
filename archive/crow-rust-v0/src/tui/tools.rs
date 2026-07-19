//! Per-tool rendering for TUI cards.
//!
//! Each kernel tool has its own argument shape and result shape.
//! Rather than hard-code "show args + show output" in [`crate::tui::ui`],
//! this module owns a [`ToolRenderer`] enum that knows how to project
//! one tool's call + result into styled [`Line`]s. The renderer is
//! selected by tool name; unknown tools fall through to a generic
//! "args + output" card so the TUI never blanks on a new tool.
//!
//! Why an enum instead of a `dyn Trait`?
//!
//! - The renderer is always local and short-lived; vtables add cost.
//! - Pattern-matching on `ToolRenderer` is exhaustive at compile time,
//!   so adding a new tool variant is a forced update here.
//! - No `Send`/`Sync` plumbing needed.
//!
//! ## Adding a new tool
//!
//! 1. Add a variant to [`ToolRenderer`].
//! 2. Add the match arm in [`ToolRenderer::from_name`].
//! 3. Implement [`ToolRenderer::render`] for the new variant.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::{ChangeTag, TextDiff};

/// Which renderer to use for a given tool call. Selected by tool name
/// in [`ToolRenderer::from_name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRenderer {
    /// `read` — file content with line numbers in the output.
    Read,
    /// `write` — file overwrite with new content.
    Write,
    /// `edit` — exact-string replacement; renders a unified diff.
    Edit,
    /// `bash` — shell command + captured stdout/stderr.
    Bash,
    /// Unknown tool — show generic args + output.
    Generic,
}

impl ToolRenderer {
    /// Pick a renderer from the tool name. Falls back to [`Self::Generic`]
    /// for any name we don't recognise so the TUI keeps working when
    /// new tools are added upstream.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "read" => Self::Read,
            "write" => Self::Write,
            "edit" => Self::Edit,
            "bash" => Self::Bash,
            _ => Self::Generic,
        }
    }

    /// Render one tool call into styled [`Line`]s.
    ///
    /// `args` is the JSON the model emitted; `output` is the captured
    /// tool result body (already a string); `is_error` flags failure;
    /// `truncated` says the kernel elided output; `stdout`/`stderr`
    /// are streamed-chunk buffers the kernel kept alongside `output`.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn render(
        self,
        name: &str,
        args: &serde_json::Value,
        output: &str,
        is_error: bool,
        truncated: bool,
        stdout: &str,
        stderr: &str,
    ) -> Vec<Line<'static>> {
        match self {
            Self::Read => render_read(args, output, is_error, truncated),
            Self::Write => render_write(args, output, is_error, truncated),
            Self::Edit => render_edit(args, output, is_error, truncated),
            Self::Bash => render_bash(args, output, stdout, stderr, is_error, truncated),
            Self::Generic => render_generic(name, args, output, is_error, truncated),
        }
    }
}

// ----- per-tool renderers ------------------------------------------------

fn render_read(
    _args: &serde_json::Value,
    output: &str,
    is_error: bool,
    truncated: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![header("read", "file", is_error)];
    if is_error {
        // On error the output IS the error; render plainly so the
        // user can read the message.
        for line in output.lines() {
            lines.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(Color::Red),
            )));
        }
        return lines;
    }
    // Output is already line-numbered ("N\tcontent"). Highlight the
    // line-number column in dark grey and the content in white.
    for raw in output.lines() {
        let (lineno, body) = split_line_numbered(raw);
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {lineno:<6}"),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(body.to_string(), Style::default().fg(Color::White)),
        ]));
    }
    if truncated {
        lines.push(truncation_note());
    }
    lines.push(Line::raw(""));
    lines
}

fn render_write(
    args: &serde_json::Value,
    output: &str,
    is_error: bool,
    truncated: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![header("write", "file", is_error)];
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    lines.push(Line::from(vec![
        Span::styled("  path: ", Style::default().fg(Color::DarkGray)),
        Span::styled(path.to_string(), Style::default().fg(Color::Cyan)),
    ]));
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if is_error {
        for line in output.lines() {
            lines.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(Color::Red),
            )));
        }
        return lines;
    }
    // Show the written body as a code-style block. Indent it so it
    // visually nests under the `path:` line.
    for raw in content.lines() {
        lines.push(Line::from(Span::styled(
            format!("    │ {raw}"),
            Style::default().fg(Color::White),
        )));
    }
    if !output.is_empty() {
        for line in output.lines() {
            lines.push(Line::from(Span::styled(
                format!("  ↳ {line}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    if truncated {
        lines.push(truncation_note());
    }
    lines.push(Line::raw(""));
    lines
}

fn render_edit(
    args: &serde_json::Value,
    output: &str,
    is_error: bool,
    truncated: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![header("edit", "file", is_error)];
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    lines.push(Line::from(vec![
        Span::styled("  path: ", Style::default().fg(Color::DarkGray)),
        Span::styled(path.to_string(), Style::default().fg(Color::Cyan)),
    ]));
    if is_error {
        for line in output.lines() {
            lines.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(Color::Red),
            )));
        }
        return lines;
    }
    let old_text = args.get("old_text").and_then(|v| v.as_str()).unwrap_or("");
    let new_text = args.get("new_text").and_then(|v| v.as_str()).unwrap_or("");
    let diff = TextDiff::from_lines(old_text, new_text);
    for change in diff.iter_all_changes() {
        let tag = change.tag();
        let prefix = match tag {
            ChangeTag::Equal => " ",
            ChangeTag::Insert => "+",
            ChangeTag::Delete => "-",
        };
        let color = match tag {
            ChangeTag::Equal => Color::DarkGray,
            ChangeTag::Insert => Color::Green,
            ChangeTag::Delete => Color::Red,
        };
        let value = change.value().trim_end_matches('\n');
        lines.push(Line::from(Span::styled(
            format!("    {prefix} {value}"),
            Style::default().fg(color),
        )));
    }
    if !output.is_empty() {
        for line in output.lines() {
            lines.push(Line::from(Span::styled(
                format!("  ↳ {line}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    if truncated {
        lines.push(truncation_note());
    }
    lines.push(Line::raw(""));
    lines
}

fn render_bash(
    args: &serde_json::Value,
    output: &str,
    stdout: &str,
    stderr: &str,
    is_error: bool,
    truncated: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![header("bash", "shell", is_error)];
    let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
    lines.push(Line::from(vec![
        Span::styled("  $ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            cmd.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    if !stdout.is_empty() {
        for line in stdout.lines() {
            lines.push(Line::from(Span::styled(
                format!("    │ {line}"),
                Style::default().fg(Color::White),
            )));
        }
    }
    if !stderr.is_empty() {
        for line in stderr.lines() {
            lines.push(Line::from(Span::styled(
                format!("    ⎿ {line}"),
                Style::default().fg(Color::Yellow),
            )));
        }
    }
    if !output.is_empty() {
        for line in output.lines() {
            lines.push(Line::from(Span::styled(
                format!("    ↳ {line}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    if is_error {
        lines.push(Line::from(Span::styled(
            "    exit: non-zero",
            Style::default().fg(Color::Red),
        )));
    }
    if truncated {
        lines.push(truncation_note());
    }
    lines.push(Line::raw(""));
    lines
}

fn render_generic(
    name: &str,
    args: &serde_json::Value,
    output: &str,
    is_error: bool,
    truncated: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![header(name, "tool", is_error)];
    if let Some(obj) = args.as_object() {
        if !obj.is_empty() {
            lines.push(Line::from(Span::styled(
                "  args:",
                Style::default().fg(Color::DarkGray),
            )));
            for (k, v) in obj {
                lines.push(Line::from(Span::styled(
                    format!("    {k}: {v}"),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }
    let color = if is_error { Color::Red } else { Color::White };
    for line in output.lines() {
        lines.push(Line::from(Span::styled(
            format!("    {line}"),
            Style::default().fg(color),
        )));
    }
    if truncated {
        lines.push(truncation_note());
    }
    lines.push(Line::raw(""));
    lines
}

// ----- helpers ----------------------------------------------------------

/// Card header line: a green/red dot, the tool name, and a one-word
/// subject hint (`file`, `shell`, `tool`) for orientation.
fn header(name: &str, subject: &str, is_error: bool) -> Line<'static> {
    let dot = if is_error { "✗" } else { "✓" };
    let dot_color = if is_error { Color::Red } else { Color::Green };
    Line::from(vec![
        Span::styled(format!(" {dot} "), Style::default().fg(dot_color)),
        Span::styled(
            name.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {subject}"), Style::default().fg(Color::DarkGray)),
    ])
}

/// Footer note shown when the kernel truncated output.
fn truncation_note() -> Line<'static> {
    Line::from(Span::styled(
        "    … (output truncated)",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::ITALIC),
    ))
}

/// Split `"<digits>\t<rest>"` into `(lineno, body)`. The `read` tool
/// emits this format; we re-parse it here so we can colour the line
/// number column separately.
fn split_line_numbered(raw: &str) -> (String, String) {
    if let Some(idx) = raw.find('\t') {
        let (n, body) = raw.split_at(idx);
        // Strip the tab from the body.
        (n.to_string(), body[1..].to_string())
    } else {
        // Output without a leading number — treat as a continuation
        // line attached to the previous frame.
        (String::new(), raw.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_tool_routes_to_generic() {
        assert_eq!(ToolRenderer::from_name("frobnicate"), ToolRenderer::Generic);
        assert_eq!(ToolRenderer::from_name("read"), ToolRenderer::Read);
        assert_eq!(ToolRenderer::from_name("edit"), ToolRenderer::Edit);
        assert_eq!(ToolRenderer::from_name("write"), ToolRenderer::Write);
        assert_eq!(ToolRenderer::from_name("bash"), ToolRenderer::Bash);
    }

    #[test]
    fn read_renderer_preserves_line_numbers() {
        let args = json!({"path": "/tmp/x.rs"});
        let output = "1\tfn main() {\n2\t    println!(\"hi\");\n3\t}\n";
        let lines = ToolRenderer::Read.render("read", &args, output, false, false, "", "");
        // Header + 3 numbered lines + trailing blank.
        assert!(lines.len() >= 4);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("1"));
        assert!(all_text.contains("fn main()"));
    }

    #[test]
    fn edit_renderer_emits_diff_markers() {
        let args = json!({
            "path": "/tmp/x.rs",
            "old_text": "let a = 1;\n",
            "new_text": "let a = 2;\n",
        });
        let lines = ToolRenderer::Edit.render("edit", &args, "", false, false, "", "");
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("- let a = 1;"));
        assert!(all_text.contains("+ let a = 2;"));
    }

    #[test]
    fn write_renderer_shows_path_and_content() {
        let args = json!({"path": "/tmp/new.rs", "content": "fn x() {}\n"});
        let lines =
            ToolRenderer::Write.render("write", &args, "wrote 11 bytes", false, false, "", "");
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("/tmp/new.rs"));
        assert!(all_text.contains("fn x()"));
    }

    #[test]
    fn bash_renderer_shows_command_and_stdout() {
        let args = json!({"command": "echo hello"});
        let lines = ToolRenderer::Bash.render("bash", &args, "", false, false, "hello\n", "");
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("echo hello"));
        assert!(all_text.contains("hello"));
    }

    #[test]
    fn error_renderer_uses_red_text() {
        let args = json!({"path": "/nope"});
        let lines =
            ToolRenderer::Read.render("read", &args, "permission denied", true, false, "", "");
        let red_seen = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .any(|s| s.style.fg == Some(Color::Red));
        assert!(red_seen);
    }

    #[test]
    fn split_line_numbered_handles_tab() {
        let (n, b) = split_line_numbered("42\thello");
        assert_eq!(n, "42");
        assert_eq!(b, "hello");
    }

    #[test]
    fn split_line_numbered_handles_missing_tab() {
        let (n, b) = split_line_numbered("no-tab-here");
        assert_eq!(n, "");
        assert_eq!(b, "no-tab-here");
    }

    #[test]
    fn truncated_renderer_emits_truncation_note() {
        let args = json!({"path": "/tmp/big.rs"});
        let output = "1\tline\n";
        let lines = ToolRenderer::Read.render("read", &args, output, false, true, "", "");
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("truncated"));
    }

    #[test]
    fn generic_renderer_falls_back_for_unknown_tool() {
        let args = json!({"whatever": 42});
        let lines = ToolRenderer::Generic.render("frobnicate", &args, "ok", false, false, "", "");
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("frobnicate"));
        assert!(all_text.contains("whatever"));
        assert!(all_text.contains("ok"));
    }

    #[test]
    fn edit_renderer_with_no_changes_renders_unchanged_line() {
        let args = json!({
            "path": "/tmp/x.rs",
            "old_text": "let a = 1;\n",
            "new_text": "let a = 1;\n",
        });
        let lines = ToolRenderer::Edit.render("edit", &args, "", false, false, "", "");
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("let a = 1;"));
        assert!(!all_text.contains("+ let a = 1;"));
        assert!(!all_text.contains("- let a = 1;"));
    }
}
