//! Shared Tier A metric builder for the wrapper-based Tier 3 harnesses
//! (Aider, Copilot CLI). Both wrappers append one JSON-line per
//! invocation with the same `{ts, tool, argc, exit_code, duration_ms}`
//! shape — the only thing that varies between them is the resource
//! attributes (service.name, harness.id, harness.name).
//!
//! Each invocation produces:
//!
//! 1. `trove.harness.events` — one data point, `event.kind = chat.turn`,
//!    count = 1. (One wrapper invocation = one user-initiated turn.)
//! 2. `trove.harness.turn.duration` — a histogram observation in
//!    seconds, derived from the wrapper's `duration_ms` field. Bounds
//!    match the Cursor hook
//!    (`resources/hooks/cursor-otel-hook-impl.cjs`) so Tier A panels
//!    can plot duration across harnesses against the same buckets.
//! 3. `trove.harness.errors` — emitted only when `exit_code != 0`,
//!    with `error.kind = unknown`. The wrapper has no visibility into
//!    upstream failure modes, so every non-zero exit lands in the
//!    catch-all bucket today.
//!
//! Token and cost are intentionally omitted: the wrapper can't see
//! Aider's tokenizer or Copilot CLI's billing, and fabricating
//! `cost.method=estimated` numbers without a real prompt/response
//! byte count would be worse than skipping.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::ApplyOptions;
use crate::harness::HarnessId;
use crate::mappings::MappingState;

/// Identifying parameters for one harness's wrapper. Lets the shared
/// builder stay generic without taking on a runtime enum dispatch.
pub struct WrapperMetricsSpec {
    /// Expected `tool` field on the JSON line. Lines whose `tool`
    /// doesn't match are rejected (so an aider wrapper file never
    /// emits as copilot, etc., even if log files get swapped).
    pub expected_tool: &'static str,
    /// Resource attributes pinned per harness.
    pub service_name: &'static str,
    /// Catalog-aware: which harness this wrapper belongs to. Drives
    /// the rules lookup into [`crate::mappings::MappingState`].
    pub harness: HarnessId,
    pub harness_id: &'static str,
    pub harness_name: &'static str,
    /// `scope.name` on the OTLP instrumentation scope. Convention:
    /// `trove.adapters.<harness>`.
    pub scope_name: &'static str,
}

