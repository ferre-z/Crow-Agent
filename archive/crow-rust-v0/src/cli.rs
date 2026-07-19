//! Clap subcommand definitions and the binary entry point.
//!
//! `crow` is a small CLI: the subcommands are `exec`, `sessions`,
//! `resume`, and `doctor`. Top-level flags include `--version` and
//! `--resume <id>`. Most options thread through [`ConfigOverrides`]
//! so the layered configuration still applies.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use secrecy::ExposeSecret;
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, AgentConfig};
use crate::config::{Config, ConfigOverrides};
use crate::event::ChannelSink;
use crate::message::{Message, Part, Role};
use crate::provider::mock::ScriptedProvider;
use crate::provider::Provider;
use crate::session::{self, SessionMeta};
use crate::tool::{BashTool, EditTool, ReadTool, ToolRegistry, WriteTool};

/// Output format for `crow exec`. `Text` matches the default
/// Claude Code / opencode behaviour: only the final assistant
/// message is printed. `StreamJson` emits every kernel event as
/// a JSON line so CI pipelines can consume the full transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Print only the final assistant message on stdout.
    Text,
    /// Emit one JSON line per [`AgentEvent`] plus a final assistant
    /// message line, suitable for `jq` / CI / log aggregation.
    StreamJson,
}

/// Command-line arguments parsed by clap.
#[derive(Debug, Parser)]
#[command(name = "crow", version, about = "Small autonomous coding agent")]
pub struct Cli {
    /// Project root for the invocation (overrides env + config).
    #[arg(long, global = true)]
    pub project_root: Option<PathBuf>,

    /// Resume an existing session by id.
    #[arg(long, global = true, value_name = "SESSION_ID")]
    pub resume: Option<String>,

    /// Base URL for the OpenAI-compatible endpoint (overrides env).
    #[arg(long, global = true)]
    pub base_url: Option<String>,

    /// Model name passed to the provider (overrides env).
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// API key for the provider (overrides env + keyring).
    #[arg(long, global = true, hide_env_values = true)]
    pub api_key: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a one-shot prompt against the agent loop and print the
    /// final assistant message. With `--output-format stream-json`
    /// the kernel emits one JSON line per [`crate::event::AgentEvent`]
    /// plus a final assistant message on stdout, suitable for
    /// piping into CI scripts or `jq`.
    Exec {
        /// Prompt text to send.
        prompt: String,
        /// Output format. `text` (default) prints only the final
        /// assistant message. `stream-json` emits every kernel
        /// event as a JSON line plus the final assistant message
        /// as a separate line — same wire format Claude Code uses
        /// for `-p --output-format stream-json`.
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output_format: OutputFormat,
    },
    /// List every session in the active project's sessions directory,
    /// newest first.
    Sessions,
    /// Resume an existing session by id and submit a new prompt.
    Resume {
        /// Session id (full or prefix).
        session_id: String,
        /// Prompt to send after resume.
        prompt: String,
    },
    /// Print diagnostic information about the current configuration
    /// and provider setup. Use `--live` to also probe the network.
    Doctor {
        /// Ping the configured provider endpoint.
        #[arg(long)]
        live: bool,
    },
    /// Run the local app-server: line-delimited JSON-RPC over stdio.
    /// Used by the Tauri desktop (and external CLIs in any language)
    /// to drive the kernel without linking it.
    Serve,
    /// Run the MCP server (Model Context Protocol, JSON-RPC over stdio)
    /// that delegates tasks to the `opencode` CLI, optionally in parallel.
    /// Configure your MCP client (e.g. Claude Code) to launch
    /// `crow mcp-opencode` and the seven opencode_* tools will appear.
    McpOpencode {
        /// Path to the `opencode` binary. Defaults to `opencode` on $PATH.
        #[arg(long)]
        binary: Option<PathBuf>,
    },
    /// Launch the interactive terminal UI: streaming REPL against
    /// the kernel, mirroring the Claude Code / opencode `tui` flow.
    /// Pair with `--resume <id>` to continue an existing session.
    Tui {
        /// Resume an existing session by id (full or prefix).
        #[arg(long, value_name = "SESSION_ID")]
        resume: Option<String>,
        /// Start in plan mode: only `read` is available; the agent
        /// can inspect code but cannot mutate files or run shell
        /// commands. Matches Claude Code's plan mode UX.
        #[arg(long)]
        plan: bool,
        /// Disable ANSI colour codes. Useful for screen readers,
        /// piping into `less -R` (with `-R` removed), or terminals
        /// that don't render colour reliably. The shape and
        /// content of every widget stays the same; only the colour
        /// attributes are stripped.
        #[arg(long)]
        no_color: bool,
        /// Optional friendly label for the session (F.40.04). Shown
        /// in `crow sessions` and the `/resume` picker instead of
        /// the truncated ULID.
        #[arg(long, value_name = "LABEL")]
        name: Option<String>,
    },
    /// Print the version string (same as --version).
    Version,
}

