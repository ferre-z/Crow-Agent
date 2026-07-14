//! The `read` tool — the first capability Crow v0 ships.
//!
//! Spec §13 mandates a `read` tool that accepts a path, optional
//! line offset, and optional line limit; rejects paths outside the
//! project root, directories, and binary files; adds line numbers; and
//! caps the returned bytes with a truncation flag. The implementation
//! here matches that contract and uses [`crate::tool::path::safe_resolve`]
//! for every filesystem access.
//!
//! ## Output format
//!
//! Each line of the returned string is prefixed with a 1-based line
//! number right-padded to a width of 6, followed by a tab, followed by
//! the line content. The trailing newline of the final line is
//! preserved if the file had one (so consumers can tell "empty file"
//! from "file with one empty line").
//!
//! Example:
//!
//! ```text
//! 1    fn main() {
//! 2        println!("hi");
//! 3    }
//! ```
//!
//! ## Truncation
//!
//! If the produced output exceeds `ctx.max_output_bytes`, we truncate
//! at the nearest byte boundary that does not split a UTF-8 codepoint
//! and set `truncated: true`. We never read past `offset + limit`
//! lines from disk.

use std::fs;
use std::path::PathBuf;

use async_trait::async_trait;
use schemars::{schema::Schema, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::path::{looks_binary, safe_resolve};
use super::{Tool, ToolContext, ToolError, ToolEventSink, ToolOutcome, ToolResult};

/// Arguments for the `read` tool.
///
/// `schemars::schema_for!(ReadArgs)` produces the JSON Schema the
/// model uses to construct a call. Keep field names snake_case so the
/// schema matches the model's expectations.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Path to the file to read. Relative paths resolve against the
    /// project's root; absolute paths must also live inside the root.
    pub path: String,

    /// 0-based line index to start at. Defaults to 0 (the first
    /// line). If the value is past EOF, the result is an empty string.
    #[serde(default)]
    pub offset: Option<u64>,

    /// Maximum number of lines to return, starting at `offset`.
    /// `None` means "until EOF or the byte cap".
    #[serde(default)]
    pub limit: Option<u64>,
}

/// The read tool. Stateless, cheap to clone (it's just a unit struct
/// holding no data), so we don't bother wrapping it in an `Arc`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReadTool;
impl ReadTool {
    pub const NAME: &'static str = "read";

    /// Read a file in its entirety (after `safe_resolve`) into a
    /// `String`, then run it through [`format_lines`] with `offset` /
    /// `limit`. Split out so the integration tests in
    /// `tests/tool_registry.rs` can exercise the formatting path
    /// without a `ToolContext`.
    ///
    /// Returns the formatted output (possibly truncated to
    /// `max_output_bytes`) and the `truncated` flag.
    fn read_and_format(
        &self,
        project_root: &std::path::Path,
        args: &ReadArgs,
        max_output_bytes: usize,
        cancel: &CancellationToken,
    ) -> ToolResult {
        // Cancellation check before any I/O so an already-cancelled
        // token does not even touch the disk.
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // 1. Resolve & confine.
        let input = PathBuf::from(&args.path);
        let resolved = safe_resolve(project_root, &input).map_err(|e| {
            // Permission errors and NotFound are surfaced as-is so
            // the model gets an honest message. Escape attempts
            // (InvalidInput from safe_resolve) become PathEscape so
            // the agent loop can distinguish them.
            match e.kind() {
                std::io::ErrorKind::InvalidInput => ToolError::PathEscape(input.clone()),
                _ => ToolError::Io(e),
            }
        })?;

        // 2. Must be a regular file.
        let meta = fs::metadata(&resolved).map_err(ToolError::Io)?;
        if !meta.is_file() {
            return Err(ToolError::NotAFile(resolved));
        }

        // 3. Read the bytes.
        let bytes = fs::read(&resolved).map_err(ToolError::Io)?;

        // 4. Mid-read cancellation check. (Reading from a small file
        // is effectively synchronous, but if we ever swap to tokio's
        // async fs we want the cancel check to fire here too.)
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // 5. Binary check — only on the first 8 KiB, which is what
        // `looks_binary` already caps at. We don't need a second
        // slice.
        if looks_binary(&bytes) {
            return Err(ToolError::Binary(resolved));
        }

        // 6. Decode UTF-8 (lossily so a stray byte doesn't crash
        // the agent; replacement char is fine for diagnostic
        // purposes).
        let text = String::from_utf8_lossy(&bytes);

        // 7. Slice by offset/limit, then format with line numbers.
        let (sliced, offset_applied) = apply_offset_limit(&text, args.offset, args.limit);
        let formatted = format_lines(&sliced, offset_applied);

        // 8. Truncate to the byte cap at a char boundary, set flag.
        let (output, truncated) = truncate_to_bytes(&formatted, max_output_bytes);
        Ok(ToolOutcome::Success { output, truncated })
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Read the contents of a file. `path` is relative to the project \
         root or absolute-under-root. `offset` is the 0-based line to \
         start at (default 0). `limit` is the max number of lines to \
         return (default: until EOF). Directories, binary files, and \
         paths outside the root are rejected."
    }

