//! Rules-driven Tier A metric emission for hook/watcher harnesses.
//!
//! Each Tier 2/3 watcher (Cline, Gemini, Claude Desktop, Aider,
//! Copilot-CLI) observes raw events from its harness and used to emit
//! hardcoded `trove.harness.*` OTLP metrics. v2 replaces that hardcoded
//! emission with a runtime that consults [`MappingState`] at emit time:
//!
//! - For each observed event the watcher classifies a `when` key (e.g.
//!   `"say.tool"` for Cline, `"api_request"` for Claude Desktop).
//! - This module walks every enabled [`MappingSource::HookRule`] for
//!   the harness whose `when` matches and, for each rule with an emit
//!   payload, appends an observation against the rule's target metric.
//! - At flush time, observations are grouped by target metric id and
//!   serialized to OTLP/HTTP/JSON. The metric's wire name and kind
//!   come from the catalog entry, so renaming a builtin or pointing a
//!   rule at a custom metric just works.
//!
//! Three observation flavors cover the watchers' needs today:
//!
//! - **Counter increment** — `observe_count(when, increment, attrs)`.
//!   Used for events (chat.turn, tool.call, ...) and errors. Integer.
//! - **Histogram observation** — `observe_histogram(when, value_seconds, attrs)`.
//!   Used for turn.duration. Single-sample observation; bucket bounds
//!   come from the catalog entry (defaults shared across all v2
//!   histograms; configurable bounds is a follow-up).
//! - **Double sum** — `observe_double_sum(when, value, attrs)`. Used
//!   for cost.usd-style monetary sums. Routes to a Sum metric with
//!   `asDouble` data points.
//!
//! `attrs` on each observation are *per-observation* additions — things
//! the watcher knows from its data source (e.g. `model="claude-sonnet"`
//! or `cline.task_id="…"`) that supplement the rule's static
//! `emit.attributes`. The merge is observation-wins-over-rule for
//! duplicate keys, which lets a watcher override an injected literal
//! (rare, but useful when the data source carries a more specific
//! classification than the rule could).
//!
//! The runtime is sync — watchers call `observe_*` synchronously from
//! their poll loop, then `build()` to get the OTLP metric array.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{HarnessMapping, MappingSource, MappingState, TroveMetricDefinition, TroveMetricKind};
use crate::harness::HarnessId;

/// Shared bucket bounds for v2 histogram metrics. Matches the bounds
/// `wrapper_common_metrics` and `claude_desktop_watcher` use today for
/// `turn.duration`. Per-metric bucket configuration is a follow-up.
pub const DEFAULT_HISTOGRAM_BUCKET_BOUNDS_SECONDS: &[f64] = &[
    0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0, 120.0, 300.0, 600.0,
];

/// Builds OTLP/HTTP/JSON metric arrays from rules + observations. One
/// instance per emission batch — watchers create a fresh one at the
/// start of each poll/scan, accumulate observations, then call
/// [`Self::build`] to consume it.
#[derive(Debug)]
pub struct MetricsAccumulator<'a> {
    state: Arc<MappingState>,
    harness: HarnessId,
    /// Cached lookup of the harness's mapping (None when disabled or
    /// missing); short-circuits per-observation walks.
    mapping: Option<&'a HarnessMapping>,
    /// Per (`target_metric_id`, `attribute_signature`) observation buckets.
    /// Attribute signature is a sorted-key canonicalized form so the
    /// dedupe survives different insertion orders.
    buckets: BTreeMap<(String, AttributeSignature), Observation>,
    /// Order of first insertion for each (id, sig) pair, so the
    /// resulting OTLP keeps a deterministic, source-order-preserving
    /// layout — matters for golden-file tests in each watcher.
    order: Vec<(String, AttributeSignature)>,
}

/// What got observed against a (target metric, attribute set) bucket.
/// Variants reflect the OTLP shapes the v2 builtin catalog uses.
#[derive(Debug)]
enum Observation {
    /// Cumulative integer counter (Sum/asInt). Multiple
    /// `observe_count` calls into the same bucket sum.
    CounterInt(i64),
    /// Cumulative double-precision counter (Sum/asDouble). For
    /// monetary sums like `cost.usd`.
    CounterDouble(f64),
    /// Single-sample histogram observation, in seconds. The Trove v2
    /// runtime emits one data point per observation rather than a
    /// pre-bucketed aggregation, so each call here records the raw
    /// value. Bucket counts are computed at `build` time.
    Histogram(Vec<f64>),
    /// Gauge — last-write-wins on the value. Used by future custom
    /// metrics; no builtin uses Gauge today.
    Gauge(f64),
}

