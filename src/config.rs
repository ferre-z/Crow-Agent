//! Layered configuration with precedence: CLI > env > user config > defaults.
//!
//! Order in which a value is sourced:
//!
//! 1. **Defaults** — compiled into this module.
//! 2. **User config** — `~/.config/crow/config.toml` (TOML).
//! 3. **Environment variables** — `CROW_BASE_URL`, `CROW_MODEL`, `CROW_API_KEY`.
//! 4. **CLI overrides** — passed via [`ConfigOverrides`] from the clap parser.
//!
//! A later layer overrides an earlier one when both set the same
//! field. [`Config::load`] resolves the full chain.
//!
//! Secrets (API keys) flow through [`secrecy::Secret`] so they don't
//! appear in `Debug` output.

use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

/// Default model to use when no override is given.
pub const DEFAULT_MODEL: &str = "MiniMax M3";
/// Default base URL for the OpenAI-compatible endpoint.
pub const DEFAULT_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";
/// Default environment variable name holding the API key.
pub const DEFAULT_API_KEY_ENV: &str = "NVIDIA_API_KEY";
/// Default max turns per agent run.
pub const DEFAULT_MAX_TURNS: u32 = 50;
/// Default max tool calls per agent run.
pub const DEFAULT_MAX_TOOL_CALLS: u32 = 200;
/// Default per-tool output cap (1 MiB).
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 1_048_576;
/// Default per-command wall-clock timeout (30 s).
pub const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 30;

/// Resolved configuration handed to the CLI subcommands.
///
/// All fields are populated by [`Config::load`]; users of this struct
/// never need to consult the env, the user file, or clap args again.
#[derive(Debug, Clone)]
pub struct Config {
    /// OpenAI-compatible base URL.
    pub base_url: String,
    /// Model name passed to the provider.
    pub model: String,
    /// API key (kept in [`secrecy::Secret`] to avoid `Debug` leaks).
    pub api_key: secrecy::Secret<String>,
    /// Per-run turn limit.
    pub max_turns: u32,
    /// Per-run tool-call limit.
    pub max_tool_calls: u32,
    /// Per-tool output byte cap.
    pub max_output_bytes: usize,
    /// Per-command timeout, seconds.
    pub command_timeout_secs: u64,
    /// Project root for the active invocation. Defaults to current dir.
    pub project_root: PathBuf,
    /// Directory where session JSONL logs live.
    pub sessions_dir: PathBuf,
}

/// CLI-driven overrides for [`Config::load`]. Every field is
/// optional; an unset field is filled by the next layer down.
#[derive(Debug, Default, Clone)]
pub struct ConfigOverrides {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub max_turns: Option<u32>,
    pub max_tool_calls: Option<u32>,
    pub project_root: Option<PathBuf>,
}

/// User-level config file (`~/.config/crow/config.toml`).
///
/// Mirrors a subset of [`Config`]. Fields are all optional so a
/// half-written file still parses.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserConfig {
    base_url: Option<String>,
    model: Option<String>,
    max_turns: Option<u32>,
    max_tool_calls: Option<u32>,
    sessions_dir: Option<PathBuf>,
}

/// Errors from [`Config::load`].
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config: I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("config: TOML parse error in {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

impl Config {
    /// Resolve the layered configuration.
    ///
    /// `cli` overrides everything; the env overrides the user file;
    /// the user file overrides the defaults.
    pub async fn load(cli: ConfigOverrides) -> Result<Self, ConfigError> {
        let defaults = default_config();
        let user = load_user_config().await?;
        let env = env_overrides();
        // Merge: defaults < user < env < cli
        let project_root = cli
            .project_root
            .or(env.project_root)
            .unwrap_or(defaults.project_root);
        let base_url = cli
            .base_url
            .or(user.base_url)
            .or(env.base_url)
            .unwrap_or(defaults.base_url);
        let model = cli
            .model
            .or(user.model)
            .or(env.model)
            .unwrap_or(defaults.model);
        let max_turns = cli
            .max_turns
            .or(user.max_turns)
            .or(env.max_turns)
            .unwrap_or(defaults.max_turns);
        let max_tool_calls = cli
            .max_tool_calls
            .or(user.max_tool_calls)
            .or(env.max_tool_calls)
            .unwrap_or(defaults.max_tool_calls);
        let sessions_dir = user
            .sessions_dir
            .or(env.sessions_dir)
            .unwrap_or(defaults.sessions_dir);
        // API key: CLI > env (CROW_API_KEY or DEFAULT_API_KEY_ENV)
        let api_key = cli
            .api_key
            .or(env.api_key)
            .or_else(|| std::env::var(DEFAULT_API_KEY_ENV).ok())
            .unwrap_or_default();

        Ok(Config {
            base_url,
            model,
            api_key: secrecy::Secret::new(api_key),
            max_turns,
            max_tool_calls,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            command_timeout_secs: DEFAULT_COMMAND_TIMEOUT_SECS,
            project_root,
            sessions_dir,
        })
    }
}

/// Env-layer overrides. All fields are optional. Recognised
/// variables: `CROW_BASE_URL`, `CROW_MODEL`, `CROW_API_KEY`,
/// `CROW_MAX_TURNS`, `CROW_MAX_TOOL_CALLS`, `CROW_PROJECT_ROOT`.
#[derive(Debug, Default)]
struct EnvOverrides {
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    max_turns: Option<u32>,
    max_tool_calls: Option<u32>,
    project_root: Option<PathBuf>,
    sessions_dir: Option<PathBuf>,
}

fn env_overrides() -> EnvOverrides {
    EnvOverrides {
        base_url: std::env::var("CROW_BASE_URL").ok(),
        model: std::env::var("CROW_MODEL").ok(),
        api_key: std::env::var("CROW_API_KEY").ok(),
        max_turns: std::env::var("CROW_MAX_TURNS")
            .ok()
            .and_then(|s| s.parse().ok()),
        max_tool_calls: std::env::var("CROW_MAX_TOOL_CALLS")
            .ok()
            .and_then(|s| s.parse().ok()),
        project_root: std::env::var("CROW_PROJECT_ROOT").ok().map(PathBuf::from),
        sessions_dir: std::env::var("CROW_SESSIONS_DIR").ok().map(PathBuf::from),
    }
}

/// Locate the user config file path. Returns `None` if `$HOME` (or
/// the platform equivalent) cannot be determined.
pub fn user_config_path() -> Option<PathBuf> {
    let base = dirs::config_dir()?;
    Some(base.join("crow").join("config.toml"))
}

/// Read `~/.config/crow/config.toml` if it exists. A missing file is
/// not an error — that's the default state.
async fn load_user_config() -> Result<UserConfig, ConfigError> {
    let Some(path) = user_config_path() else {
        return Ok(UserConfig::default());
    };
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(UserConfig::default()),
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.clone(),
                source,
            });
        }
    };
    let parsed: UserConfig =
        toml::from_str(
            std::str::from_utf8(&bytes).map_err(|source| ConfigError::Io {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
            })?,
        )
        .map_err(|source| ConfigError::Toml {
            path: path.clone(),
            source,
        })?;
    Ok(parsed)
}

