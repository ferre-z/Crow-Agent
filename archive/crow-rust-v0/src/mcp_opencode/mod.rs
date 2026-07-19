//! MCP server that delegates work in parallel to the `opencode` CLI.
//!
//! Entry point is [`run`], wired to the `crow mcp-opencode` subcommand.
//! The server speaks JSON-RPC 2.0 over stdio per the Model Context
//! Protocol (2025-06-18) and exposes seven tools:
//!
//! | Tool | Purpose |
//! |---|---|
//! | `opencode_delegate` | One task → one opencode run, return result |
//! | `opencode_delegate_parallel` | N independent tasks, run concurrently |
//! | `opencode_delegate_fanout` | One prompt × N working directories |
//! | `opencode_status` | Lookup in-flight task status by id |
//! | `opencode_cancel` | Cancel an in-flight task by id |
//! | `opencode_list_models` | List models known to opencode |
//! | `opencode_version` | Server + opencode binary diagnostic info |
//!
//! ## Architecture
//!
//! ```text
//! protocol.rs   ← JSON-RPC over stdio (initialize, tools/list, tools/call)
//!      │
//!      ▼
//! tools.rs      ← per-tool handlers; fans out via join_all
//!      │
//!      ▼
//! runner.rs     ← OpencodeRunner trait (SubprocessRunner / ScriptedRunner)
//!      │
//!      ▼
//! events.rs     ← JSON event types parsed from `opencode run --format json`
//! registry.rs   ← in-flight task map (cancel tokens + submitted_at)
//! ```
//!
//! Subprocess pattern (`kill_on_drop` + `pre_exec setpgid(0,0)`) is
//! copied verbatim from `src/tool/bash.rs` so cancellation can reach
//! the entire opencode subtree via `killpg`.

pub mod events;
pub mod protocol;
pub mod registry;
pub mod runner;
pub mod tools;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::mcp_opencode::runner::{OpencodeRunner, SubprocessConfig, SubprocessRunner};

/// Run the MCP server on stdio until EOF. `binary` is the path to the
/// `opencode` binary; `server_version` is the value embedded in the
/// `initialize` response's `serverInfo.version`.
#[allow(clippy::missing_errors_doc)]
pub async fn run(binary: PathBuf, server_version: Arc<String>) -> Result<()> {
    let runner: Arc<dyn OpencodeRunner> = Arc::new(SubprocessRunner::new(SubprocessConfig {
        binary: binary.clone(),
    }));
    protocol::run(runner, binary, server_version)
        .await
        .context("mcp-opencode protocol loop")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_exports_public_surface() {
        // Compile-time check: every public type is re-exported from
        // the module root so `crate::mcp_opencode::X` works.
        let _: fn() -> Vec<serde_json::Value> = tools::tool_schemas;
        let _ = protocol::PROTOCOL_VERSION;
    }
}