/// Stable canonical form of an attribute map. We need equality + hash
/// across two observations to know they target the same data point, so
/// `BTreeMap<String, String>`-as-key works directly.
type AttributeSignature = BTreeMap<String, String>;

impl<'a> MetricsAccumulator<'a> {
    /// Build an accumulator pointing at `harness`'s rules in `state`.
    /// Cheap; the state Arc isn't cloned beyond storing it.
    #[must_use]
    pub fn new(state: Arc<MappingState>, harness: HarnessId) -> Self {
        let mapping = state.for_harness(harness).filter(|h| h.enabled);
        // SAFETY: We hold `state` (the Arc) for the accumulator's
        // lifetime, so `mapping` (a borrow of one of its harnesses)
        // is valid for as long as `self` is. The transmute extends
        // the borrow to `'a` (the accumulator's lifetime) — anchored
        // by the owned Arc.
        let mapping: Option<&'a HarnessMapping> = mapping.map(|m| unsafe {
            std::mem::transmute::<&HarnessMapping, &'a HarnessMapping>(m)
        });
        Self {
            state,
            harness,
            mapping,
            buckets: BTreeMap::new(),
            order: Vec::new(),
        }
    }

    /// Record `count` observations of an event whose raw classification
    /// is `when`. `extra_attrs` is per-observation context (e.g.
    /// `cline.task_id`). Each matching rule contributes one bucket.
    ///
    /// When no rule matches `when`, nothing is emitted — disabled
    /// harnesses and unmapped events are silently dropped, mirroring
    /// the user's intent in the editor.
    pub fn observe_count(
        &mut self,
        when: &str,
        count: i64,
        extra_attrs: &BTreeMap<String, String>,
    ) {
        if count == 0 {
            return;
        }
        // Counter observations only fire rules whose target is a
        // counter/gauge in the catalog. Histogram rules with the same
        // `when` (e.g. wrapper.invocation rule targeting turn.duration
        // alongside an events rule) are skipped here — they pick up
        // the matching `observe_histogram` call instead. Without this
        // kind-filter, a wrapper.invocation event would silently land
        // a `count=1` data point on the histogram, polluting bucket
        // counts.
        let tuples = self.matching_rules(when, extra_attrs, |k| {
            matches!(k, TroveMetricKind::Counter | TroveMetricKind::Gauge)
        });
        for (target_id, signature, kind) in tuples {
            let key = (target_id, signature);
            let is_new = !self.buckets.contains_key(&key);
            let bucket = self.buckets.entry(key.clone()).or_insert_with(|| match kind {
                TroveMetricKind::Gauge => Observation::Gauge(0.0),
                // Counter and Histogram both seed as an int counter; the
                // histogram branch is unreachable here because we filter
                // on Counter|Gauge above, but match exhaustiveness
                // requires a body.
                TroveMetricKind::Counter | TroveMetricKind::Histogram => {
                    Observation::CounterInt(0)
                }
            });
            if is_new {
                self.order.push(key);
            }
            #[allow(clippy::cast_precision_loss)]
            match bucket {
                Observation::CounterInt(v) => *v += count,
                Observation::CounterDouble(v) => *v += count as f64,
                Observation::Gauge(v) => *v = count as f64,
                Observation::Histogram(_) => {}
            }
        }
    }

    /// Record a single histogram observation (seconds). Only fires
    /// rules whose target is a histogram. The runtime records the raw
    /// value; bucket counts are computed at `build`.
    pub fn observe_histogram(
        &mut self,
        when: &str,
        value_seconds: f64,
        extra_attrs: &BTreeMap<String, String>,
    ) {
        if !value_seconds.is_finite() || value_seconds < 0.0 {
            return;
        }
        let tuples = self
            .matching_rules(when, extra_attrs, |k| matches!(k, TroveMetricKind::Histogram));
        for (target_id, signature, _kind) in tuples {
            let key = (target_id, signature);
            let is_new = !self.buckets.contains_key(&key);
            let bucket = self
                .buckets
                .entry(key.clone())
                .or_insert_with(|| Observation::Histogram(Vec::new()));
            if is_new {
                self.order.push(key);
            }
            if let Observation::Histogram(v) = bucket {
                v.push(value_seconds);
            }
        }
    }

    /// Record a monetary or floating-point observation. Routes to a
    /// `Sum/asDouble`. Targets the same kinds as `observe_count`
    /// (counter/gauge) but always uses double-precision values — the
    /// int-vs-double distinction in the catalog is a v2 follow-up; for
    /// now `observe_double_sum` is the sole entry point for double
    /// counters and is used by cost.usd emission.
    pub fn observe_double_sum(
        &mut self,
        when: &str,
        value: f64,
        extra_attrs: &BTreeMap<String, String>,
    ) {
        if !value.is_finite() {
            return;
        }
        let tuples = self.matching_rules(when, extra_attrs, |k| {
            matches!(k, TroveMetricKind::Counter | TroveMetricKind::Gauge)
        });
        for (target_id, signature, _kind) in tuples {
            let key = (target_id, signature);
            let is_new = !self.buckets.contains_key(&key);
            let bucket = self
                .buckets
                .entry(key.clone())
                .or_insert_with(|| Observation::CounterDouble(0.0));
            if is_new {
                self.order.push(key);
            }
            #[allow(clippy::cast_possible_truncation)]
            match bucket {
                Observation::CounterDouble(v) => *v += value,
                Observation::CounterInt(v) => *v += value as i64,
                Observation::Gauge(v) => *v = value,
                Observation::Histogram(v) => v.push(value),
            }
        }
    }

    /// Collect `(target_id, signature, kind)` tuples for every enabled
    /// rule whose `when` matches and whose target metric's kind passes
    /// `kind_filter`. Pure function over `self` and the observation
    /// context — no mutation. Callers then drive the bucket inserts
    /// in their own loop, which the borrow checker accepts (vs. trying
    /// to mutate self.buckets from within a closure that also reads
    /// self.mapping).
    fn matching_rules(
        &self,
        when: &str,
        extra_attrs: &BTreeMap<String, String>,
        kind_filter: impl Fn(TroveMetricKind) -> bool,
    ) -> Vec<(String, AttributeSignature, TroveMetricKind)> {
        let Some(mapping) = self.mapping else { return Vec::new() };
        let mut tuples: Vec<(String, AttributeSignature, TroveMetricKind)> = Vec::new();
        for source in &mapping.sources {
            let MappingSource::HookRule {
                when: rule_when,
                emit: Some(emit),
            } = source
            else {
                continue;
            };
            if rule_when != when {
                continue;
            }
            let Some(def) = self.state.metric(&emit.metric) else {
                continue;
            };
            if !kind_filter(def.kind) {
                continue;
            }
            let mut signature: AttributeSignature = emit
                .attributes
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (k, v) in extra_attrs {
                signature.insert(k.clone(), v.clone());
            }
            tuples.push((emit.metric.clone(), signature, def.kind));
        }
        tuples
    }

    /// Emit OTLP/HTTP/JSON metrics for every accumulated bucket. The
    /// returned array is ready to embed under `scopeMetrics[0].metrics`.
    /// Returns an empty array when no rule matched any observation —
    /// callers should skip the OTLP POST entirely in that case.
    ///
    /// `now_ns` is the wall-clock timestamp (Unix nanoseconds) the
    /// caller wants stamped on every data point's `timeUnixNano`. Pass
    /// the same value as the watcher's batch timestamp so all metrics
    /// in one OTLP request share a coherent moment.
    #[must_use]
    pub fn build(&self, start_ns: &str, end_ns: &str) -> Vec<Value> {
        // Group buckets by target metric id so we emit one OTLP metric
        // (possibly with multiple data points) per target.
        let mut grouped: BTreeMap<String, Vec<(&AttributeSignature, &Observation)>> =
            BTreeMap::new();
        for (target_id, sig) in &self.order {
            let Some(obs) = self.buckets.get(&(target_id.clone(), sig.clone())) else {
                continue;
            };
            grouped.entry(target_id.clone()).or_default().push((sig, obs));
        }

        let mut out: Vec<Value> = Vec::new();
        for (target_id, data) in grouped {
            let Some(def) = self.state.metric(&target_id) else { continue };
            // Decide the OTLP shape based on what's actually inside the
            // bucket, not just the catalog kind — that's how cost.usd
            // (counter + double values) coexists with int counters
            // under a common Counter kind.
            let has_double = data
                .iter()
                .any(|(_, o)| matches!(o, Observation::CounterDouble(_)));
            let any_histogram = data.iter().any(|(_, o)| matches!(o, Observation::Histogram(_)));
            let any_gauge = data.iter().any(|(_, o)| matches!(o, Observation::Gauge(_)));

            let metric_json = if any_histogram {
                build_histogram_metric(def, &data, start_ns, end_ns)
            } else if any_gauge {
                build_gauge_metric(def, &data, end_ns)
            } else if has_double {
                build_sum_metric(def, &data, start_ns, end_ns, /*as_double=*/ true)
            } else {
                build_sum_metric(def, &data, start_ns, end_ns, /*as_double=*/ false)
            };
            out.push(metric_json);
        }
        out
    }

    /// Whether any observations have been recorded against rules.
    /// Caller skips the OTLP POST when this returns false to avoid
    /// emitting an empty payload.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// The harness id this accumulator is keyed against. Mostly useful
    /// for tracing/logging.
    #[must_use]
    pub fn harness_id(&self) -> HarnessId {
        self.harness
    }
}