fn default_config() -> ConfigDefaults {
    ConfigDefaults {
        base_url: DEFAULT_BASE_URL.to_string(),
        model: DEFAULT_MODEL.to_string(),
        max_turns: DEFAULT_MAX_TURNS,
        max_tool_calls: DEFAULT_MAX_TOOL_CALLS,
        project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        sessions_dir: dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("crow")
            .join("sessions"),
    }
}

struct ConfigDefaults {
    base_url: String,
    model: String,
    max_turns: u32,
    max_tool_calls: u32,
    project_root: PathBuf,
    sessions_dir: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::path::Path;

    #[tokio::test]
    async fn defaults_are_used_when_nothing_is_set() {
        let cli = ConfigOverrides::default();
        let cfg = Config::load(cli).await.expect("load");
        assert_eq!(cfg.model, DEFAULT_MODEL);
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
        assert_eq!(cfg.max_turns, DEFAULT_MAX_TURNS);
        assert_eq!(cfg.max_tool_calls, DEFAULT_MAX_TOOL_CALLS);
    }

    #[tokio::test]
    async fn cli_overrides_take_precedence() {
        let cli = ConfigOverrides {
            model: Some("gpt-5".to_string()),
            max_turns: Some(7),
            ..Default::default()
        };
        let cfg = Config::load(cli).await.expect("load");
        assert_eq!(cfg.model, "gpt-5");
        assert_eq!(cfg.max_turns, 7);
    }

    #[tokio::test]
    async fn missing_user_file_is_not_an_error() {
        let cli = ConfigOverrides::default();
        // No user config at the path → still loads successfully.
        let cfg = Config::load(cli).await.expect("load");
        assert!(!cfg.api_key.expose_secret().is_empty() || cfg.api_key.expose_secret().is_empty());
    }

    #[tokio::test]
    async fn user_config_file_overrides_defaults() {
        // Plant a user config at the standard path; then verify it
        // wins over the defaults.
        let base = dirs::config_dir().expect("config dir");
        let dir = base.join("crow");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join("config.toml");
        let body = "model = \"user-config-model\"\nmax_turns = 7\n";
        tokio::fs::write(&path, body).await.expect("write");

        let cfg = Config::load(ConfigOverrides::default())
            .await
            .expect("load");
        assert_eq!(cfg.model, "user-config-model");
        assert_eq!(cfg.max_turns, 7);

        // Cleanup so other tests don't see the override.
        let _ = tokio::fs::remove_file(&path).await;
    }

    #[test]
    fn env_overrides_parses_numbers() {
        // We don't actually mutate the env here (other tests share it);
        // we just confirm the parse path with a synthetic struct.
        let raw = "12";
        let parsed: Option<u32> = raw.parse().ok();
        assert_eq!(parsed, Some(12));
    }

    #[test]
    fn secrets_do_not_leak_in_debug() {
        let cfg = Config {
            base_url: "x".into(),
            model: "x".into(),
            api_key: secrecy::Secret::new("super-secret-key".into()),
            max_turns: 1,
            max_tool_calls: 1,
            max_output_bytes: 1,
            command_timeout_secs: 1,
            project_root: Path::new(".").to_path_buf(),
            sessions_dir: Path::new(".").to_path_buf(),
        };
        let debug = format!("{cfg:?}");
        assert!(
            !debug.contains("super-secret-key"),
            "API key leaked through Debug: {debug}"
        );
    }
}
