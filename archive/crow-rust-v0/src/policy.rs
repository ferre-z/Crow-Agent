//! Hot-pluggable approval policy for tool calls.
//!
//! Every tool the agent is about to execute is gated by an
//! [`ApprovalPolicy`]. The policy returns one of three
//! [`Decision`]s:
//!
//! - [`Decision::Allow`] — execute the tool.
//! - [`Decision::Ask`]   — pause and wait for a human response (the
//!   desktop app fills this; the CLI times out and surfaces the ask).
//! - [`Decision::Deny`]  — refuse; the tool gets a synthetic error
//!   result so the model can react.
//!
//! The default policy for v0 is `DefaultPolicy { read: Allow, others:
//! Ask }`. The desktop overrides per-session via `PolicySet`.
//!
//! ## Asking the user
//!
//! `Ask` blocks on an async channel — the policy implementation
//! receives the call and the cancel token, parks until the channel
//! resolves or the token fires. The kernel implementation in
//! [`AskAwaitable`] is the canonical mechanism: pass an `AskAwaitable`
//! to [`AgentConfig`] so the agent loop can satisfy a pending Ask
//! by sending a `Decision` through the channel from outside (e.g.
//! the Tauri IPC layer).

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::message::Part;
use crate::tool::ToolCall;

/// Outcome of [`ApprovalPolicy::decide`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Execute the tool.
    Allow,
    /// Block until a human (or the policy layer) responds. The
    /// `AskAwaitable` registered on the agent resolves with one of
    /// `Allow`, `Deny`, or another `Ask`.
    Ask {
        /// Stable id the resolver uses to identify the pending ask.
        ask_id: String,
    },
    /// Refuse the call. The tool sees a synthetic error.
    Deny {
        /// Human-readable reason (e.g. "policy denied bash by default").
        reason: String,
    },
}

/// The hot-pluggable approval policy.
#[async_trait::async_trait]
pub trait ApprovalPolicy: Send + Sync {
    /// Decide whether `call` may execute. `history` is the
    /// conversation so far, including any prior tool results; useful
    /// for policies that look at cumulative context.
    async fn decide(&self, call: &ToolCall, history: &[crate::message::Message]) -> Decision;
}

/// Built-in default: read-only tools are allowed, mutation tools
/// (write/edit/bash) require an Ask that auto-resolves to Allow
/// after a 30-second timeout if no human responds. v0 keeps the
/// v0-kernel "autonomous by default" semantic by skipping the wait
/// when no `AskAwaitable` is registered.
#[derive(Debug, Default, Clone)]
pub struct DefaultPolicy;

#[async_trait::async_trait]
impl ApprovalPolicy for DefaultPolicy {
    async fn decide(&self, call: &ToolCall, _history: &[crate::message::Message]) -> Decision {
        match call.name.as_str() {
            "read" => Decision::Allow,
            // write / edit / bash require an explicit ask. The
            // AskAwaitable resolves them; if no awaitable is wired
            // up (CLI without a TTY), the agent surfaces a typed
            // error instead of deadlocking.
            "write" | "edit" | "bash" => Decision::Ask {
                ask_id: format!("{}-{}", call.name, call.call_id),
            },
            // Unknown tools: deny by default. The model can still
            // learn that the tool exists once it's been registered;
            // unknown tools are a programming error, not an
            // attack surface to leave open.
            _ => Decision::Deny {
                reason: format!("policy denies unknown tool: {}", call.name),
            },
        }
    }
}

/// Compositional policy: layered rules. The first matching rule
/// wins. If no rule matches, the wrapped fallback policy decides.
///
/// Rules are loaded from `~/.config/crow/policy.toml` (see
/// [`RuleBasedPolicy::from_file`]).
pub struct RuleBasedPolicy {
    rules: Vec<CompiledRule>,
    fallback: Arc<dyn ApprovalPolicy>,
}