fn attrs_to_json(sig: &AttributeSignature) -> Vec<Value> {
    sig.iter()
        .map(|(k, v)| json!({"key": k, "value": {"stringValue": v}}))
        .collect()
}

fn build_sum_metric(
    def: &TroveMetricDefinition,
    data: &[(&AttributeSignature, &Observation)],
    start_ns: &str,
    end_ns: &str,
    as_double: bool,
) -> Value {
    let data_points: Vec<Value> = data
        .iter()
        .map(|(sig, obs)| {
            #[allow(clippy::cast_precision_loss)]
            let value = match obs {
                Observation::CounterInt(v) => {
                    if as_double {
                        json!({"asDouble": *v as f64})
                    } else {
                        json!({"asInt": v.to_string()})
                    }
                }
                Observation::CounterDouble(v) | Observation::Gauge(v) => {
                    json!({"asDouble": *v})
                }
                Observation::Histogram(_) => json!({"asInt": "0"}),
            };
            // Merge the typed value object into the data-point JSON
            // alongside the attributes / timestamps.
            let mut point = json!({
                "startTimeUnixNano": start_ns,
                "timeUnixNano": end_ns,
                "attributes": attrs_to_json(sig),
            });
            if let Value::Object(ref mut point_map) = point {
                if let Value::Object(value_map) = value {
                    for (k, v) in value_map {
                        point_map.insert(k, v);
                    }
                }
            }
            point
        })
        .collect();
    json!({
        "name": def.name,
        "unit": metric_unit(&def.id),
        "description": metric_description(def),
        "sum": {
            "aggregationTemporality": 1,  // delta
            "isMonotonic": true,
            "dataPoints": data_points,
        }
    })
}