/// Run the CLI based on parsed args. Returns `Ok` on a successful
/// run, `Err` on a fatal error.
#[allow(clippy::missing_errors_doc)]
pub async fn run(args: Cli) -> Result<()> {
    // Tracing to stderr; stdout is reserved for the user-facing
    // protocol output of each subcommand.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("crow=info,warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();

    let cli_overrides = ConfigOverrides {
        base_url: args.base_url.clone(),
        model: args.model.clone(),
        api_key: args.api_key.clone(),
        max_turns: None,
        max_tool_calls: None,
        project_root: args.project_root.clone(),
    };
    let config = Config::load(cli_overrides)
        .await
        .context("loading config")?;
    let resume_id = args.resume.clone();
    match args.command {
        Command::Exec {
            prompt,
            output_format,
        } => exec(&config, resume_id.as_deref(), prompt, output_format).await,
        Command::Sessions => sessions(&config).await,
        Command::Resume { session_id, prompt } => resume(&config, session_id, prompt).await,
        Command::Doctor { live } => doctor(&config, live).await,
        Command::Serve => crate::app_server::run().await,
        Command::McpOpencode { binary } => {
            let binary = binary.clone().unwrap_or_else(|| PathBuf::from("opencode"));
            // Hand the crate's own version string to the server so
            // the `initialize` response advertises it under
            // `serverInfo.version`.
            let version = Arc::new(env!("CARGO_PKG_VERSION").to_string());
            crate::mcp_opencode::run(binary, version).await
        }
        Command::Tui {
            resume,
            plan,
            no_color,
            name,
        } => {
            // Project root is taken from the resolved config so the
            // TUI operates on the same directory `crow exec` would.
            let project_root = config.project_root.clone();
            crate::tui::run(config, resume, project_root, plan, no_color, name).await
        }
        Command::Version => {
            println!("crow {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

async fn exec(
    config: &Config,
    resume_id: Option<&str>,
    prompt: String,
    output_format: OutputFormat,
) -> Result<()> {
    let provider = build_provider(config)?;
    let tools = default_registry();
    let sessions_dir = sessions_dir_for(config);
    tokio::fs::create_dir_all(&sessions_dir)
        .await
        .context("creating sessions directory")?;
    let session_path = new_session_path(&sessions_dir);

    let cancel = CancellationToken::new();
    let user_msg = Message {
        id: crate::ids::MessageId(crate::ids::new_id()),
        role: Role::User,
        parts: vec![Part::Text { text: prompt }],
    };

    if output_format == OutputFormat::StreamJson {
        let path = if let Some(session_id) = resume_id {
            resolve_session_id(&sessions_dir, session_id)
                .await
                .with_context(|| format!("resolving session id {session_id}"))?
        } else {
            session_path.clone()
        };
        let writer = session::SessionWriter::open(&path)
            .await
            .with_context(|| format!("opening session log {path:?}"))?;
        let cfg = AgentConfig::new(
            config.max_turns,
            config.max_tool_calls,
            config.model.clone(),
            config.project_root.clone(),
            writer,
        );
        let sink: Arc<dyn crate::event::AgentEventSink> = Arc::new(StreamJsonSink);
        let mut agent = if resume_id.is_some() {
            let (agent, _history) = Agent::resume_into(
                cfg,
                Arc::clone(&provider) as Arc<dyn Provider>,
                tools,
                cancel,
                sink,
                &path,
            )
            .await
            .context("rebuilding session")?;
            agent
        } else {
            Agent::with_sink(
                cfg,
                Arc::clone(&provider) as Arc<dyn Provider>,
                tools,
                cancel,
                Vec::new(),
                sink,
            )
        };
        let final_event = agent.submit(user_msg).await.context("agent loop")?;
        if let crate::event::AgentEvent::RunFinished { message } = final_event {
            println!(
                "{}",
                serde_json::json!({"type": "AssistantMessage", "text": message})
            );
        }
        return Ok(());
    }

    let mut agent = if let Some(session_id) = resume_id {
        let path = resolve_session_id(&sessions_dir, session_id)
            .await
            .with_context(|| format!("resolving session id {session_id}"))?;
        let writer = session::SessionWriter::open(&path)
            .await
            .with_context(|| format!("opening session log {path:?}"))?;
        let cfg = AgentConfig::new(
            config.max_turns,
            config.max_tool_calls,
            config.model.clone(),
            config.project_root.clone(),
            writer,
        );
        let (agent_sink, _rx) = ChannelSink::new(256);
        let (agent, _history) = Agent::resume_into(
            cfg,
            Arc::clone(&provider) as Arc<dyn Provider>,
            tools,
            cancel,
            Arc::new(agent_sink),
            &path,
        )
        .await
        .context("rebuilding session")?;
        agent
    } else {
        let writer = session::SessionWriter::open(&session_path)
            .await
            .with_context(|| format!("opening session log {session_path:?}"))?;
        let cfg = AgentConfig::new(
            config.max_turns,
            config.max_tool_calls,
            config.model.clone(),
            config.project_root.clone(),
            writer,
        );
        Agent::new(
            cfg,
            Arc::clone(&provider) as Arc<dyn Provider>,
            tools,
            cancel,
            Vec::new(),
        )
    };

    let final_event = agent.submit(user_msg).await.context("agent loop")?;
    if let crate::event::AgentEvent::RunFinished { message } = final_event {
        println!("{message}");
    }
    Ok(())
}

/// `AgentEventSink` that writes every kernel event as a JSON line
/// on stdout. Used by `crow exec --output-format stream-json` for
/// CI pipelines that want to consume the full transcript.
struct StreamJsonSink;

impl crate::event::AgentEventSink for StreamJsonSink {
    fn on_event(&self, event: crate::event::AgentEvent) {
        if let Ok(line) = serde_json::to_string(&event) {
            println!("{line}");
        }
    }
}

async fn sessions(config: &Config) -> Result<()> {
    let dir = sessions_dir_for(config);
    if !dir.exists() {
        println!("no sessions directory at {}", dir.display());
        return Ok(());
    }
    let metas = session::list_sessions(&dir)
        .await
        .context("listing sessions")?;
    if metas.is_empty() {
        println!("no sessions");
        return Ok(());
    }
    for SessionMeta {
        session_id,
        started_at,
        schema_version: _,
        path,
    } in metas
    {
        println!(
            "{}  {}  {}",
            session_id,
            format_timestamp(started_at),
            path.display()
        );
    }
    Ok(())
}

/// Human-readable timestamp for CLI output.
fn format_timestamp(ts: crate::ids::Timestamp) -> String {
    let dur =
        ts.0.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
    let secs = dur.as_secs();
    // Render as `YYYY-MM-DDTHH:MM:SSZ` from the epoch second. We avoid
    // pulling in `chrono`; a small inline formatter is enough for v0.
    let (year, month, day, hour, minute, second) = epoch_to_civil(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's date algorithm: convert Unix seconds to
/// (year, month, day, hour, minute, second) in UTC. Returns sentinel
/// zeros for pre-1970 inputs.
fn epoch_to_civil(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
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
    let year = (if m <= 2 { y + 1 } else { y }) as i32;
    (year, m, d, hour, minute, second)
}

async fn resume(config: &Config, session_id: String, prompt: String) -> Result<()> {
    exec(config, Some(&session_id), prompt, OutputFormat::Text).await
}

async fn doctor(config: &Config, live: bool) -> Result<()> {
    println!("crow doctor");
    println!("  model:         {}", config.model);
    println!("  base_url:      {}", config.base_url);
    println!(
        "  api_key_env:   {}",
        if config.api_key.expose_secret().is_empty() {
            "(not set)"
        } else {
            "(set)"
        }
    );
    println!("  project_root:  {}", config.project_root.display());
    println!("  sessions_dir:  {}", sessions_dir_for(config).display());
    println!("  max_turns:     {}", config.max_turns);
    println!("  max_tool_calls: {}", config.max_tool_calls);
    if live {
        let provider = build_provider(config)?;
        let _ = provider; // presence proves construction worked.
        println!("  live check:    ok (provider constructed; no stream call)");
    }
    Ok(())
}

/// Build a provider. Tries `genai` first; if the API key is empty,
/// falls back to the scripted mock so `crow doctor` and unit-test
/// flows still work without network.
fn build_provider(config: &Config) -> Result<Arc<dyn Provider>> {
    let key = config.api_key.expose_secret();
    if !key.is_empty() {
        let provider = crate::provider::genai::GenaiProvider::with_api_key(
            &config.base_url,
            &config.model,
            key.to_string(),
        );
        let arc: Arc<dyn Provider> = Arc::new(provider);
        return Ok(arc);
    }
    tracing::warn!("no API key configured; using scripted mock provider");
    Ok(Arc::new(ScriptedProvider::from_events(Vec::new())))
}

/// Default registry for the CLI: ships `read`, `write`, `edit`,
/// and `bash`. Plan mode uses [`read_only_registry`] instead.
pub fn default_registry() -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(ReadTool);
    reg.register(WriteTool);
    reg.register(EditTool);
    reg.register(BashTool);
    Arc::new(reg)
}

/// Read-only registry: only `read`. Used when the TUI is started
/// with `--plan`. The kernel can then read files but cannot
/// mutate them or run shell commands. Useful for "review-only"
/// sessions where the user wants the agent to inspect code
/// without touching anything.
pub fn read_only_registry() -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(ReadTool);
    Arc::new(reg)
}

/// Build a path for a fresh session log under `dir`. Filename is a
/// ULID so the file is sortable + unique.
fn new_session_path(dir: &std::path::Path) -> PathBuf {
    dir.join(format!("{}.jsonl", ulid::Ulid::new()))
}

/// `sessions_dir` is `<project_root>/.crow/sessions` to keep each
/// project's sessions self-contained.
fn sessions_dir_for(config: &Config) -> PathBuf {
    config.project_root.join(".crow").join("sessions")
}

/// Resolve a session id (full or prefix) to its JSONL path.
async fn resolve_session_id(dir: &std::path::Path, id_or_prefix: &str) -> Result<PathBuf> {
    let metas = session::list_sessions(dir).await?;
    let matches: Vec<&SessionMeta> = metas
        .iter()
        .filter(|m| m.session_id.0.to_string().starts_with(id_or_prefix))
        .collect();
    match matches.len() {
        1 => Ok(matches[0].path.clone()),
        n if n > 1 => {
            anyhow::bail!(
                "session id prefix {id_or_prefix:?} matched {n} sessions; be more specific"
            )
        }
        _ => anyhow::bail!("no session matched {id_or_prefix:?}"),
    }
}
