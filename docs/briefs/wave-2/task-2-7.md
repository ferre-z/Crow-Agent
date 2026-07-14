### Task 2.7 — Headless `crow exec`, `crow sessions`, `crow --resume`

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs` (dispatch to CLI subcommands)

**Spec references:** v0 spec §15 (CLI behavior), §18 (acceptance criterion 6: "Closing and reopening the program resumes completed conversation history").

**Interfaces (exact):**

```rust
// src/cli.rs
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "crow", version, about = "Small autonomous coding agent")]
pub struct Cli {
    /// Optional path to the project root (defaults to cwd).
    pub path: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub subcommand: Option<Subcommand>,
}

#[derive(Subcommand, Debug)]
pub enum Subcommand {
    /// Run a single task against the agent and exit.
    Exec {
        /// The task description (a free-form string).
        task: String,
        /// Optional session ID to resume into. If absent, a new session is created.
        #[arg(long)]
        session: Option<String>,
    },
    /// List all sessions in the configured sessions directory.
    Sessions {
        /// Optional path to the sessions directory.
        #[arg(long)]
        directory: Option<std::path::PathBuf>,
    },
    /// Resume a session by ID, replay history, then run interactively.
    Resume {
        /// The session ID (ULID prefix or full).
        id: String,
    },
    /// Validate config, key, and endpoint. Does NOT consume API quota
    /// unless --live is passed.
    Doctor {
        /// Make a minimal authenticated model request to confirm the
        /// endpoint is reachable.
        #[arg(long)]
        live: bool,
    },
}
```

**Behavior:**

- `crow` (no args) → prints version and exits. (Wave 1 baseline.)
- `crow exec "fix the bug"` → open a new session, build Agent with default provider, run `agent.submit(user_msg)`, print resulting events to stdout (one per line, JSON), exit with code 0.
- `crow sessions` → read the sessions directory (default `~/.local/share/crow/sessions/`), print a table: `id, started_at, last_status, message_count`. Newest first.
- `crow --resume ID` → load the session, replay its history to stdout, then enter `crow exec` mode (interactive, future TUI work).
- `crow doctor` → check the config file loads, the API key env var is set, the provider can be constructed. With `--live`, also make a 1-token request to confirm the endpoint.

**Acceptance:**
- 6+ unit tests in `src/cli.rs` (clap subcommand parsing):
  1. `crow` (no args) → version
  2. `crow exec "task"` → Exec subcommand with task
  3. `crow exec "task" --session <id>` → Exec with session
  4. `crow sessions` → Sessions subcommand
  5. `crow --resume <id>` → Resume subcommand
  6. `crow doctor --live` → Doctor with live flag
  7. `crow --version` → exits 0 with "crow 0.1.0"
- 2+ integration tests in `tests/cli.rs` using `assert_cmd`:
  1. `crow --version` exits 0
  2. `crow sessions` on an empty dir prints a header and exits 0
- Gate: clean.

**Forbidden:**
- No `unsafe`.
- No `unwrap`/`expect` in library code (use `?` and the `CliError` enum).
- No blocking IO in async fn.

**Dependencies:** `assert_cmd` already in dev-deps.
