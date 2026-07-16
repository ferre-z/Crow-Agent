//! The `write` tool — atomic file write inside the project root.
//!
//! Creates or overwrites a file with the given content. Writes are
//! atomic at the filesystem level: content goes to a sibling
//! temp-file and is then renamed into place, so a crash mid-write
//! leaves either the old content or the new content — never a
//! half-written file.
//!
//! Path resolution and root confinement reuse
//! [`crate::tool::path::safe_resolve`].

use std::path::PathBuf;

use async_trait::async_trait;
use schemars::{schema::Schema, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::path::safe_resolve;
use super::{Tool, ToolContext, ToolError, ToolEventSink, ToolOutcome, ToolResult};

/// Arguments for the `write` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WriteArgs {
    /// Path to write. Relative paths resolve against the project
    /// root; absolute paths must also live inside the root. Parent
    /// directories are created if missing.
    pub path: String,
    /// Full content to write. The file is overwritten.
    pub content: String,
}

/// The write tool. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct WriteTool;
impl WriteTool {
    pub const NAME: &'static str = "write";

    async fn write_atomic(
        &self,
        project_root: &std::path::Path,
        args: &WriteArgs,
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

        // Parent dir creation. safe_resolve above would have walked
        // through nonexistent ancestors; here we create them if
        // missing. We still re-check that the created parent is
        // inside project_root after creation.
        if let Some(parent) = resolved.parent() {
            if !parent.exists() {
                let canonical_parent = tokio::fs::canonicalize(project_root)
                    .await
                    .map_err(ToolError::Io)?;
                // Re-resolve the parent under the canonical root so
                // symlinked parents can't redirect the new directory.
                let safe_parent = match safe_resolve(&canonical_parent, parent)
                    .or_else(|_| safe_resolve(project_root, parent))
                {
                    Ok(p) => p,
                    Err(e) => return Err(ToolError::Io(e)),
                };
                tokio::fs::create_dir_all(&safe_parent)
                    .await
                    .map_err(ToolError::Io)?;
            }
        }

        // Atomic temp + rename.
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
        if let Err(e) = tokio::fs::write(tmp.path(), &args.content).await {
            // tmp's Drop will remove the partial file.
            return Err(ToolError::Io(e));
        }
        if let Err(e) = tmp.persist(&resolved) {
            return Err(ToolError::Io(e.error));
        }
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(ToolOutcome::Success {
            output: format!(
                "wrote {} bytes to {}",
                args.content.len(),
                resolved.display()
            ),
            truncated: false,
        })
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        Self::NAME
    }
    fn description(&self) -> &'static str {
        "Write `content` to `path` (creates parent dirs if needed). \
         Overwrites any existing file atomically. The path must be \
         inside the project root."
    }
    fn schema(&self) -> Schema {
        let mut gen = schemars::gen::SchemaGenerator::default();
        <WriteArgs as schemars::JsonSchema>::json_schema(&mut gen)
    }
    async fn execute(
        &self,
        args: Value,
        ctx: ToolContext,
        _events: ToolEventSink,
        cancel: CancellationToken,
    ) -> ToolResult {
        let parsed: WriteArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
        self.write_atomic(&ctx.project_root, &parsed, &cancel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    fn ctx(root: &std::path::Path) -> ToolContext {
        ToolContext {
            call_id: crate::ids::ToolCallId(crate::ids::new_id()),
            project_root: root.to_path_buf(),
            max_output_bytes: 4096,
            command_timeout: std::time::Duration::from_secs(5),
        }
    }

    #[tokio::test]
    async fn writes_new_file() {
        let tmp = TempDir::new().unwrap();
        let tool = WriteTool;
        let args = WriteArgs {
            path: "hello.txt".into(),
            content: "hi".into(),
        };
        let outcome = tool
            .write_atomic(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap();
        match outcome {
            ToolOutcome::Success { .. } => {}
            other => panic!("expected Success, got {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("hello.txt")).unwrap(),
            "hi"
        );
    }

    #[tokio::test]
    async fn overwrites_existing_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "old").unwrap();
        let tool = WriteTool;
        let args = WriteArgs {
            path: "a.txt".into(),
            content: "new".into(),
        };
        tool.write_atomic(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "new"
        );
    }

    #[tokio::test]
    async fn creates_missing_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let tool = WriteTool;
        let args = WriteArgs {
            path: "deep/nested/dir/file.txt".into(),
            content: "deep".into(),
        };
        tool.write_atomic(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("deep/nested/dir/file.txt")).unwrap(),
            "deep"
        );
    }

    #[tokio::test]
    async fn rejects_path_outside_root() {
        let tmp = TempDir::new().unwrap();
        let tool = WriteTool;
        let args = WriteArgs {
            path: "../escape.txt".into(),
            content: "x".into(),
        };
        let err = tool
            .write_atomic(tmp.path(), &args, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
    }

    #[tokio::test]
    async fn execute_passes_through_invalid_args() {
        let tool = WriteTool;
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let outcome = tool
            .execute(
                serde_json::json!({"path": 123}),
                ctx(std::path::Path::new("/tmp")),
                tx,
                CancellationToken::new(),
            )
            .await;
        assert!(matches!(outcome, Err(ToolError::InvalidArgs(_))));
    }
}
