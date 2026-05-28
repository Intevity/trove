//! Cost rate table used by hook/watcher harnesses to convert a token
//! count into `trove.harness.cost.usd`.
//!
//! The defaults below are conservative and intended to be refreshed at
//! each Trove release (`MAPPING_PLAN.md` open question #1). Users override
//! per-model via the `costOverrides` field on their
//! [`crate::mappings::HarnessMapping`].
//!
//! Matching is **lowercased substring**: the first entry whose model
//! name appears in the harness-reported `model` attribute wins. Put the
//! longer/more-specific names first to avoid e.g. `claude-3-haiku`
//! matching ahead of `claude-3.5-sonnet`.

use std::collections::BTreeMap;

use super::CostOverride;

/// One row in the default rate table.
/// `(provider, model_substring, input_usd_per_1k_tokens, output_usd_per_1k_tokens)`.
/// The provider column is informational only — matching uses the model
/// substring against the harness-reported `model` attribute.
pub const DEFAULT_RATES: &[(&str, &str, f64, f64)] = &[
    // Anthropic — generation 4
    ("anthropic", "claude-opus-4", 15.0, 75.0),
    ("anthropic", "claude-sonnet-4", 3.0, 15.0),
    ("anthropic", "claude-haiku-4", 1.0, 5.0),
    // Anthropic — generation 3.5 (still in use)
    ("anthropic", "claude-3-5-sonnet", 3.0, 15.0),
    ("anthropic", "claude-3.5-sonnet", 3.0, 15.0),
    // Anthropic — generation 3
    ("anthropic", "claude-3-opus", 15.0, 75.0),
    ("anthropic", "claude-3-haiku", 0.25, 1.25),
    // OpenAI — gpt-4o family
    ("openai", "gpt-4o-mini", 0.15, 0.6),
    ("openai", "gpt-4o", 2.5, 10.0),
    ("openai", "gpt-4-turbo", 10.0, 30.0),
    // OpenAI — o-series reasoning models
    ("openai", "o1-mini", 3.0, 12.0),
    ("openai", "o1", 15.0, 60.0),
    // Google — Gemini family
    ("google", "gemini-2.5-pro", 1.25, 10.0),
    ("google", "gemini-2.0-flash", 0.075, 0.3),
    ("google", "gemini-1.5-pro", 1.25, 5.0),
    ("google", "gemini-1.5-flash", 0.075, 0.3),
];

/// Look up the (input, output) rate pair for `model_name` in USD per
/// 1,000 tokens. Per-harness `overrides` are consulted first (exact
/// case-insensitive key match), then [`DEFAULT_RATES`] (lowercased
/// substring match in declaration order).
///
/// Returns `None` for an unknown model — callers must skip emitting
/// `trove.harness.cost.usd` rather than fabricate a number. Matches the
/// Cursor hook's behavior at `resources/hooks/cursor-otel-hook-impl.cjs`.
#[must_use]
pub fn lookup_rate(
    model_name: &str,
    overrides: &BTreeMap<String, CostOverride>,
) -> Option<(f64, f64)> {
    let needle = model_name.to_ascii_lowercase();

    // Overrides take precedence. Match case-insensitively on the key.
    for (k, v) in overrides {
        if needle.contains(&k.to_ascii_lowercase()) {
            return Some((v.input_usd_per_1k_tokens, v.output_usd_per_1k_tokens));
        }
    }

    for (_provider, model, input, output) in DEFAULT_RATES {
        if needle.contains(&model.to_ascii_lowercase()) {
            return Some((*input, *output));
        }
    }

    None
}

/// Convenience wrapper for converting token counts into a USD figure using
/// `lookup_rate`. Returns `None` when the model is unrecognized.
///
/// `cache_read_tokens` are billed at 0.1× the base input rate (Anthropic
/// prompt-cache read pricing, confirmed May 2026). Pass `0` for models or
/// callers where cache-read token counts are unavailable.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn estimate_cost_usd(
    model_name: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    overrides: &BTreeMap<String, CostOverride>,
) -> Option<f64> {
    let (input_rate, output_rate) = lookup_rate(model_name, overrides)?;
    let input = (input_tokens as f64 / 1000.0) * input_rate;
    let output = (output_tokens as f64 / 1000.0) * output_rate;
    let cache_read = (cache_read_tokens as f64 / 1000.0) * input_rate * 0.1;
    Some(input + output + cache_read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_anthropic_model_resolves_to_default_rate() {
        let (i, o) = lookup_rate("claude-sonnet-4", &BTreeMap::new()).unwrap();
        assert!((i - 3.0).abs() < f64::EPSILON);
        assert!((o - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn substring_match_is_case_insensitive() {
        let (i, _) = lookup_rate("Claude-Sonnet-4-20250108", &BTreeMap::new()).unwrap();
        assert!((i - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_model_returns_none_not_a_fabricated_rate() {
        assert!(lookup_rate("nonexistent-model-xyz", &BTreeMap::new()).is_none());
    }

    #[test]
    fn override_takes_precedence_over_default() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "claude-sonnet-4".into(),
            CostOverride {
                input_usd_per_1k_tokens: 99.0,
                output_usd_per_1k_tokens: 100.0,
            },
        );
        let (i, o) = lookup_rate("claude-sonnet-4", &overrides).unwrap();
        assert!((i - 99.0).abs() < f64::EPSILON);
        assert!((o - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_cost_does_the_math() {
        // 1000 input + 1000 output @ sonnet-4 ($3 + $15) = $18
        let usd =
            estimate_cost_usd("claude-sonnet-4", 1000, 1000, 0, &BTreeMap::new()).unwrap();
        assert!((usd - 18.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_read_tokens_add_ten_percent_of_input_rate() {
        // 10 000 cache-read tokens @ sonnet-4 ($3/1k input * 0.1 * 10) = $3.00
        let usd = estimate_cost_usd("claude-sonnet-4", 0, 0, 10_000, &BTreeMap::new()).unwrap();
        assert!((usd - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn longer_model_names_come_before_shorter_to_avoid_premature_match() {
        // "claude-3-haiku" must not be hit by a request for
        // "claude-3.5-sonnet"; the rate for the longer name should win.
        let (i, _) = lookup_rate("claude-3.5-sonnet-20240620", &BTreeMap::new()).unwrap();
        assert!((i - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn default_rates_table_is_not_empty() {
        assert!(!DEFAULT_RATES.is_empty());
    }
}