/// Parse one wrapper-emitted JSON-line and produce a Tier A metric
/// payload, applying the user's current mapping rules. Returns `None`
/// when the line is unparseable, carries an unrelated `tool` value, or
/// when no rule produces an emission for this harness's events.
///
/// The wrapper classifies its single event as `when=wrapper.invocation`
/// for the `events` and `turn.duration` facets, and as
/// `when=wrapper.error` (only when `exit_code != 0`) for `errors`. Rules
/// in [`crate::mappings::MappingState`] decide which target metrics each
/// facet flows into.
#[must_use]
pub fn build_invocation_metrics(
    line: &str,
    opts: &ApplyOptions,
    spec: &WrapperMetricsSpec,
    mappings: Arc<MappingState>,
) -> Option<Value> {
    let event: Value = serde_json::from_str(line.trim()).ok()?;
    let tool = event.get("tool").and_then(Value::as_str)?;
    if tool != spec.expected_tool {
        return None;
    }
    let exit = event.get("exit_code").and_then(Value::as_i64).unwrap_or(0);
    let elapsed_ms = event.get("duration_ms").and_then(Value::as_i64).unwrap_or(0);
    #[allow(clippy::cast_precision_loss)]
    let duration_s = (elapsed_ms.max(0) as f64) / 1000.0;

    let mut acc = crate::mappings::runtime::MetricsAccumulator::new(mappings, spec.harness);
    let no_extras: BTreeMap<String, String> = BTreeMap::new();
    // One invocation = one event for the events facet.
    acc.observe_count("wrapper.invocation", 1, &no_extras);
    // Same logical event also contributes to turn.duration as a
    // histogram observation (in seconds).
    acc.observe_histogram("wrapper.invocation", duration_s, &no_extras);
    if exit != 0 {
        acc.observe_count("wrapper.error", 1, &no_extras);
    }

    if acc.is_empty() {
        return None;
    }

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
        .to_string();
    let metrics = acc.build(&now_ns, &now_ns);
    if metrics.is_empty() {
        return None;
    }

    let mut resource_attrs = vec![
        json!({"key": "service.name", "value": {"stringValue": spec.service_name}}),
        json!({"key": "harness.id", "value": {"stringValue": spec.harness_id}}),
        json!({"key": "harness.name", "value": {"stringValue": spec.harness_name}}),
        json!({"key": "trove.source", "value": {"stringValue": spec.harness_id}}),
    ];
    for (k, v) in &opts.custom_attributes {
        resource_attrs.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    Some(json!({
        "resourceMetrics": [{
            "resource": {"attributes": resource_attrs},
            "scopeMetrics": [{
                "scope": {"name": spec.scope_name, "version": env!("CARGO_PKG_VERSION")},
                "metrics": metrics,
            }]
        }]
    }))
}

#[cfg(test)]
mod tests {

    fn rules_arc() -> std::sync::Arc<crate::mappings::MappingState> {
        std::sync::Arc::new(crate::mappings::default_state())
    }
    use super::*;

    fn aider_spec() -> WrapperMetricsSpec {
        WrapperMetricsSpec {
            expected_tool: "aider",
            service_name: "aider",
            harness: crate::harness::HarnessId::Aider,
            harness_id: "aider",
            harness_name: "Aider",
            scope_name: "trove.adapters.aider",
        }
    }

    #[test]
    fn returns_none_for_non_json() {
        assert!(
            build_invocation_metrics("not json", &ApplyOptions::default(), &aider_spec(), rules_arc()).is_none()
        );
    }

    #[test]
    fn returns_none_for_mismatched_tool() {
        let line = r#"{"tool":"notaider","exit_code":0,"duration_ms":50}"#;
        assert!(build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec(), rules_arc()).is_none());
    }

    #[test]
    fn emits_events_and_duration_for_a_successful_invocation() {
        let line = r#"{"tool":"aider","argc":1,"exit_code":0,"duration_ms":2500,"ts":"2026-05-09T00:00:00Z"}"#;
        let p = build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec(), rules_arc()).unwrap();
        let names: Vec<&str> = p["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"trove.harness.events"));
        assert!(names.contains(&"trove.harness.turn.duration"));
        // Successful exit → no errors metric.
        assert!(!names.contains(&"trove.harness.errors"));
    }

    #[test]
    fn emits_errors_for_a_nonzero_exit_code() {
        let line = r#"{"tool":"aider","argc":1,"exit_code":1,"duration_ms":100,"ts":""}"#;
        let p = build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec(), rules_arc()).unwrap();
        let names: Vec<&str> = p["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"trove.harness.errors"));
    }

    #[test]
    fn histogram_places_observation_in_the_right_bucket() {
        // 2.5s falls in the (2, 5] bucket — index 3 in
        // [0.5, 1, 2, 5, 10, ...]. The bucket counts use the OTel
        // convention bucketCounts[i] = #observations in (b[i-1], b[i]]
        // with b[-1] = -∞. So 2.5 lands in bucket index 3 (value of
        // bound 5).
        let line = r#"{"tool":"aider","exit_code":0,"duration_ms":2500,"ts":""}"#;
        let p = build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec(), rules_arc()).unwrap();
        let histogram = p["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == "trove.harness.turn.duration")
            .unwrap();
        let counts = histogram["histogram"]["dataPoints"][0]["bucketCounts"]
            .as_array()
            .unwrap();
        // index for bound 5.0 (third bound, 0-indexed = 3)
        assert_eq!(counts[3], "1");
        assert_eq!(histogram["histogram"]["dataPoints"][0]["sum"], 2.5);
    }

    #[test]
    fn custom_attributes_attach_to_the_resource() {
        let line = r#"{"tool":"aider","exit_code":0,"duration_ms":50,"ts":""}"#;
        let mut opts = ApplyOptions::default();
        opts.custom_attributes
            .insert("env".into(), "prod".into());
        let p = build_invocation_metrics(line, &opts, &aider_spec(), rules_arc()).unwrap();
        let resource_attrs = p["resourceMetrics"][0]["resource"]["attributes"]
            .as_array()
            .unwrap();
        assert!(resource_attrs
            .iter()
            .any(|a| a["key"] == "env" && a["value"]["stringValue"] == "prod"));
    }

    #[test]
    fn resource_attributes_match_the_spec() {
        let line = r#"{"tool":"aider","exit_code":0,"duration_ms":50,"ts":""}"#;
        let p = build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec(), rules_arc()).unwrap();
        let attrs = p["resourceMetrics"][0]["resource"]["attributes"].as_array().unwrap();
        assert!(attrs
            .iter()
            .any(|a| a["key"] == "harness.id" && a["value"]["stringValue"] == "aider"));
        assert!(attrs
            .iter()
            .any(|a| a["key"] == "service.name" && a["value"]["stringValue"] == "aider"));
    }
}
