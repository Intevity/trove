//! Per-harness Tier A mapping configuration.
//!
//! The five-metric Tier A schema is fixed (see [`TierAMetric`]). What the
//! user *can* configure is how each harness's raw signals contribute to
//! that schema:
//!
//! - **Hook/watcher harnesses** declare [`MappingSource::HookRule`] rows
//!   keyed by raw event name (e.g. `"afterAgentResponse"`). The hook
//!   driver consults the matching row at emission time and constructs
//!   the right Tier A metric + attributes inline. The collector pipeline
//!   doesn't see these signals as Tier B at all.
//!
//! - **Native-OTel harnesses** declare [`MappingSource::SynthesizeFromNative`]
//!   rows that copy/rename a native metric (e.g. `claude_code.token.usage`)
//!   onto a Tier A metric. The supervisor regenerates `collector.yaml` to
//!   include a `transform/tierA-<harness>` processor that performs the
//!   rename at the collector. Tier B passes through unchanged on top.
//!
//! [`MappingState`] is persisted in `state.json` alongside the rest of
//! [`crate::app_state::AppState`]. Schema v6 introduced the field; v5
//! documents auto-populate per-harness defaults via [`default_state`].
//!
//! Mirrors the Zod schemas in `packages/shared/src/schemas.ts`:
//! `TierAMetric`, `MappingSource`, `HarnessMapping`, `MappingState`.
//! Wire format is camelCase (matches the existing `AppState` convention).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::harness::HarnessId;

pub mod cost;
pub mod defaults;
pub mod validate;

pub use defaults::{default_state, defaults_for};
pub use validate::{validate, ValidationError};

/// Persisted schema version. Bumped any time [`MappingState`]'s on-disk
/// shape changes incompatibly. Today: always `1`.
pub const MAPPING_SCHEMA_VERSION: u32 = 1;

/// The fixed five-metric Tier A schema. Variants serialize as the
/// literal metric-name suffixes (after `trove.harness.`) so the wire
/// format matches the Zod enum byte-for-byte.
///
/// The dotted forms (`cost.usd`, `turn.duration`) are intentional —
/// they correspond to the literal metric names a user would see in
/// `SignOz` when filtering. Rust's enum variant names can't carry dots,
/// so each is `#[serde(rename = ...)]`'d to the literal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum TierAMetric {
    #[serde(rename = "events")]
    Events,
    #[serde(rename = "tokens")]
    Tokens,
    #[serde(rename = "cost.usd")]
    CostUsd,
    #[serde(rename = "turn.duration")]
    TurnDuration,
    #[serde(rename = "errors")]
    Errors,
}

impl TierAMetric {
    /// Full OTLP metric name as it appears on the wire (`trove.harness.X`).
    /// Use this when constructing OTLP payloads or collector transform
    /// statements — the bare variant string is fine for UI labels and
    /// serialized config, but the OTLP wire wants the full namespace.
    #[must_use]
    pub fn full_name(self) -> &'static str {
        match self {
            Self::Events => "trove.harness.events",
            Self::Tokens => "trove.harness.tokens",
            Self::CostUsd => "trove.harness.cost.usd",
            Self::TurnDuration => "trove.harness.turn.duration",
            Self::Errors => "trove.harness.errors",
        }
    }

    /// The set of attribute keys this Tier A metric expects on every
    /// data point. Used by [`validate`] to catch typo'd attribute keys
    /// before the collector ever sees them.
    #[must_use]
    pub fn required_attributes(self) -> &'static [&'static str] {
        match self {
            Self::Events | Self::TurnDuration => &["event.kind"],
            Self::Tokens => &["direction"],
            Self::CostUsd => &["cost.method"],
            Self::Errors => &["error.kind"],
        }
    }
}

/// Where a Tier A signal comes from. Tagged on the wire by `kind`.
///
/// Hook/watcher harnesses emit Tier A inline from their driver code —
/// the row tells the driver which raw event maps onto which Tier A
/// metric. Native-OTel harnesses synthesize Tier A from a native metric
/// already on the wire — the row tells the collector's `transform`
/// processor to rename `nativeMetric` → `targetMetric`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MappingSource {
    #[serde(rename_all = "camelCase")]
    HookRule {
        /// Raw event name as the hook driver sees it (e.g.
        /// `"afterAgentResponse"`, `"api_req_finished"`). Free-form
        /// because each harness's event namespace is its own.
        when: String,
        /// What the driver should emit when this rule fires. `None`
        /// means "explicitly do not emit anything for this event"
        /// (the default for `beforeSubmitPrompt` etc., to avoid
        /// double-counting). Mirrors the Zod `.nullable()` shape.
        emit: Option<HookEmit>,
    },
    #[serde(rename_all = "camelCase")]
    SynthesizeFromNative {
        /// Native metric name as the harness emits it
        /// (e.g. `"claude_code.token.usage"`,
        /// `"gen_ai.client.token.usage"`).
        native_metric: String,
        /// Tier A bucket the native metric copies into.
        target_metric: TierAMetric,
        /// Per-attribute *rename* map: raw key → Tier A key (e.g.
        /// `"gen_ai.token.type" → "direction"`). Empty map means
        /// "carry every attribute over unchanged". Resulting attribute
        /// values must still fall within the Tier A enum domain;
        /// validation catches mismatches.
        ///
        /// Wired through `metricstransform`'s `update_label` operation.
        /// Pre-existing attributes on the source metric are preserved
        /// on the synthesized copy; this only renames the listed keys.
        attribute_map: BTreeMap<String, String>,
        /// Per-attribute *injection* map: key → constant value to add.
        /// Used to set required Tier A attributes like `event.kind` and
        /// `error.kind` that the native metric doesn't carry. Without
        /// these, dashboards filtering on those attributes see empty
        /// results even when the synthesized metric is flowing.
        ///
        /// Wired through `metricstransform`'s `add_label` operation.
        /// Empty map = no injections.
        #[serde(default)]
        inject_attributes: BTreeMap<String, String>,
    },
}