    fn schema(&self) -> Schema {
        let mut gen = schemars::gen::SchemaGenerator::default();
        <ReadArgs as schemars::JsonSchema>::json_schema(&mut gen)
    }

    async fn execute(
        &self,
        args: Value,
        ctx: ToolContext,
        _events: ToolEventSink,
        cancel: CancellationToken,
    ) -> ToolResult {
        // Parse args. If they fail to deserialise at all, surface as
        // InvalidArgs so the wrapper reports `invalid_args`.
        let parsed: ReadArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
        self.read_and_format(&ctx.project_root, &parsed, ctx.max_output_bytes, &cancel)
    }
}

/// Slice `text` by line offset / limit.
///
/// `offset` is 0-based. `limit: None` means "rest of file". Returns
/// the sliced string and the 0-based line number that the first line
/// of the slice corresponds to (so the formatter can label lines
/// starting at the right number).
fn apply_offset_limit(text: &str, offset: Option<u64>, limit: Option<u64>) -> (String, u64) {
    // Split on `\n` keeping the original trailing-newline
    // semantics: a trailing `\n` produces an empty trailing element
    // we drop, otherwise the line count is off by one.
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let total = lines.len();

    let start = offset.unwrap_or(0) as usize;
    if start >= total {
        return (String::new(), offset.unwrap_or(0));
    }
    let end = match limit {
        Some(n) => (start + n as usize).min(total),
        None => total,
    };
    // Re-join with `\n`. If the original file ended with `\n` we
    // preserve that by checking the source: if the source's last
    // byte is `\n` and we are not truncating by limit, re-append.
    let body = lines[start..end].join("\n");
    let trailing_nl = text.ends_with('\n') && limit.is_none();
    let out = if trailing_nl {
        format!("{body}\n")
    } else {
        body
    };
    (out, start as u64)
}

/// Format a slice of text with `1-based + start_offset` line numbers.
///
/// Each line is prefixed with `<n>\t` where `<n>` is right-justified
/// to a width of 6 digits (room for ~10M lines before it grows). For
/// files past 9_999_999 lines the width is just whatever Rust's
/// default formatting does — still readable, not a hard error.
fn format_lines(text: &str, start_offset: u64) -> String {
    if text.is_empty() {
        return String::new();
    }
    // Split by '\n' and drop a trailing empty (so "a\n" yields
    // ["a"] not ["a", ""]). Number each remaining line.
    let mut lines: Vec<&str> = text.split('\n').collect();
    let trailing_newline = lines.last() == Some(&"");
    if trailing_newline {
        lines.pop();
    }
    if lines.is_empty() {
        // The input was just newlines; emit one empty formatted line
        // so the output is at least one line. (Edge case: a file
        // containing only "\n".)
        return String::new();
    }
    // Width = number of digits in the largest line number we will
    // produce. Minimum 6 — the v0 CLI always shows line numbers
    // right-justified to a width of 6, even for short files (this
    // keeps the column visually stable across a session).
    let max_n = start_offset + lines.len() as u64;
    let width = std::cmp::max(6, max_n.ilog10() as usize + 1);
    let mut out = String::with_capacity(text.len() + text.len() / 40 * (width + 1));
    for (i, line) in lines.iter().enumerate() {
        let n = start_offset + i as u64 + 1; // 1-based
        let formatted = format!("{n:>width$}");
        out.push_str(&formatted);
        out.push('\t');
        out.push_str(line);
        out.push('\n');
    }
    // If the input ended with '\n', we already produced one '\n' per
    // line in the loop. The expected output for "a\n" is
    // "     1\ta\n" — single '\n' at end. For "a\nb\n" the expected
    // is "     1\ta\n     2\tb\n\n" — TWO '\n' at end (one per
    // line in the loop, plus one from the trailing newline we
    // stripped). Match that: if the input had a trailing '\n', add
    // an extra one.
    if trailing_newline {
        out.push('\n');
    }
    out
}