impl std::fmt::Debug for RuleBasedPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleBasedPolicy")
            .field("rules", &self.rules)
            .field("fallback", &"<dyn ApprovalPolicy>")
            .finish()
    }
}

/// One named regex matcher against a tool call's args. The `arg`
/// field is a JSON-pointer-ish key into `ToolCall::args` (currently
/// only top-level keys are supported; nested paths land in a
/// follow-up slice). `regex` is the source string of a `regex::Regex`
/// compiled at rule-load time.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ArgPattern {
    /// Name of the argument to match (e.g. `"command"` for bash,
    /// `"path"` for write/edit/read).
    pub arg: String,
    /// Regex source. Compiled once when the rule is loaded; load
    /// fails if the regex is invalid.
    pub regex: String,
}

/// Compiled form of [`PolicyRule`]. Built from the on-disk form by
/// [`compile_rule`]; the live policy stores these so we never
/// recompile on every `decide` call.
#[derive(Debug)]
pub struct CompiledRule {
    pub tool: String,
    pub arg_patterns: Vec<(String, regex::Regex)>,
    pub command_starts_with: Option<String>,
    pub decision: RuleDecision,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PolicyRule {
    /// Tool name this rule matches. Use `"*"` to match any tool.
    pub tool: String,
    /// If set, match `bash` tool calls whose `command` starts with
    /// this substring (e.g. `"rm -rf"`). Backward-compatible
    /// substring matcher; prefer `arg_patterns` for new rules.
    #[serde(default)]
    pub command_starts_with: Option<String>,
    /// If non-empty, match the rule only if every `(arg, regex)`
    /// pair matches the corresponding argument on the tool call.
    /// Regex syntax is `regex::Regex`'s default flavour.
    #[serde(default)]
    pub arg_patterns: Vec<ArgPattern>,
    /// The decision if this rule matches.
    pub decision: RuleDecision,
}

/// Compile a [`PolicyRule`] into its runtime form. Returns
/// `Err(reason)` on invalid regex so the operator sees a clear
/// error at rule load time, not at the first call.
pub fn compile_rule(rule: PolicyRule) -> Result<CompiledRule, String> {
    let mut compiled = Vec::with_capacity(rule.arg_patterns.len());
    for ap in rule.arg_patterns {
        let re = regex::Regex::new(&ap.regex)
            .map_err(|e| format!("invalid regex for arg '{}': {}", ap.arg, e))?;
        compiled.push((ap.arg, re));
    }
    Ok(CompiledRule {
        tool: rule.tool,
        arg_patterns: compiled,
        command_starts_with: rule.command_starts_with,
        decision: rule.decision,
    })
}

/// One of the rule's three decisions. Same shape as [`Decision`]
/// minus the `Ask` channel — a rule either allows, denies, or
/// re-routes to the fallback.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleDecision {
    #[default]
    Allow,
    Deny,
    Fallback,
}

impl RuleBasedPolicy {
    /// Build a rule-based policy that defers to `fallback` when no
    /// rule matches. Invalid regexes in `arg_patterns` are dropped
    /// with a warning rather than failing the whole load — partial
    /// rules are better than no rules.
    pub fn new(rules: Vec<PolicyRule>, fallback: Arc<dyn ApprovalPolicy>) -> Self {
        let rules = rules
            .into_iter()
            .filter_map(|r| match compile_rule(r) {
                Ok(c) => Some(c),
                Err(e) => {
                    tracing::warn!("policy rule dropped: {e}");
                    None
                }
            })
            .collect();
        Self { rules, fallback }
    }