/// What a [`MappingSource::HookRule`] tells the driver to emit when its
/// trigger fires.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEmit {
    /// Which Tier A metric this row contributes to.
    pub metric: TierAMetric,
    /// Attribute key/value pairs the driver should attach to every
    /// data point. Values must match the Tier A enum domain for the
    /// keys the metric requires (e.g. `event.kind = "chat.turn"`).
    pub attributes: BTreeMap<String, String>,
}

/// One harness's worth of mapping rows plus its enable flag and any
/// per-model cost-rate overrides. Mirrors the Zod `HarnessMapping`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessMapping {
    pub harness_id: HarnessId,
    /// Master switch. When `false`, the driver/collector falls back to
    /// Tier B passthrough only and no Tier A is produced for this
    /// harness. Disabling does not delete the `sources` array —
    /// re-enabling restores it as-is.
    pub enabled: bool,
    pub sources: Vec<MappingSource>,
    /// Per-harness rate-table overrides for `cost.usd` synthesis.
    /// Empty map = use Trove defaults from
    /// [`crate::mappings::cost::DEFAULT_RATES`]. Matched against the
    /// `model` attribute by lower-cased substring (see
    /// [`crate::mappings::cost::lookup_rate`]).
    #[serde(default)]
    pub cost_overrides: BTreeMap<String, CostOverride>,
}

/// One row in the per-harness cost-override table. Both rates are
/// dollars per 1,000 tokens.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostOverride {
    pub input_usd_per_1k_tokens: f64,
    pub output_usd_per_1k_tokens: f64,
}

// f64 doesn't implement Eq because of NaN. For HarnessMapping's derive
// chain we use PartialEq above; tests compare with assert_eq! against
// well-known values so NaN never enters the picture.
impl Eq for CostOverride {}

/// The whole mapping config, persisted in `state.json` under the
/// `mappings` field. Mirrors the Zod `MappingState`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingState {
    /// Pinned to [`MAPPING_SCHEMA_VERSION`]. Independent of
    /// [`crate::app_state::CURRENT_SCHEMA_VERSION`] — the outer state
    /// migration handles older `AppState` shapes; this inner version
    /// only moves when [`MappingState`] itself changes incompatibly.
    pub schema_version: u32,
    pub harnesses: Vec<HarnessMapping>,
}

impl Default for MappingState {
    /// Fresh state ships per-harness defaults so first-launch users
    /// immediately see Tier A signals on the dashboard. See
    /// [`default_state`] for the actual defaults table.
    fn default() -> Self {
        default_state()
    }
}

impl MappingState {
    /// Look up the mapping for `id`. Returns `None` if the state has no
    /// entry for that harness (e.g. user pruned it; the IPC layer
    /// auto-restores on load via [`default_state`], so this is rare).
    #[must_use]
    pub fn for_harness(&self, id: HarnessId) -> Option<&HarnessMapping> {
        self.harnesses.iter().find(|h| h.harness_id == id)
    }

    /// Mutable lookup. Used by the `apply_mappings` IPC command to
    /// stage edits before validation.
    pub fn for_harness_mut(&mut self, id: HarnessId) -> Option<&mut HarnessMapping> {
        self.harnesses.iter_mut().find(|h| h.harness_id == id)
    }