/// Truncate `s` so it fits within `limit` bytes without splitting a
/// UTF-8 codepoint. Returns the (possibly shortened) string and a
/// flag indicating whether truncation happened.
fn truncate_to_bytes(s: &str, limit: usize) -> (String, bool) {
    if s.len() <= limit {
        return (s.to_string(), false);
    }
    // Walk back from `limit` until we land on a char boundary.
    let mut cut = limit;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut truncated = s[..cut].to_string();
    truncated.push_str("\n…[truncated]");
    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    fn temp_root() -> TempDir {
        TempDir::new().unwrap()
    }

    fn ctx_for(root: &std::path::Path, max_bytes: usize) -> ToolContext {
        ToolContext {
            project_root: root.to_path_buf(),
            max_output_bytes: max_bytes,
            command_timeout: std::time::Duration::from_secs(5),
        }
    }

    fn cancelled_token() -> CancellationToken {
        let t = CancellationToken::new();
        t.cancel();
        t
    }

    #[tokio::test]
    async fn reads_small_file() {
        let tmp = temp_root();
        let path = tmp.path().join("a.txt");
        fs::write(&path, "hello\nworld\n").unwrap();
        let args = ReadArgs {
            path: "a.txt".into(),
            offset: None,
            limit: None,
        };
        let outcome = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap();
        let ToolOutcome::Success { output, truncated } = outcome else {
            panic!("expected Success");
        };
        assert_eq!(output, "     1\thello\n     2\tworld\n\n");
        assert!(!truncated);
    }

    #[tokio::test]
    async fn reads_with_offset_and_limit() {
        let tmp = temp_root();
        let path = tmp.path().join("lines.txt");
        let body = (1..=10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, &body).unwrap();
        let args = ReadArgs {
            path: "lines.txt".into(),
            offset: Some(2),
            limit: Some(3),
        };
        let outcome = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap();
        let ToolOutcome::Success { output, truncated } = outcome else {
            panic!("Success expected");
        };
        // Lines indexed 3..=5 (1-based, offset=2 → start at line 3).
        assert_eq!(output, "     3\tline3\n     4\tline4\n     5\tline5\n");
        assert!(!truncated);
    }

    #[tokio::test]
    async fn read_offset_past_eof_returns_empty() {
        let tmp = temp_root();
        let path = tmp.path().join("tiny.txt");
        fs::write(&path, "one\n").unwrap();
        let args = ReadArgs {
            path: "tiny.txt".into(),
            offset: Some(100),
            limit: None,
        };
        let outcome = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap();
        let ToolOutcome::Success { output, truncated } = outcome else {
            panic!("Success expected");
        };
        assert_eq!(output, "");
        assert!(!truncated);
    }

    #[tokio::test]
    async fn read_binary_file_returns_binary_error() {
        let tmp = temp_root();
        let path = tmp.path().join("blob.bin");
        let mut bytes = vec![b'a'; 100];
        bytes[50] = 0;
        bytes[60] = 0x01;
        bytes[70] = 0x02;
        bytes[80] = 0x03;
        fs::write(&path, &bytes).unwrap();
        let args = ReadArgs {
            path: "blob.bin".into(),
            offset: None,
            limit: None,
        };
        let err = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap_err();
        assert!(matches!(err, ToolError::Binary(_)));
    }

    #[tokio::test]
    async fn read_directory_returns_not_a_file_error() {
        let tmp = temp_root();
        fs::create_dir(tmp.path().join("d")).unwrap();
        let args = ReadArgs {
            path: "d".into(),
            offset: None,
            limit: None,
        };
        let err = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap_err();
        assert!(matches!(err, ToolError::NotAFile(_)));
    }

    #[tokio::test]
    async fn read_dotdot_escape_returns_path_escape_error() {
        let tmp = temp_root();
        let args = ReadArgs {
            path: "../escape.txt".into(),
            offset: None,
            limit: None,
        };
        let err = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
    }

    #[tokio::test]
    async fn read_absolute_outside_root_returns_path_escape_error() {
        let tmp = temp_root();
        let outside = std::path::Path::new("/etc/hostname");
        if !outside.exists() {
            return;
        }
        let args = ReadArgs {
            path: "/etc/hostname".into(),
            offset: None,
            limit: None,
        };
        let err = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn read_through_symlink_outside_root_returns_path_escape_error() {
        let tmp = temp_root();
        let outside = std::path::Path::new("/etc/hosts");
        if !outside.exists() {
            return;
        }
        let link = tmp.path().join("pass-through");
        symlink(outside, &link).unwrap();
        let args = ReadArgs {
            path: "pass-through".into(),
            offset: None,
            limit: None,
        };
        let err = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
    }

    #[tokio::test]
    async fn large_output_is_truncated() {
        let tmp = temp_root();
        let path = tmp.path().join("big.txt");
        // 5000 lines, each ~10 bytes → 50KB body. Plus prefix,
        // easily exceeds a 1KB cap.
        let body = (1..=5000)
            .map(|i| format!("line-{i:05}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, &body).unwrap();
        let args = ReadArgs {
            path: "big.txt".into(),
            offset: None,
            limit: None,
        };
        let outcome = ReadTool
            .read_and_format(tmp.path(), &args, 1024, &CancellationToken::new())
            .unwrap();
        let ToolOutcome::Success { output, truncated } = outcome else {
            panic!("Success expected");
        };
        assert!(truncated, "large output must set truncated flag");
        // Output must end with the truncation marker.
        assert!(output.ends_with("…[truncated]"));
        // Output must be at most max_output_bytes plus the marker.
        assert!(output.len() <= 1024 + "…[truncated]\n".len() + 1);
    }

    #[tokio::test]
    async fn empty_file_returns_empty_output() {
        let tmp = temp_root();
        let path = tmp.path().join("empty.txt");
        fs::write(&path, "").unwrap();
        let args = ReadArgs {
            path: "empty.txt".into(),
            offset: None,
            limit: None,
        };
        let outcome = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &CancellationToken::new())
            .unwrap();
        let ToolOutcome::Success { output, truncated } = outcome else {
            panic!("Success expected");
        };
        assert_eq!(output, "");
        assert!(!truncated);
    }

    #[tokio::test]
    async fn cancelled_token_short_circuits_before_io() {
        let tmp = temp_root();
        // Don't even create the file — a cancelled token should
        // fail before touching the disk.
        let args = ReadArgs {
            path: "nope.txt".into(),
            offset: None,
            limit: None,
        };
        let err = ReadTool
            .read_and_format(tmp.path(), &args, 4096, &cancelled_token())
            .unwrap_err();
        assert!(matches!(err, ToolError::Cancelled));
    }

    #[tokio::test]
    async fn looks_binary_returns_true_for_nul_bytes() {
        let mut bytes = vec![b'a'; 100];
        bytes[50] = 0;
        assert!(looks_binary(&bytes));
    }

    #[tokio::test]
    async fn looks_binary_returns_false_for_ascii() {
        assert!(!looks_binary(b"hello world\n"));
    }

    // ---- helper tests ----

    #[test]
    fn format_lines_adds_one_based_prefix() {
        let s = format_lines("a\nb\nc\n", 0);
        assert_eq!(s, "     1\ta\n     2\tb\n     3\tc\n\n");
    }

    #[test]
    fn format_lines_honours_start_offset() {
        let s = format_lines("a\nb\n", 9);
        // offset=9 → first line is #10.
        assert_eq!(s, "    10\ta\n    11\tb\n\n");
    }

    #[test]
    fn apply_offset_limit_truncates_by_lines() {
        let text = "l1\nl2\nl3\nl4\n";
        let (out, start) = apply_offset_limit(text, Some(1), Some(2));
        assert_eq!(out, "l2\nl3");
        assert_eq!(start, 1);
    }

    #[test]
    fn apply_offset_limit_returns_empty_for_past_eof() {
        let text = "l1\nl2\n";
        let (out, start) = apply_offset_limit(text, Some(99), None);
        assert_eq!(out, "");
        assert_eq!(start, 99);
    }

    #[test]
    fn truncate_to_bytes_preserves_char_boundaries() {
        // "héllo" = 6 bytes; truncating at 3 hits in the middle of
        // "é" (0xC3 0xA9). The function must walk back to 1.
        let s = "héllo";
        let (out, truncated) = truncate_to_bytes(s, 3);
        assert!(truncated);
        assert!(out.starts_with('h'));
        assert!(out.ends_with("…[truncated]"));
    }

    #[test]
    fn truncate_to_bytes_no_op_when_under_limit() {
        let s = "hello";
        let (out, truncated) = truncate_to_bytes(s, 100);
        assert_eq!(out, "hello");
        assert!(!truncated);
    }

    #[tokio::test]
    async fn execute_passes_through_invalid_args() {
        let tool = ReadTool;
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let outcome = tool
            .execute(
                serde_json::json!({"path": 123}), // wrong type
                ctx_for(std::path::Path::new("/tmp"), 1024),
                tx,
                CancellationToken::new(),
            )
            .await;
        assert!(matches!(outcome, Err(ToolError::InvalidArgs(_))));
    }

    #[tokio::test]
    async fn tool_name_is_read() {
        assert_eq!(ReadTool.name(), "read");
        assert!(!ReadTool.description().is_empty());
    }
}