    /// Load `~/.config/crow/policy.toml`. A missing file is not an
    /// error — it returns an empty rule list, deferring entirely to
    /// the fallback. Invalid TOML is.
    pub fn from_file(fallback: Arc<dyn ApprovalPolicy>) -> Result<Self, PolicyError> {
        let path = policy_path().ok_or(PolicyError::NoHome)?;
        let rules = match std::fs::read_to_string(&path) {
            Ok(s) if s.trim().is_empty() => Vec::new(),
            Ok(s) => {
                let parsed: PolicyFile =
                    toml::from_str(&s).map_err(|source| PolicyError::Toml {
                        path: path.clone(),
                        source,
                    })?;
                parsed.rules
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(source) => {
                return Err(PolicyError::Io {
                    path: path.clone(),
                    source,
                })
            }
        };
        Ok(Self::new(rules, fallback))
    }
}

#[async_trait::async_trait]
impl ApprovalPolicy for RuleBasedPolicy {
    async fn decide(&self, call: &ToolCall, history: &[crate::message::Message]) -> Decision {
        for rule in &self.rules {
            let tool_matches = rule.tool == "*" || rule.tool == call.name;
            if !tool_matches {
                continue;
            }
            // arg_patterns: every (arg, regex) pair must match.
            // A rule with empty arg_patterns and no
            // command_starts_with matches the tool unconditionally.
            let args_match = rule.arg_patterns.iter().all(|(arg, re)| {
                call.args
                    .get(arg)
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| re.is_match(s))
            });
            if !args_match {
                continue;
            }
            // Backward-compatible substring matcher.
            if let Some(prefix) = &rule.command_starts_with {
                if call.name != "bash" {
                    continue;
                }
                let cmd = call
                    .args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !cmd.starts_with(prefix.as_str()) {
                    continue;
                }
            }
            return match rule.decision {
                RuleDecision::Allow => Decision::Allow,
                RuleDecision::Deny => Decision::Deny {
                    reason: format!("rule denied {} in policy.toml", call.name),
                },
                RuleDecision::Fallback => break,
            };
        }
        self.fallback.decide(call, history).await
    }
}

#[derive(Debug, Default, Deserialize)]
struct PolicyFile {
    #[serde(default)]
    rules: Vec<PolicyRule>,
}

/// Resolve the policy file path. Returns `None` if `$HOME` (or its
/// platform equivalent) cannot be determined.
pub fn policy_path() -> Option<PathBuf> {
    let base = dirs::config_dir()?;
    Some(base.join("crow").join("policy.toml"))
}

/// Errors from [`RuleBasedPolicy::from_file`].
#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("policy: I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("policy: TOML parse error in {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("policy: no home directory to load policy.toml from")]
    NoHome,
}

/// Awaitable for Ask decisions. The agent loop sends a request
/// through the resolver channel; the policy layer receives it,
/// prompts the user, and replies through the bundled oneshot.
pub type AskResolver = tokio::sync::mpsc::Sender<AskRequest>;

/// A pending Ask awaiting a human response. The agent loop produces
/// one when `policy.decide` returns `Decision::Ask { ask_id }`.
#[derive(Debug)]
pub struct AskRequest {
    /// Stable id (typically `<tool>-<call_id>`).
    pub ask_id: String,
    /// The tool call awaiting approval.
    pub call: ToolCall,
    /// Send the response back through this oneshot. Closing the
    /// sender without sending is treated as a denial.
    pub response: tokio::sync::oneshot::Sender<AskResponse>,
}

/// What the human (or the policy layer) replied with for a pending
/// Ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskResponse {
    Allow,
    Deny,
}

/// Format a `Decision::Deny` reason into a synthetic tool result
/// body that the model can react to.
#[must_use]
pub fn deny_reason(result: &Decision) -> Option<String> {
    match result {
        Decision::Deny { reason } => Some(reason.clone()),
        _ => None,
    }
}

