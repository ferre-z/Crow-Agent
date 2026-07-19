//! Per-model token pricing.
//!
//! Loaded from `<workspace_root>/config/pricing.toml` at startup.
//! Each entry sets `input_per_1k`, `output_per_1k` (USD per 1K
//! tokens), and `context_size` (the model's known context window
//! in tokens). The `default` entry is the fallback for unknown
//! model ids so the status bar and `/cost` always render a number.
//!
//! The TUI reads this file once per launch. Editing the file and
//! restarting `crow tui` is enough to pick up new rates — no
//! recompile required.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

/// Per-model pricing row.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModelPricing {
    /// Known context window in tokens. Used for the F.04.06
    /// "context window used %" indicator.
    #[serde(default = "default_context_size")]
    pub context_size: u32,
    /// USD per 1K input tokens.
    #[serde(default = "default_rate")]
    pub input_per_1k: f64,
    /// USD per 1K output tokens.
    #[serde(default = "default_rate")]
    pub output_per_1k: f64,
}

fn default_context_size() -> u32 {
    32_768
}

fn default_rate() -> f64 {
    0.0
}

/// File schema.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PricingFile {
    #[serde(flatten)]
    pub models: HashMap<String, ModelPricing>,
}

/// In-memory pricing table keyed by model id.
#[derive(Debug, Clone, Default)]
pub struct Pricing {
    models: HashMap<String, ModelPricing>,
}

impl Pricing {
    /// Empty pricing (everything zero, default context).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Load from a TOML file. Missing file is OK — empty pricing
    /// falls back to the `default` entry which starts at zero
    /// rates (so the cost indicator just shows `$0.0000` instead of
    /// crashing).
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => match toml::from_str::<PricingFile>(&s) {
                Ok(file) => Self {
                    models: file.models,
                },
                Err(e) => {
                    tracing::warn!("pricing.toml parse error: {e}");
                    Self::empty()
                }
            },
            Err(_) => Self::empty(),
        }
    }

    /// Look up pricing for `model`. Falls back to `default`,
    /// then to a zero-rate synthetic row so the TUI always has a
    /// value to display.
    ///
    /// Returns a reference to a synthesized default if no entry is
    /// found anywhere — that keeps the call site `.cost(...)` and
    /// the status bar cheap to render even when pricing.toml is
    /// missing or doesn't list the active model.
    #[must_use]
    pub fn for_model(&self, model: &str) -> &ModelPricing {
        if let Some(p) = self.models.get(model) {
            return p;
        }
        if let Some(p) = self.models.get("default") {
            return p;
        }
        // Synthesize a default. The static is fine here because
        // `for_model` only reads from it (no mutation).
        static DEFAULT: std::sync::OnceLock<ModelPricing> = std::sync::OnceLock::new();
        DEFAULT.get_or_init(|| ModelPricing {
            context_size: default_context_size(),
            input_per_1k: default_rate(),
            output_per_1k: default_rate(),
        })
    }

    /// Compute the USD cost for `input_tokens` input and
    /// `output_tokens` output under the active model's rate table.
    #[must_use]
    pub fn cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        let p = self.for_model(model);
        (input_tokens as f64 / 1000.0) * p.input_per_1k
            + (output_tokens as f64 / 1000.0) * p.output_per_1k
    }

    /// Add `p` to this table under `model`. Used by tests that
    /// build a pricing table inline.
    pub fn insert(&mut self, model: impl Into<String>, p: ModelPricing) {
        self.models.insert(model.into(), p);
    }
}

/// Render `usd` as a short string. `< $1` uses 4 fraction digits;
/// `>= $1` uses 2.
#[must_use]
pub fn format_usd(usd: f64) -> String {
    if usd < 1.0 {
        format!("${usd:.4}")
    } else {
        format!("${usd:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(input: f64, output: f64) -> ModelPricing {
        ModelPricing {
            context_size: 100_000,
            input_per_1k: input,
            output_per_1k: output,
        }
    }

    #[test]
    fn empty_pricing_loads_as_zero_rate_default() {
        let p = Pricing::empty();
        let m = p.for_model("anything");
        assert_eq!(m.input_per_1k, 0.0);
        assert_eq!(m.output_per_1k, 0.0);
    }

    #[test]
    fn load_missing_file_is_empty_pricing() {
        let p = Pricing::load(Path::new("/nonexistent.toml"));
        assert!(p.models.is_empty());
    }

    #[test]
    fn lookup_falls_back_to_default() {
        let mut p = Pricing::empty();
        p.insert("default", fp(0.001, 0.003));
        let m = p.for_model("unknown-model");
        assert_eq!(m.input_per_1k, 0.001);
    }

    #[test]
    fn lookup_uses_exact_match_when_present() {
        let mut p = Pricing::empty();
        p.insert("default", fp(0.001, 0.003));
        p.insert("special", fp(0.5, 0.5));
        let m = p.for_model("special");
        assert_eq!(m.input_per_1k, 0.5);
        assert_eq!(m.output_per_1k, 0.5);
    }

    #[test]
    fn cost_combines_input_and_output() {
        let mut p = Pricing::empty();
        p.insert("default", fp(0.001, 0.003));
        // 1000 in + 2000 out at 0.001/0.003 per 1k = 0.001 + 0.006 = 0.007
        assert!((p.cost("any", 1000, 2000) - 0.007).abs() < 1e-9);
    }

    #[test]
    fn format_usd_switches_precision() {
        assert_eq!(format_usd(0.0), "$0.0000");
        assert_eq!(format_usd(0.5), "$0.5000");
        assert_eq!(format_usd(1.0), "$1.00");
        assert_eq!(format_usd(12.345), "$12.35");
    }

    #[test]
    fn parses_real_pricing_file() {
        let toml = r#"
[default]
context_size = 32768
input_per_1k = 0.001
output_per_1k = 0.003

["nvidia/nemotron-3-ultra-550b-a55b"]
context_size = 262144
input_per_1k = 0.0005
output_per_1k = 0.0015
"#;
        let file: PricingFile = toml::from_str(toml).unwrap();
        assert_eq!(file.models.len(), 2);
        assert_eq!(
            file.models["nvidia/nemotron-3-ultra-550b-a55b"].context_size,
            262144
        );
    }
}