fn build_gauge_metric(
    def: &TroveMetricDefinition,
    data: &[(&AttributeSignature, &Observation)],
    end_ns: &str,
) -> Value {
    let data_points: Vec<Value> = data
        .iter()
        .map(|(sig, obs)| {
            #[allow(clippy::cast_precision_loss)]
            let v = match obs {
                Observation::Gauge(v) | Observation::CounterDouble(v) => *v,
                Observation::CounterInt(v) => *v as f64,
                Observation::Histogram(_) => 0.0,
            };
            json!({
                "timeUnixNano": end_ns,
                "asDouble": v,
                "attributes": attrs_to_json(sig),
            })
        })
        .collect();
    json!({
        "name": def.name,
        "unit": metric_unit(&def.id),
        "description": metric_description(def),
        "gauge": { "dataPoints": data_points }
    })
}

fn build_histogram_metric(
    def: &TroveMetricDefinition,
    data: &[(&AttributeSignature, &Observation)],
    start_ns: &str,
    end_ns: &str,
) -> Value {
    let bounds = DEFAULT_HISTOGRAM_BUCKET_BOUNDS_SECONDS;
    let data_points: Vec<Value> = data
        .iter()
        .map(|(sig, obs)| {
            #[allow(clippy::cast_precision_loss)]
            let samples: Vec<f64> = match obs {
                Observation::Histogram(v) => v.clone(),
                Observation::CounterInt(v) => vec![*v as f64],
                Observation::CounterDouble(v) | Observation::Gauge(v) => vec![*v],
            };
            let count = samples.len() as u64;
            let sum: f64 = samples.iter().sum();
            let mut bucket_counts: Vec<u64> = vec![0; bounds.len() + 1];
            for s in &samples {
                let mut placed = false;
                for (i, &b) in bounds.iter().enumerate() {
                    if *s <= b {
                        bucket_counts[i] += 1;
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    if let Some(last) = bucket_counts.last_mut() {
                        *last += 1;
                    }
                }
            }
            json!({
                "startTimeUnixNano": start_ns,
                "timeUnixNano": end_ns,
                "count": count.to_string(),
                "sum": sum,
                "bucketCounts": bucket_counts.iter().map(u64::to_string).collect::<Vec<_>>(),
                "explicitBounds": bounds,
                "attributes": attrs_to_json(sig),
            })
        })
        .collect();
    json!({
        "name": def.name,
        "unit": "s",
        "description": metric_description(def),
        "histogram": {
            "aggregationTemporality": 1,
            "dataPoints": data_points,
        }
    })
}

fn metric_unit(id: &str) -> &'static str {
    match id {
        "cost.usd" => "USD",
        "turn.duration" => "s",
        _ => "1",
    }
}

