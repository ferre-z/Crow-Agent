//! The `edit` tool — exact-string replacement in a file.
//!
//! By default the replacement must match exactly once in the file
//! (zero or many matches are rejected so the model can't accidentally
//! edit the wrong spot). The `replace_all` flag opts into a global
//! replacement. A `similar`-powered diff summary is included in the
//! `ToolFinished` event for downstream rendering.

use std::path::PathBuf;

use async_trait::async_trait;
use schemars::{schema::Schema, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::path::safe_resolve;
use super::{Tool, ToolContext, ToolError, ToolEventSink, ToolOutcome, ToolResult};

/// Arguments for the `edit` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EditArgs {
    /// Path to the file to edit (relative to project root or
    /// absolute-under-root).
    pub path: String,
    /// Exact string to find. Must match exactly once unless
    /// `replace_all` is true.
    pub old_text: String,
    /// Replacement string.
    pub new_text: String,
    /// When `true`, allow multiple matches and replace all of them.
    #[serde(default)]
    pub replace_all: bool,
}

/// Edit tool.
#[derive(Debug, Default, Clone, Copy)]
pub struct EditTool;
impl EditTool {
    pub const NAME: &'static str = "edit";

    async fn apply_edit(
        &self,
        project_root: &std::path::Path,
        args: &EditArgs,
        cancel: &CancellationToken,
    ) -> ToolResult {
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let input = PathBuf::from(&args.path);
        let resolved = safe_resolve(project_root, &input).map_err(|e| match e.kind() {
            std::io::ErrorKind::InvalidInput => ToolError::PathEscape(input.clone()),
            _ => ToolError::Io(e),
        })?;
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        // Reject directories explicitly — edit is for regular files.
        let meta = tokio::fs::metadata(&resolved)
            .await
            .map_err(ToolError::Io)?;
        if !meta.is_file() {
            return Err(ToolError::NotAFile(resolved));
        }

        let original = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(ToolError::Io)?;

        let occurrences = original.matches(&args.old_text).count();
        match (occurrences, args.replace_all) {
            (0, _) => {
                return Err(ToolError::InvalidArgs(format!(
                    "old_text not found in {}",
                    resolved.display()
                )));
            }
            (n, false) if n != 1 => {
                return Err(ToolError::InvalidArgs(format!(
                    "old_text matched {n} times; pass replace_all=true to replace all"
                )));
            }
            _ => {}
        }

        let new_content = if args.replace_all {
            original.replace(&args.old_text, &args.new_text)
        } else {
            original.replacen(&args.old_text, &args.new_text, 1)
        };

        // Atomic write to the same path.
        let parent = resolved
            .parent()
            .ok_or_else(|| {
                ToolError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "resolved path has no parent",
                ))
            })?
            .to_path_buf();
        let tmp = tempfile::NamedTempFile::new_in(&parent).map_err(ToolError::Io)?;
        if let Err(e) = tokio::fs::write(tmp.path(), &new_content).await {
            return Err(ToolError::Io(e));
        }
        if let Err(e) = tmp.persist(&resolved) {
            return Err(ToolError::Io(e.error));
        }

        // Compute a diff summary for downstream consumers.
        let diff = similar::TextDiff::from_lines(&original, &new_content)
            .unified_diff()
            .to_string();
        let trimmed_diff: String = diff.chars().take(4096).collect();
        let truncated_diff = diff.chars().count() > trimmed_diff.chars().count();
        let mut output = String::new();
        output.push_str(&format!(
            "replaced {} occurrence{} in {}\n",
            occurrences,
            if occurrences == 1 { "" } else { "s" },
            resolved.display()
        ));
        if !trimmed_diff.is_empty() {
            output.push_str("---\n");
            output.push_str(&trimmed_diff);
            if truncated_diff {
                output.push_str("\n…[diff truncated]");
            }
        }
        Ok(ToolOutcome::Success {
            output,
            truncated: truncated_diff,
        })
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }
    fn description(&self) -> &'static str {
        "Replace `old_text` with `new_text` in `path`. By default the \
         match must be unique; pass `replace_all=true` to opt into a \
         global replace. The path must be inside the project root."
    }
    fn schema(&self) -> Schema {
        let mut gen = schemars::gen::SchemaGenerator::default();
        <EditArgs as schemars::JsonSchema>::json_schema(&mut gen)
    }
    async fn execute(
        &self,
        args: Value,
        ctx: ToolContext,
        _events: ToolEventSink,
        cancel: CancellationToken,
    ) -> ToolResult {
        let parsed: EditArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
        self.apply_edit(&ctx.project_root, &parsed, &cancel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn replaces_unique_match() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "hello world\n").unwrap();
        let tool = EditTool;
        let args = EditArgs {
            path: "a.txt".into(),
            old_text: "world".into(),
            new_text: "planet".into(),
            replace_all: false,
        };
        tool.apply_edit(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "hello planet\n"
        );
    }

    #[tokio::test]
    async fn rejects_zero_matches() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        let tool = EditTool;
        let args = EditArgs {
            path: "a.txt".into(),
            old_text: "absent".into(),
            new_text: "x".into(),
            replace_all: false,
        };
        let err = tool
            .apply_edit(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn rejects_many_matches_without_replace_all() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a a a").unwrap();
        let tool = EditTool;
        let args = EditArgs {
            path: "a.txt".into(),
            old_text: "a".into(),
            new_text: "b".into(),
            replace_all: false,
        };
        let err = tool
            .apply_edit(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn replace_all_handles_many_matches() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a a a").unwrap();
        let tool = EditTool;
        let args = EditArgs {
            path: "a.txt".into(),
            old_text: "a".into(),
            new_text: "b".into(),
            replace_all: true,
        };
        tool.apply_edit(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "b b b"
        );
    }

    #[tokio::test]
    async fn rejects_path_outside_root() {
        let tmp = TempDir::new().unwrap();
        let tool = EditTool;
        let args = EditArgs {
            path: "../escape.txt".into(),
            old_text: "x".into(),
            new_text: "y".into(),
            replace_all: false,
        };
        let err = tool
            .apply_edit(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
    }
}