    /// Every harness with at least one enabled
    /// [`MappingSource::SynthesizeFromNative`] row — the set the
    /// collector codegen needs to emit `transform/tierA-*` processors
    /// for.
    #[must_use]
    pub fn native_synthesis_harnesses(&self) -> Vec<&HarnessMapping> {
        self.harnesses
            .iter()
            .filter(|h| {
                h.enabled
                    && h.sources
                        .iter()
                        .any(|s| matches!(s, MappingSource::SynthesizeFromNative { .. }))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_a_metric_serializes_with_dotted_renames() {
        assert_eq!(serde_json::to_string(&TierAMetric::Events).unwrap(), "\"events\"");
        assert_eq!(
            serde_json::to_string(&TierAMetric::CostUsd).unwrap(),
            "\"cost.usd\""
        );
        assert_eq!(
            serde_json::to_string(&TierAMetric::TurnDuration).unwrap(),
            "\"turn.duration\""
        );
        assert_eq!(serde_json::to_string(&TierAMetric::Tokens).unwrap(), "\"tokens\"");
        assert_eq!(serde_json::to_string(&TierAMetric::Errors).unwrap(), "\"errors\"");
    }

    #[test]
    fn tier_a_metric_full_name_carries_trove_prefix() {
        assert_eq!(TierAMetric::Events.full_name(), "trove.harness.events");
        assert_eq!(TierAMetric::CostUsd.full_name(), "trove.harness.cost.usd");
        assert_eq!(
            TierAMetric::TurnDuration.full_name(),
            "trove.harness.turn.duration"
        );
    }

    #[test]
    fn mapping_source_hook_rule_round_trips_with_nullable_emit() {
        let r = MappingSource::HookRule {
            when: "beforeSubmitPrompt".into(),
            emit: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"kind\":\"hook-rule\""));
        assert!(json.contains("\"emit\":null"));
        let back: MappingSource = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn mapping_source_synthesize_round_trips() {
        let s = MappingSource::SynthesizeFromNative {
            native_metric: "claude_code.token.usage".into(),
            target_metric: TierAMetric::Tokens,
            attribute_map: BTreeMap::from([("type".into(), "direction".into())]),
            inject_attributes: BTreeMap::new(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"synthesize-from-native\""));
        assert!(json.contains("\"nativeMetric\":\"claude_code.token.usage\""));
        assert!(json.contains("\"targetMetric\":\"tokens\""));
        let back: MappingSource = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn mapping_source_synthesize_with_inject_attributes_round_trips() {
        let s = MappingSource::SynthesizeFromNative {
            native_metric: "gemini_cli.api.request.count".into(),
            target_metric: TierAMetric::Events,
            attribute_map: BTreeMap::new(),
            inject_attributes: BTreeMap::from([
                ("event.kind".into(), "chat.turn".into()),
            ]),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"injectAttributes\""));
        assert!(json.contains("\"event.kind\":\"chat.turn\""));
        let back: MappingSource = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn mapping_source_synthesize_defaults_inject_attributes_to_empty() {
        // Round-trip from a wire payload that omits injectAttributes — older
        // clients / saved state files won't carry the field; default is "".
        let wire = r#"{"kind":"synthesize-from-native","nativeMetric":"x","targetMetric":"events","attributeMap":{}}"#;
        let parsed: MappingSource = serde_json::from_str(wire).unwrap();
        match parsed {
            MappingSource::SynthesizeFromNative { inject_attributes, .. } => {
                assert!(inject_attributes.is_empty());
            }
            MappingSource::HookRule { .. } => panic!("expected synthesize"),
        }
    }

    #[test]
    fn harness_mapping_serializes_camel_case_fields() {
        let m = HarnessMapping {
            harness_id: HarnessId::GeminiCli,
            enabled: true,
            sources: vec![],
            cost_overrides: BTreeMap::new(),
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"harnessId\":\"gemini-cli\""));
        assert!(json.contains("\"costOverrides\":{}"));
    }

    #[test]
    fn cost_override_serializes_camel_case() {
        let o = CostOverride {
            input_usd_per_1k_tokens: 3.0,
            output_usd_per_1k_tokens: 15.0,
        };
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains("\"inputUsdPer1kTokens\":3.0"));
        assert!(json.contains("\"outputUsdPer1kTokens\":15.0"));
    }

    #[test]
    fn mapping_state_round_trips_through_serde() {
        let s = default_state();
        let json = serde_json::to_string(&s).unwrap();
        let back: MappingState = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert_eq!(s.schema_version, MAPPING_SCHEMA_VERSION);
    }

    #[test]
    fn for_harness_lookup_returns_existing_entry() {
        let s = default_state();
        let entry = s.for_harness(HarnessId::ClaudeCode).unwrap();
        assert_eq!(entry.harness_id, HarnessId::ClaudeCode);
    }

    #[test]
    fn native_synthesis_harnesses_only_lists_those_with_synthesis_rows() {
        let s = default_state();
        let with_synth: Vec<HarnessId> = s
            .native_synthesis_harnesses()
            .iter()
            .map(|h| h.harness_id)
            .collect();
        // Tier 1 (native-OTel) harnesses should be in this set;
        // hook/watcher harnesses should not.
        assert!(with_synth.contains(&HarnessId::ClaudeCode));
        assert!(with_synth.contains(&HarnessId::GeminiCli));
        assert!(!with_synth.contains(&HarnessId::CursorIde));
        assert!(!with_synth.contains(&HarnessId::Aider));
    }

    #[test]
    fn required_attributes_match_tier_a_schema() {
        assert_eq!(TierAMetric::Events.required_attributes(), &["event.kind"]);
        assert_eq!(TierAMetric::Tokens.required_attributes(), &["direction"]);
        assert_eq!(TierAMetric::CostUsd.required_attributes(), &["cost.method"]);
        assert_eq!(TierAMetric::Errors.required_attributes(), &["error.kind"]);
    }
}