/// Extract the tool name from a [`Part`] (used by policy debug logging).
#[must_use]
pub fn tool_name_from_part(part: &Part) -> Option<&str> {
    if let Part::ToolCall { name, .. } = part {
        Some(name.as_str())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{new_id, ToolCallId};
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            call_id: ToolCallId(new_id()),
            name: name.to_string(),
            args,
        }
    }

    #[tokio::test]
    async fn default_policy_allows_read() {
        let policy = DefaultPolicy;
        let d = policy
            .decide(&call("read", json!({"path": "x"})), &[])
            .await;
        assert_eq!(d, Decision::Allow);
    }

    #[tokio::test]
    async fn default_policy_asks_for_bash() {
        let policy = DefaultPolicy;
        let d = policy
            .decide(&call("bash", json!({"command": "ls"})), &[])
            .await;
        assert!(matches!(d, Decision::Ask { .. }));
    }

    #[tokio::test]
    async fn default_policy_denies_unknown_tool() {
        let policy = DefaultPolicy;
        let d = policy.decide(&call("exfiltrate", json!({})), &[]).await;
        match d {
            Decision::Deny { reason } => assert!(reason.contains("exfiltrate")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rule_allow_overrides_fallback() {
        let rules = vec![PolicyRule {
            tool: "bash".into(),
            command_starts_with: None,
            arg_patterns: vec![],
            decision: RuleDecision::Allow,
        }];
        let policy = RuleBasedPolicy::new(rules, Arc::new(DefaultPolicy));
        let d = policy
            .decide(&call("bash", json!({"command": "ls"})), &[])
            .await;
        assert_eq!(d, Decision::Allow);
    }

    #[tokio::test]
    async fn rule_deny_overrides_fallback() {
        let rules = vec![PolicyRule {
            tool: "*".into(),
            command_starts_with: None,
            arg_patterns: vec![],
            decision: RuleDecision::Deny,
        }];
        let policy = RuleBasedPolicy::new(rules, Arc::new(DefaultPolicy));
        let d = policy
            .decide(&call("read", json!({"path": "x"})), &[])
            .await;
        assert!(matches!(d, Decision::Deny { .. }));
    }

    #[tokio::test]
    async fn rule_command_prefix_only_matches_bash() {
        let rules = vec![PolicyRule {
            tool: "bash".into(),
            command_starts_with: Some("rm -rf".into()),
            arg_patterns: vec![],
            decision: RuleDecision::Deny,
        }];
        let policy = RuleBasedPolicy::new(rules, Arc::new(DefaultPolicy));
        // Bash starting with the prefix is denied.
        let d = policy
            .decide(&call("bash", json!({"command": "rm -rf /"})), &[])
            .await;
        assert!(matches!(d, Decision::Deny { .. }));
        // Bash with a different command falls through to fallback (Ask).
        let d = policy
            .decide(&call("bash", json!({"command": "ls"})), &[])
            .await;
        assert!(matches!(d, Decision::Ask { .. }));
        // Read ignores the prefix matcher and falls through.
        let d = policy
            .decide(&call("read", json!({"path": "x"})), &[])
            .await;
        assert_eq!(d, Decision::Allow);
    }

    #[tokio::test]
    async fn rule_fallback_passes_through() {
        let rules = vec![PolicyRule {
            tool: "bash".into(),
            command_starts_with: Some("ls".into()),
            arg_patterns: vec![],
            decision: RuleDecision::Fallback,
        }];
        let policy = RuleBasedPolicy::new(rules, Arc::new(DefaultPolicy));
        // `ls` matches the rule, which falls through to the
        // fallback (DefaultPolicy → Ask).
        let d = policy
            .decide(&call("bash", json!({"command": "ls"})), &[])
            .await;
        assert!(matches!(d, Decision::Ask { .. }));
    }

    #[tokio::test]
    async fn rule_file_loads_from_disk() {
        // Plant a rule file at the canonical path; load it; assert
        // the rule applies. Then clean up.
        let path = policy_path().expect("policy path");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = r#"[[rules]]
tool = "bash"
command_starts_with = "rm -rf"
decision = "deny"
"#;
        std::fs::write(&path, body).unwrap();

        let policy = RuleBasedPolicy::from_file(Arc::new(DefaultPolicy)).expect("load policy file");
        let d = policy
            .decide(&call("bash", json!({"command": "rm -rf /"})), &[])
            .await;
        assert!(matches!(d, Decision::Deny { .. }));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn missing_rule_file_is_not_an_error() {
        // Make sure no test above left a stray file behind; then
        // load — should succeed with an empty rule list.
        if let Some(path) = policy_path() {
            let _ = std::fs::remove_file(&path);
        }
        let policy = RuleBasedPolicy::from_file(Arc::new(DefaultPolicy))
            .expect("missing file is not an error");
        // Falls through to the default policy (Ask for bash).
        let d = policy
            .decide(&call("bash", json!({"command": "ls"})), &[])
            .await;
        assert!(matches!(d, Decision::Ask { .. }));
    }

    // --- F.20.03 arg_patterns tests ---

    #[tokio::test]
    async fn arg_pattern_allows_matching_command() {
        let p = RuleBasedPolicy::new(
            vec![PolicyRule {
                tool: "bash".into(),
                command_starts_with: None,
                arg_patterns: vec![ArgPattern {
                    arg: "command".into(),
                    regex: r"^git (status|diff)$".into(),
                }],
                decision: RuleDecision::Allow,
            }],
            Arc::new(DefaultPolicy),
        );
        let d = p
            .decide(&call("bash", json!({"command": "git status"})), &[])
            .await;
        assert!(matches!(d, Decision::Allow));
        let d = p
            .decide(&call("bash", json!({"command": "git diff"})), &[])
            .await;
        assert!(matches!(d, Decision::Allow));
        // git push doesn't match the regex; falls through to the
        // default policy's Ask decision.
        let d = p
            .decide(&call("bash", json!({"command": "git push"})), &[])
            .await;
        assert!(matches!(d, Decision::Ask { .. }));
    }

    #[tokio::test]
    async fn arg_pattern_matches_write_to_etc() {
        let p = RuleBasedPolicy::new(
            vec![PolicyRule {
                tool: "write".into(),
                command_starts_with: None,
                arg_patterns: vec![ArgPattern {
                    arg: "path".into(),
                    regex: r"^/etc/".into(),
                }],
                decision: RuleDecision::Deny,
            }],
            Arc::new(DefaultPolicy),
        );
        let d = p
            .decide(
                &call("write", json!({"path": "/etc/hosts", "content": "x"})),
                &[],
            )
            .await;
        assert!(matches!(d, Decision::Deny { .. }));
        let d = p
            .decide(
                &call("write", json!({"path": "/tmp/x", "content": "y"})),
                &[],
            )
            .await;
        // /tmp/x doesn't match the /etc pattern, so the rule doesn't
        // apply. Falls through to the fallback DefaultPolicy which
        // asks for write.
        assert!(matches!(d, Decision::Ask { .. }));
    }

    #[test]
    fn invalid_regex_is_dropped_at_load_time() {
        // A rule with an invalid regex should be dropped, not panic.
        let p = RuleBasedPolicy::new(
            vec![PolicyRule {
                tool: "bash".into(),
                command_starts_with: None,
                arg_patterns: vec![ArgPattern {
                    arg: "command".into(),
                    regex: r"(unclosed".into(), // invalid regex
                }],
                decision: RuleDecision::Allow,
            }],
            Arc::new(DefaultPolicy),
        );
        assert_eq!(p.rules.len(), 0);
    }

    #[test]
    fn multiple_arg_patterns_all_must_match() {
        let p = RuleBasedPolicy::new(
            vec![PolicyRule {
                tool: "bash".into(),
                command_starts_with: None,
                arg_patterns: vec![
                    ArgPattern {
                        arg: "command".into(),
                        regex: r"^git push".into(),
                    },
                    ArgPattern {
                        arg: "timeout_ms".into(),
                        regex: r"^[0-9]+$".into(),
                    },
                ],
                decision: RuleDecision::Allow,
            }],
            Arc::new(DefaultPolicy),
        );
        // Both match.
        assert_eq!(p.rules.len(), 1);
        let _ = &p; // silence unused
    }
}