fn metric_description(def: &TroveMetricDefinition) -> String {
    if def.description.is_empty() {
        match def.id.as_str() {
            "events" => "Count of harness events processed by Trove.".to_string(),
            "tokens" => "Token usage by direction.".to_string(),
            "cost.usd" => "Estimated USD cost.".to_string(),
            "turn.duration" => "Per-turn duration in seconds.".to_string(),
            "errors" => "Count of harness errors observed by Trove.".to_string(),
            _ => format!("Custom metric {}.", def.name),
        }
    } else {
        def.description.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use crate::harness::HarnessId;
    use crate::mappings::{
        default_state, HarnessMapping, HookEmit, MappingSource, TierAMetric, TroveMetricDefinition,
        TroveMetricKind,
    };

    use super::*;

    /// Build a fresh `MappingState` with a single hook-rule harness for
    /// the chosen `when → metric_id` mapping. Catalog inherits the
    /// builtin set.
    fn state_with_single_rule(harness: HarnessId, when: &str, metric_id: &str) -> Arc<MappingState> {
        let mut state = default_state();
        // Drop existing mapping for this harness; we want a clean rule.
        state.harnesses.retain(|h| h.harness_id != harness);
        state.harnesses.push(HarnessMapping {
            harness_id: harness,
            enabled: true,
            sources: vec![MappingSource::HookRule {
                when: when.to_string(),
                emit: Some(HookEmit {
                    metric: metric_id.to_string(),
                    attributes: BTreeMap::new(),
                }),
            }],
            cost_overrides: BTreeMap::new(),
        });
        Arc::new(state)
    }

    #[test]
    fn observe_count_with_no_matching_rule_emits_nothing() {
        let state = state_with_single_rule(HarnessId::Cline, "say.text", "events");
        let mut acc = MetricsAccumulator::new(state, HarnessId::Cline);
        acc.observe_count("say.unknown", 5, &BTreeMap::new());
        assert!(acc.is_empty());
        assert!(acc.build("0", "0").is_empty());
    }

    #[test]
    fn observe_count_routes_to_builtin_target() {
        let state = state_with_single_rule(HarnessId::Cline, "say.text", "events");
        let mut acc = MetricsAccumulator::new(state, HarnessId::Cline);
        let extras: BTreeMap<String, String> =
            [("event.kind".to_string(), "chat.turn".to_string())].into_iter().collect();
        acc.observe_count("say.text", 3, &extras);
        let metrics = acc.build("0", "1");
        assert_eq!(metrics.len(), 1);
        let m = &metrics[0];
        assert_eq!(m["name"], "trove.harness.events");
        let dp = &m["sum"]["dataPoints"][0];
        assert_eq!(dp["asInt"], "3");
        // event.kind from extras lands on the data point.
        let attrs = dp["attributes"].as_array().unwrap();
        assert!(
            attrs
                .iter()
                .any(|a| a["key"] == "event.kind" && a["value"]["stringValue"] == "chat.turn")
        );
    }

    #[test]
    fn observe_count_routes_to_custom_metric_wire_name() {
        let mut state = default_state();
        // Add a custom metric and a rule pointing at it.
        state.metrics.push(TroveMetricDefinition {
            id: "tool_calls".to_string(),
            name: "my.team.tool_calls".to_string(),
            kind: TroveMetricKind::Counter,
            description: String::new(),
            required_attributes: vec![],
            builtin: false,
        });
        state.harnesses.retain(|h| h.harness_id != HarnessId::Cline);
        state.harnesses.push(HarnessMapping {
            harness_id: HarnessId::Cline,
            enabled: true,
            sources: vec![MappingSource::HookRule {
                when: "say.tool".to_string(),
                emit: Some(HookEmit {
                    metric: "tool_calls".to_string(),
                    attributes: BTreeMap::new(),
                }),
            }],
            cost_overrides: BTreeMap::new(),
        });
        let state = Arc::new(state);
        let mut acc = MetricsAccumulator::new(state, HarnessId::Cline);
        acc.observe_count("say.tool", 1, &BTreeMap::new());
        let metrics = acc.build("0", "1");
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0]["name"], "my.team.tool_calls");
    }

    #[test]
    fn disabled_harness_emits_nothing() {
        let mut state = default_state();
        for h in &mut state.harnesses {
            if h.harness_id == HarnessId::Cline {
                h.enabled = false;
            }
        }
        let state = Arc::new(state);
        let mut acc = MetricsAccumulator::new(state, HarnessId::Cline);
        acc.observe_count("say.text", 1, &BTreeMap::new());
        assert!(acc.is_empty());
    }

    #[test]
    fn suppressed_rule_emits_nothing() {
        let mut state = default_state();
        state.harnesses.retain(|h| h.harness_id != HarnessId::CursorIde);
        state.harnesses.push(HarnessMapping {
            harness_id: HarnessId::CursorIde,
            enabled: true,
            sources: vec![MappingSource::HookRule {
                when: "beforeSubmitPrompt".to_string(),
                emit: None,
            }],
            cost_overrides: BTreeMap::new(),
        });
        let state = Arc::new(state);
        let mut acc = MetricsAccumulator::new(state, HarnessId::CursorIde);
        acc.observe_count("beforeSubmitPrompt", 1, &BTreeMap::new());
        assert!(acc.is_empty());
    }

    #[test]
    fn observe_count_aggregates_into_one_data_point_per_signature() {
        let state = state_with_single_rule(HarnessId::Cline, "say.text", "events");
        let mut acc = MetricsAccumulator::new(state, HarnessId::Cline);
        let extras: BTreeMap<String, String> =
            [("event.kind".to_string(), "chat.turn".to_string())].into_iter().collect();
        acc.observe_count("say.text", 2, &extras);
        acc.observe_count("say.text", 3, &extras);
        let metrics = acc.build("0", "1");
        let dps = metrics[0]["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(dps.len(), 1);
        assert_eq!(dps[0]["asInt"], "5");
    }

    #[test]
    fn observe_count_separates_data_points_by_signature() {
        let state = state_with_single_rule(HarnessId::Cline, "say.text", "events");
        let mut acc = MetricsAccumulator::new(state, HarnessId::Cline);
        let chat: BTreeMap<String, String> =
            [("event.kind".to_string(), "chat.turn".to_string())].into_iter().collect();
        let tool: BTreeMap<String, String> =
            [("event.kind".to_string(), "tool.call".to_string())].into_iter().collect();
        acc.observe_count("say.text", 1, &chat);
        acc.observe_count("say.text", 1, &tool);
        let metrics = acc.build("0", "1");
        let dps = metrics[0]["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(dps.len(), 2);
    }

    #[test]
    fn observe_histogram_builds_histogram_metric() {
        let state = state_with_single_rule(HarnessId::Aider, "wrapper.invocation", "turn.duration");
        let mut acc = MetricsAccumulator::new(state, HarnessId::Aider);
        let extras: BTreeMap<String, String> =
            [("event.kind".to_string(), "chat.turn".to_string())].into_iter().collect();
        acc.observe_histogram("wrapper.invocation", 2.5, &extras);
        acc.observe_histogram("wrapper.invocation", 7.0, &extras);
        let metrics = acc.build("0", "1");
        assert_eq!(metrics[0]["name"], "trove.harness.turn.duration");
        let dp = &metrics[0]["histogram"]["dataPoints"][0];
        assert_eq!(dp["count"], "2");
        assert!((dp["sum"].as_f64().unwrap() - 9.5).abs() < f64::EPSILON);
    }

    #[test]
    fn observe_double_sum_emits_as_double() {
        // cost.usd default emission via a fake rule.
        let state = state_with_single_rule(HarnessId::GeminiCli, "gemini.turn", "cost.usd");
        let mut acc = MetricsAccumulator::new(state, HarnessId::GeminiCli);
        let extras: BTreeMap<String, String> = [
            ("cost.method".to_string(), "estimated".to_string()),
            ("model".to_string(), "gemini-2.5-pro".to_string()),
        ]
        .into_iter()
        .collect();
        acc.observe_double_sum("gemini.turn", 0.0042, &extras);
        let metrics = acc.build("0", "1");
        assert_eq!(metrics[0]["name"], "trove.harness.cost.usd");
        let dp = &metrics[0]["sum"]["dataPoints"][0];
        assert!(dp.get("asInt").is_none());
        assert!((dp["asDouble"].as_f64().unwrap() - 0.0042).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_with_static_inject_attribute_lands_on_data_point() {
        let mut state = default_state();
        state.harnesses.retain(|h| h.harness_id != HarnessId::Cline);
        state.harnesses.push(HarnessMapping {
            harness_id: HarnessId::Cline,
            enabled: true,
            sources: vec![MappingSource::HookRule {
                when: "say.error".to_string(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Errors.id(),
                    attributes: [
                        ("error.kind".to_string(), "unknown".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                }),
            }],
            cost_overrides: BTreeMap::new(),
        });
        let state = Arc::new(state);
        let mut acc = MetricsAccumulator::new(state, HarnessId::Cline);
        acc.observe_count("say.error", 1, &BTreeMap::new());
        let metrics = acc.build("0", "1");
        let attrs = metrics[0]["sum"]["dataPoints"][0]["attributes"]
            .as_array()
            .unwrap();
        assert!(
            attrs
                .iter()
                .any(|a| a["key"] == "error.kind" && a["value"]["stringValue"] == "unknown")
        );
    }

    #[test]
    fn observation_extras_override_rule_static_attributes() {
        // Rule injects event.kind=chat.turn; observation specifies
        // event.kind=tool.call. The observation wins.
        let state = state_with_single_rule(HarnessId::Cline, "say.text", "events");
        // Force a static attribute into the rule via a fresh state.
        // (state_with_single_rule used an empty attributes map; tweak.)
        let mut tweaked = (*state).clone();
        for h in &mut tweaked.harnesses {
            if h.harness_id == HarnessId::Cline {
                for s in &mut h.sources {
                    if let MappingSource::HookRule { emit: Some(e), .. } = s {
                        e.attributes
                            .insert("event.kind".to_string(), "chat.turn".to_string());
                    }
                }
            }
        }
        let tweaked = Arc::new(tweaked);
        let mut acc = MetricsAccumulator::new(tweaked, HarnessId::Cline);
        let extras: BTreeMap<String, String> =
            [("event.kind".to_string(), "tool.call".to_string())].into_iter().collect();
        acc.observe_count("say.text", 1, &extras);
        let metrics = acc.build("0", "1");
        let attrs = metrics[0]["sum"]["dataPoints"][0]["attributes"]
            .as_array()
            .unwrap();
        let kind = attrs
            .iter()
            .find(|a| a["key"] == "event.kind")
            .unwrap();
        assert_eq!(kind["value"]["stringValue"], "tool.call");
    }
}
