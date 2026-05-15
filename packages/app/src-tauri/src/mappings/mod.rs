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
pub mod runtime;
pub mod store;
pub mod validate;

pub use defaults::{default_metric_catalog, default_state, defaults_for};
pub use store::{MappingStateStore, SharedMappingState};
pub use validate::{validate, ValidationError};

/// Persisted schema version. Bumped any time [`MappingState`]'s on-disk
/// shape changes incompatibly.
///
/// - v1: initial five-metric Tier A schema, hardcoded.
/// - v2: introduces user-customizable [`TroveMetricDefinition`] catalog
///   alongside `harnesses`. The five Tier A metrics are seeded as
///   builtin entries (`builtin: true`, read-only in the UI); users may
///   add custom counters/gauges/histograms with their own required
///   attribute lists.
pub const MAPPING_SCHEMA_VERSION: u32 = 2;

/// Builtin metric identifier on the wire — the short suffix (after
/// `trove.harness.`) for the five ships-with metrics, or a user-chosen
/// slug for custom metrics. Resolution happens via the
/// [`MappingState::metrics`] catalog.
///
/// String, not enum: the catalog is open by design (v2). Callers that
/// need the metric's wire name, kind, or required attributes consult
/// [`MappingState::metric`] rather than pattern-matching on the id.
pub type MetricId = String;

/// What kind of OTLP signal a [`TroveMetricDefinition`] represents.
/// Drives the codegen + watcher emission path (`Sum` for counter,
/// `Gauge` for gauge, `Histogram` for histogram).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TroveMetricKind {
    Counter,
    Gauge,
    Histogram,
}

/// One entry in the metric catalog. The five builtin Tier A metrics
/// ship with `builtin: true` and are locked in the UI (rename/delete
/// disabled). Users may add custom entries with their own name, kind,
/// and required attribute list.
///
/// Mirrors the Zod `TroveMetricDefinition` in
/// `packages/shared/src/schemas.ts`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TroveMetricDefinition {
    /// Stable reference used by [`MappingSource`] target fields and
    /// [`HookEmit::metric`]. For builtins this is the legacy short
    /// string (`"events"`, `"tokens"`, `"cost.usd"`, `"turn.duration"`,
    /// `"errors"`) so v1 docs migrate forward without rewriting any
    /// target references. For custom metrics the user picks a slug.
    pub id: MetricId,
    /// Full OTLP metric name as it appears on the wire. For builtins
    /// this is `trove.harness.<id>`. For custom metrics, whatever the
    /// user typed.
    pub name: String,
    pub kind: TroveMetricKind,
    #[serde(default)]
    pub description: String,
    /// Attribute keys every data point of this metric is expected to
    /// carry. Used by the validator to flag rows missing required keys
    /// before the collector ever sees them.
    #[serde(default)]
    pub required_attributes: Vec<String>,
    /// `true` only for the five ships-with Tier A metrics. The UI locks
    /// rename/delete on these; only the description is editable. The
    /// rest of the runtime treats them like any other catalog entry.
    #[serde(default)]
    pub builtin: bool,
}

