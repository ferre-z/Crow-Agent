### Task 4.5 — Policy persistence + project config

**Files:**
- Create: `src/policy/load.rs` (load/save project + user policy)
- Create: `src/policy/file.rs` (a file-backed policy: `~/.config/crow/policy.toml`)
- Modify: `src/policy.rs` (re-export the loader)
- Create: `examples/policy.toml` (the default policy file)
- Modify: `Cargo.toml` (add `toml = "0.8"`, `dirs = "5"`)

**Why this exists:** users want a way to say "always deny `rm -rf`" without writing Rust. A TOML config file at `~/.config/crow/policy.toml` (or `<project_root>/.crow/policy.toml` for project-scoped rules) is the right shape.

**Spec references:** spec §4 (project-root confinement) — the policy file location must respect the project root.

**Format:**

```toml
# ~/.config/crow/policy.toml (user-wide, applies to all projects)
default = "allow"  # or "deny" or "ask"

[[rule]]
tool = "bash"
when.command_starts_with = "rm -rf"
decision = "deny"
reason = "destructive"

[[rule]]
tool = "write"
when.path_matches = "**/secrets/**"
decision = "deny"

# <project_root>/.crow/policy.toml (project-scoped, takes precedence)
default = "ask"

[[rule]]
tool = "bash"
when.command_starts_with = "cargo test"
decision = "allow"
reason = "test runner is safe"
```

**Interfaces (exact):**

```rust
// src/policy/load.rs
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::policy::{Decision, SharedPolicy, ApprovalPolicy};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicyConfig {
    #[serde(default)]
    pub default: PolicyDefault,
    #[serde(default)]
    pub rule: Vec<PolicyRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PolicyDefault {
    #[default]
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub tool: String,
    #[serde(default)]
    pub when: PolicyWhen,
    pub decision: PolicyDefault,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct PolicyWhen {
    /// Bash command starts with this string.
    #[serde(default)]
    pub command_starts_with: Option<String>,
    /// Tool path matches a glob (e.g. `**/secrets/**`).
    #[serde(default)]
    pub path_matches: Option<String>,
    /// Tool name exact match.
    #[serde(default)]
    pub tool_is: Option<String>,
}

pub fn load_from(project_root: &Path) -> Result<PolicyConfig, PolicyLoadError> {
    // Load ~/.config/crow/policy.toml first (if it exists), then
    // <project_root>/.crow/policy.toml (overrides). Merge.
}

pub fn build_policy(config: &PolicyConfig) -> SharedPolicy;
```

**`FilePolicy` implementation:**

```rust
// src/policy/file.rs
pub struct FilePolicy {
    config: PolicyConfig,
}

#[async_trait]
impl ApprovalPolicy for FilePolicy {
    async fn evaluate(&self, tool: &str, args: &Value, ctx: &ToolContext) -> Decision {
        // Match each rule in order. First match wins.
        // If no rule matches, return Decision::Ask (resolve to default).
        // ...
    }
}
```

**Procedure:**
1. Add `toml = "0.8"` and `dirs = "5"` to Cargo.toml.
2. Write the `PolicyConfig` types with `serde` derives.
3. Write `load_from(project_root)` that reads from `~/.config/crow/policy.toml` (via `dirs::config_dir()`) and `<project_root>/.crow/policy.toml`. The project file wins on conflict (later in the chain overrides earlier).
4. Write `FilePolicy` that matches rules.
5. Write `examples/policy.toml` with a sane default (deny `rm -rf`, allow common read operations).
6. CLI subcommand `crow policy show <project_root>` prints the effective policy.

**Acceptance:**
- 8+ unit tests in `policy/load.rs` and `policy/file.rs`:
  - Default policy from a missing file is `Allow` (matches v0 spec)
  - User config is loaded
  - Project config overrides user config
  - Project config can be loaded in isolation
  - `command_starts_with` matches bash commands
  - `path_matches` uses glob (use `globset` crate if needed; default to `String::contains` for v0)
  - `tool_is` matches tool names exactly
  - First matching rule wins
- 2+ integration tests:
  - `crow policy show` on a project with no policy file prints `default = allow`
  - `crow policy show` on a project with a policy file prints the merged policy
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` clean

**Forbidden:**
- No `unwrap`/`expect` in library code.
- No silent failures: missing file → default, malformed file → typed error.
- No new dependency for glob matching in v0; use `String::contains` for `path_matches`. (Glob support is a v1 feature.)

**Dependency:** `toml = "0.8"`, `dirs = "5"`.