/// Helper enum naming the five builtin metrics — used **only** when
/// constructing default mappings and seeding the catalog. The persisted
/// types ([`MappingSource`], [`HookEmit`], [`TroveMetricDefinition`])
/// all reference metrics by [`MetricId`] string, not this enum.
///
/// Variants serialize via per-variant `#[serde(rename)]` so that
/// [`Self::id`] returns the exact wire string used by v1 documents
/// (`"events"`, `"tokens"`, `"cost.usd"`, `"turn.duration"`,
/// `"errors"`). v1 → v2 migration relies on this byte-identical
/// continuity to avoid rewriting any `targetMetric`/`metric` references.
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
    /// Stable id used in [`MappingSource`] target fields and the catalog
    /// entry's `id`. Matches the wire string the v1 schema used.
    #[must_use]
    pub fn id(self) -> MetricId {
        match self {
            Self::Events => "events".to_string(),
            Self::Tokens => "tokens".to_string(),
            Self::CostUsd => "cost.usd".to_string(),
            Self::TurnDuration => "turn.duration".to_string(),
            Self::Errors => "errors".to_string(),
        }
    }

    /// Full OTLP metric name as it appears on the wire (`trove.harness.X`).
    /// Used when seeding the builtin catalog entry's `name` field.
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

    /// OTLP kind for this Tier A metric. Drives the catalog entry's
    /// `kind` and (via lookup) the watcher emission code path.
    #[must_use]
    pub fn kind(self) -> TroveMetricKind {
        match self {
            // Counters with delta+monotonic semantics, per Tier A schema.
            Self::Events | Self::Tokens | Self::CostUsd | Self::Errors => TroveMetricKind::Counter,
            // `turn.duration` is the only histogram in the Tier A schema.
            Self::TurnDuration => TroveMetricKind::Histogram,
        }
    }

    /// The set of attribute keys this Tier A metric expects on every
    /// data point. Seeds the builtin catalog entry's
    /// `required_attributes`.
    #[must_use]
    pub fn required_attributes(self) -> &'static [&'static str] {
        match self {
            Self::Events | Self::TurnDuration => &["event.kind"],
            Self::Tokens => &["direction"],
            Self::CostUsd => &["cost.method"],
            Self::Errors => &["error.kind"],
        }
    }

    /// Materialize the builtin [`TroveMetricDefinition`] for this metric.
    /// Used by `defaults::default_metric_catalog` to seed the v2
    /// `MappingState::metrics` array.
    #[must_use]
    pub fn definition(self) -> TroveMetricDefinition {
        TroveMetricDefinition {
            id: self.id(),
            name: self.full_name().to_string(),
            kind: self.kind(),
            description: String::new(),
            required_attributes: self
                .required_attributes()
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            builtin: true,
        }
    }

    /// Every builtin Tier A metric, in dashboard-display order. Used by
    /// `default_metric_catalog` and tests.
    #[must_use]
    pub fn all() -> [Self; 5] {
        [
            Self::Events,
            Self::Tokens,
            Self::CostUsd,
            Self::TurnDuration,
            Self::Errors,
        ]
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
        /// Target metric id — references a [`TroveMetricDefinition::id`]
        /// in [`MappingState::metrics`]. For the five builtin Tier A
        /// metrics these are the short strings `"events"`, `"tokens"`,
        /// `"cost.usd"`, `"turn.duration"`, `"errors"`; for custom
        /// metrics, whatever slug the user picked. Validation rejects
        /// rows whose `target_metric` doesn't resolve to a catalog entry.
        target_metric: MetricId,
        /// Per-attribute *rename* map: raw key → target key (e.g.
        /// `"gen_ai.token.type" → "direction"`). Empty map means
        /// "carry every attribute over unchanged". Resulting attribute
        /// values must still fall within the target metric's enum
        /// domain (for builtin keys); validation catches mismatches.
        ///
        /// Wired through `metricstransform`'s `update_label` operation.
        /// Pre-existing attributes on the source metric are preserved
        /// on the synthesized copy; this only renames the listed keys.
        attribute_map: BTreeMap<String, String>,
        /// Per-attribute *injection* map: key → constant value to add.
        /// Used to set required attributes the native metric doesn't
        /// carry (e.g. `event.kind`, `error.kind`). Without these,
        /// dashboards filtering on those attributes see empty results
        /// even when the synthesized metric is flowing.
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
    /// Catalog id of the metric this row contributes to. See
    /// [`MappingSource::SynthesizeFromNative::target_metric`] for the
    /// id convention. Validation rejects emit rows whose metric
    /// doesn't resolve to a catalog entry.
    pub metric: MetricId,
    /// Attribute key/value pairs the driver should attach to every
    /// data point. Values must match the closed-enum domain for the
    /// well-known keys the metric requires (e.g.
    /// `event.kind = "chat.turn"`).
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
    /// User-customizable metric catalog. v1 documents (no `metrics`
    /// field on disk) load via `serde(default)` with an empty vec; the
    /// `migrate_to_current` helper then seeds the 5 builtin entries.
    /// Custom metrics (`builtin: false`) extend the catalog and may be
    /// targeted from any [`MappingSource`] row.
    #[serde(default)]
    pub metrics: Vec<TroveMetricDefinition>,
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

    /// Look up the catalog definition for a [`MetricId`] (the string
    /// stored on `MappingSource`/`HookEmit`). Returns `None` when the
    /// id doesn't resolve — validation flags these before runtime so
    /// consumers can usually `expect` after a successful `validate()`.
    #[must_use]
    pub fn metric(&self, id: &str) -> Option<&TroveMetricDefinition> {
        self.metrics.iter().find(|m| m.id == id)
    }

    /// In-place migration to the current schema version. Idempotent.
    /// Called from the outer `app_state::load_from_dir` after parse;
    /// also useful in tests that synthesize a v1 [`MappingState`] by
    /// hand.
    ///
    /// Returns `true` when the migration mutated the state (so the
    /// caller can persist the fixed-up document back to disk).
    ///
    /// v1 → v2:
    /// - ensure every builtin catalog entry is present;
    /// - coerce legacy closed-enum values (`error.kind = routing | retry`
    ///   → `network`) that pre-dated the connector's validation rules;
    /// - rewrite the Claude Desktop hook-rule fan-out from the old
    ///   single-key (`api_request → tokens|cost.usd`) layout to the new
    ///   per-facet keys (`api_request.tokens.{input,output}` /
    ///   `api_request.cost`). The watcher emits per-facet `when` keys
    ///   so the old layout silently dropped data points; we migrate it
    ///   to match the new defaults.
    ///
    /// Custom metric definitions (`builtin: false`) and user-authored
    /// rules that don't match the legacy patterns are left untouched.
    pub fn migrate_to_current(&mut self) -> bool {
        let mut changed = false;
        for builtin in TierAMetric::all() {
            let id = builtin.id();
            if !self.metrics.iter().any(|m| m.id == id) {
                self.metrics.push(builtin.definition());
                changed = true;
            }
        }
        for harness in &mut self.harnesses {
            for source in &mut harness.sources {
                if coerce_legacy_error_kind(source) {
                    changed = true;
                }
            }
        }
        for harness in &mut self.harnesses {
            if harness.harness_id == HarnessId::ClaudeDesktop
                && split_legacy_api_request_fanout(&mut harness.sources)
            {
                changed = true;
            }
            if harness.harness_id == HarnessId::Cline
                && drop_legacy_cline_api_req_finished(&mut harness.sources)
            {
                changed = true;
            }
        }
        if self.schema_version != MAPPING_SCHEMA_VERSION {
            self.schema_version = MAPPING_SCHEMA_VERSION;
            changed = true;
        }
        changed
    }

    /// Every configured harness mapping, in declaration order. The
    /// codegen uses this when it needs to decide which harnesses get a
    /// `transform/harness-tag` rule — that set is broader than
    /// [`Self::native_synthesis_harnesses`] because hook-only Tier-1
    /// emitters (e.g., Claude Desktop) also need their `service.name`
    /// mapped to `harness.id`.
    #[must_use]
    pub fn all_harnesses(&self) -> Vec<&HarnessMapping> {
        self.harnesses.iter().collect()
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

/// Coerce legacy `error.kind` inject values to the current closed-enum
/// domain. Pre-v2 defaults shipped `routing` and `retry` as error kinds
/// for Gemini-CLI; the connector's classification table never accepted
/// those — both upstream-API failure flavors collapse to `network`.
/// Idempotent: returns `false` if nothing matched.
fn coerce_legacy_error_kind(source: &mut MappingSource) -> bool {
    let attrs = match source {
        MappingSource::SynthesizeFromNative {
            inject_attributes, ..
        } => inject_attributes,
        MappingSource::HookRule {
            emit: Some(HookEmit { attributes, .. }),
            ..
        } => attributes,
        MappingSource::HookRule { emit: None, .. } => return false,
    };
    let Some(v) = attrs.get_mut("error.kind") else {
        return false;
    };
    if v == "routing" || v == "retry" {
        *v = "network".to_string();
        true
    } else {
        false
    }
}

/// Migrate the legacy Claude Desktop `api_request` hook-rule layout
/// (single `when` key fanning out to events/tokens/cost/duration) to
/// the v2 per-facet keys (`api_request.tokens.input`,
/// `api_request.tokens.output`, `api_request.cost`). The watcher emits
/// per-facet observations now; legacy rules silently dropped data
/// points because nothing matched their `when` key. Idempotent.
fn split_legacy_api_request_fanout(sources: &mut Vec<MappingSource>) -> bool {
    let mut changed = false;
    let mut next: Vec<MappingSource> = Vec::with_capacity(sources.len());
    for source in sources.drain(..) {
        match &source {
            MappingSource::HookRule {
                when,
                emit: Some(emit),
            } if when == "api_request"
                && emit.metric == TierAMetric::Tokens.id()
                && !emit.attributes.contains_key("direction") =>
            {
                changed = true;
                let mut input_attrs = emit.attributes.clone();
                input_attrs.insert("direction".into(), "input".into());
                next.push(MappingSource::HookRule {
                    when: "api_request.tokens.input".into(),
                    emit: Some(HookEmit {
                        metric: emit.metric.clone(),
                        attributes: input_attrs,
                    }),
                });
                let mut output_attrs = emit.attributes.clone();
                output_attrs.insert("direction".into(), "output".into());
                next.push(MappingSource::HookRule {
                    when: "api_request.tokens.output".into(),
                    emit: Some(HookEmit {
                        metric: emit.metric.clone(),
                        attributes: output_attrs,
                    }),
                });
            }
            MappingSource::HookRule {
                when,
                emit: Some(emit),
            } if when == "api_request" && emit.metric == TierAMetric::CostUsd.id() => {
                changed = true;
                next.push(MappingSource::HookRule {
                    when: "api_request.cost".into(),
                    emit: Some(emit.clone()),
                });
            }
            _ => next.push(source),
        }
    }
    *sources = next;
    changed
}

/// Drop the legacy Cline `api_req_finished → tokens` rule. Pre-v2
/// defaults shipped this rule speculatively but the Cline watcher
/// never classified `api_req_finished` rows (only `say.text|tool|
/// command|error`), so the rule was dead from day one. It also
/// produced a UI warning because Tokens requires `direction` and the
/// rule had empty attributes. Returns `true` if anything was removed.
fn drop_legacy_cline_api_req_finished(sources: &mut Vec<MappingSource>) -> bool {
    let before = sources.len();
    sources.retain(|s| match s {
        MappingSource::HookRule {
            when,
            emit: Some(emit),
        } => !(when == "api_req_finished"
            && emit.metric == TierAMetric::Tokens.id()
            && emit.attributes.is_empty()),
        _ => true,
    });
    sources.len() != before
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
            target_metric: TierAMetric::Tokens.id(),
            attribute_map: BTreeMap::from([("type".into(), "direction".into())]),
            inject_attributes: BTreeMap::new(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"synthesize-from-native\""));
        assert!(json.contains("\"nativeMetric\":\"claude_code.token.usage\""));
        // Builtin ids serialize byte-identically to the v1 wire form so
        // existing state.json docs round-trip without rewriting any
        // targetMetric references.
        assert!(json.contains("\"targetMetric\":\"tokens\""));
        let back: MappingSource = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn mapping_source_synthesize_with_inject_attributes_round_trips() {
        let s = MappingSource::SynthesizeFromNative {
            native_metric: "gemini_cli.api.request.count".into(),
            target_metric: TierAMetric::Events.id(),
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
    fn mapping_source_synthesize_accepts_custom_metric_id() {
        // Custom metric ids carry through serde unchanged. Validation
        // catches refs that don't resolve to a catalog entry; the
        // type itself is intentionally open.
        let s = MappingSource::SynthesizeFromNative {
            native_metric: "claude_code.tool.usage".into(),
            target_metric: "my.team.tool_calls".to_string(),
            attribute_map: BTreeMap::new(),
            inject_attributes: BTreeMap::new(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"targetMetric\":\"my.team.tool_calls\""));
        let back: MappingSource = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn trove_metric_definition_round_trips_with_camel_case() {
        let d = TroveMetricDefinition {
            id: "tool_calls".to_string(),
            name: "my.team.tool_calls".to_string(),
            kind: TroveMetricKind::Counter,
            description: "tool invocations".to_string(),
            required_attributes: vec!["tool.name".to_string()],
            builtin: false,
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"id\":\"tool_calls\""));
        assert!(json.contains("\"kind\":\"counter\""));
        assert!(json.contains("\"requiredAttributes\":[\"tool.name\"]"));
        assert!(json.contains("\"builtin\":false"));
        let back: TroveMetricDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn tier_a_metric_definition_seeds_builtin_with_expected_shape() {
        let d = TierAMetric::TurnDuration.definition();
        assert_eq!(d.id, "turn.duration");
        assert_eq!(d.name, "trove.harness.turn.duration");
        assert_eq!(d.kind, TroveMetricKind::Histogram);
        assert_eq!(d.required_attributes, vec!["event.kind".to_string()]);
        assert!(d.builtin);
    }

    #[test]
    fn migrate_to_current_seeds_missing_builtins_idempotently() {
        // Simulate a v1 state freshly deserialized — schema_version 1,
        // no metrics field. After migrate_to_current, the catalog
        // carries every builtin and the version is bumped.
        let mut state = MappingState {
            schema_version: 1,
            metrics: vec![],
            harnesses: vec![],
        };
        assert!(state.migrate_to_current(), "first migration should report changes");
        assert_eq!(state.schema_version, MAPPING_SCHEMA_VERSION);
        assert_eq!(state.metrics.len(), 5);
        for builtin in TierAMetric::all() {
            assert!(state.metrics.iter().any(|m| m.id == builtin.id()));
        }
        // Re-running is a no-op and reports no changes.
        let copy = state.clone();
        assert!(!state.migrate_to_current(), "second migration should be a no-op");
        assert_eq!(state, copy);
    }

    #[test]
    fn migrate_to_current_preserves_custom_metrics() {
        // A v2 state that already has a custom entry must not lose it
        // when migrate_to_current runs again (defensive — the loader
        // calls migrate unconditionally).
        let mut state = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            metrics: vec![TroveMetricDefinition {
                id: "tool_calls".to_string(),
                name: "my.team.tool_calls".to_string(),
                kind: TroveMetricKind::Counter,
                description: String::new(),
                required_attributes: vec!["tool.name".to_string()],
                builtin: false,
            }],
            harnesses: vec![],
        };
        state.migrate_to_current();
        assert!(state.metrics.iter().any(|m| m.id == "tool_calls" && !m.builtin));
        // All builtins also present.
        for builtin in TierAMetric::all() {
            assert!(state.metrics.iter().any(|m| m.id == builtin.id()));
        }
    }

    #[test]
    fn migrate_coerces_legacy_error_kind_routing_and_retry_to_network() {
        // Gemini-CLI's pre-v2 defaults injected error.kind=routing|retry
        // which the connector's closed-enum table never accepted. The
        // migration coerces them to the canonical "network" bucket so
        // the UI no longer surfaces them as validation errors.
        let mut state = MappingState {
            schema_version: 1,
            metrics: vec![],
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::GeminiCli,
                enabled: true,
                sources: vec![
                    MappingSource::SynthesizeFromNative {
                        native_metric: "gemini_cli.model_routing.failure.count".into(),
                        target_metric: TierAMetric::Errors.id(),
                        attribute_map: BTreeMap::new(),
                        inject_attributes: BTreeMap::from([(
                            "error.kind".into(),
                            "routing".into(),
                        )]),
                    },
                    MappingSource::SynthesizeFromNative {
                        native_metric: "gemini_cli.chat.content_retry_failure.count".into(),
                        target_metric: TierAMetric::Errors.id(),
                        attribute_map: BTreeMap::new(),
                        inject_attributes: BTreeMap::from([(
                            "error.kind".into(),
                            "retry".into(),
                        )]),
                    },
                ],
                cost_overrides: BTreeMap::new(),
            }],
        };
        assert!(state.migrate_to_current());
        let h = &state.harnesses[0];
        for source in &h.sources {
            let MappingSource::SynthesizeFromNative {
                inject_attributes, ..
            } = source
            else {
                panic!("expected SynthesizeFromNative");
            };
            assert_eq!(
                inject_attributes.get("error.kind").map(String::as_str),
                Some("network")
            );
        }
    }

    #[test]
    fn migrate_splits_legacy_claude_desktop_api_request_fanout() {
        // Pre-v2 Claude Desktop had a single `when=api_request` rule
        // emitting tokens and another emitting cost.usd. The v2 watcher
        // emits per-facet observations (`api_request.tokens.input`,
        // `api_request.tokens.output`, `api_request.cost`), so the old
        // rules silently dropped data points. Migration rewrites them
        // to match the new keys.
        let mut state = MappingState {
            schema_version: 1,
            metrics: vec![],
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::ClaudeDesktop,
                enabled: true,
                sources: vec![
                    MappingSource::HookRule {
                        when: "api_request".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::Tokens.id(),
                            attributes: BTreeMap::new(),
                        }),
                    },
                    MappingSource::HookRule {
                        when: "api_request".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::CostUsd.id(),
                            attributes: BTreeMap::from([(
                                "cost.method".into(),
                                "exact".into(),
                            )]),
                        }),
                    },
                ],
                cost_overrides: BTreeMap::new(),
            }],
        };
        assert!(state.migrate_to_current());
        let whens: Vec<&str> = state.harnesses[0]
            .sources
            .iter()
            .filter_map(|s| match s {
                MappingSource::HookRule { when, .. } => Some(when.as_str()),
                MappingSource::SynthesizeFromNative { .. } => None,
            })
            .collect();
        assert!(whens.contains(&"api_request.tokens.input"));
        assert!(whens.contains(&"api_request.tokens.output"));
        assert!(whens.contains(&"api_request.cost"));
        // The legacy `api_request → tokens|cost.usd` rules should be gone.
        let leftover = state.harnesses[0].sources.iter().any(|s| match s {
            MappingSource::HookRule {
                when,
                emit: Some(emit),
            } => {
                when == "api_request"
                    && (emit.metric == TierAMetric::Tokens.id()
                        || emit.metric == TierAMetric::CostUsd.id())
            }
            _ => false,
        });
        assert!(!leftover, "legacy single-key tokens/cost rules should be removed");
    }

    #[test]
    fn migrate_drops_legacy_cline_api_req_finished_tokens_rule() {
        // Cline's pre-v2 default shipped a speculative
        // `api_req_finished → tokens` rule that the watcher never
        // matched (only `say.*` rows are classified). Migration removes
        // it so the UI no longer surfaces the missing-direction warning.
        let mut state = MappingState {
            schema_version: 1,
            metrics: vec![],
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::Cline,
                enabled: true,
                sources: vec![
                    MappingSource::HookRule {
                        when: "say.text".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::Events.id(),
                            attributes: BTreeMap::from([(
                                "event.kind".into(),
                                "chat.turn".into(),
                            )]),
                        }),
                    },
                    MappingSource::HookRule {
                        when: "api_req_finished".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::Tokens.id(),
                            attributes: BTreeMap::new(),
                        }),
                    },
                ],
                cost_overrides: BTreeMap::new(),
            }],
        };
        assert!(state.migrate_to_current());
        let cline = &state.harnesses[0];
        assert_eq!(cline.sources.len(), 1, "legacy tokens rule should be dropped");
        // The non-legacy rule survives.
        let MappingSource::HookRule { when, .. } = &cline.sources[0] else {
            panic!("expected HookRule");
        };
        assert_eq!(when, "say.text");
    }

    #[test]
    fn migrate_does_not_drop_user_authored_api_req_finished_rule() {
        // A user who deliberately wires `api_req_finished` to a custom
        // metric (or supplies a non-empty attribute map) gets to keep
        // it — the migration only drops the exact legacy default shape.
        let mut state = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            metrics: default_metric_catalog(),
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::Cline,
                enabled: true,
                sources: vec![MappingSource::HookRule {
                    when: "api_req_finished".into(),
                    emit: Some(HookEmit {
                        metric: TierAMetric::Tokens.id(),
                        attributes: BTreeMap::from([("direction".into(), "input".into())]),
                    }),
                }],
                cost_overrides: BTreeMap::new(),
            }],
        };
        let before = state.harnesses[0].sources.clone();
        state.migrate_to_current();
        assert_eq!(state.harnesses[0].sources, before);
    }

    #[test]
    fn migrate_to_current_leaves_already_correct_state_alone() {
        // A v2 default_state should pass through migration with no
        // change — important because the loader persists on change and
        // we don't want to thrash the disk on every launch.
        let mut state = default_state();
        assert!(!state.migrate_to_current(), "default state should already be current");
    }

    #[test]
    fn mapping_state_metric_lookup_returns_builtin() {
        let s = default_state();
        let m = s.metric("events").expect("events builtin should be seeded");
        assert!(m.builtin);
        assert_eq!(m.kind, TroveMetricKind::Counter);
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
