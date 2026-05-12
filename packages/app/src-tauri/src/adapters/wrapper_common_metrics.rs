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

use serde_json::{json, Value};

use super::ApplyOptions;

/// Identifying parameters for one harness's wrapper. Lets the shared
/// builder stay generic without taking on a runtime enum dispatch.
pub struct WrapperMetricsSpec {
    /// Expected `tool` field on the JSON line. Lines whose `tool`
    /// doesn't match are rejected (so an aider wrapper file never
    /// emits as copilot, etc., even if log files get swapped).
    pub expected_tool: &'static str,
    /// Resource attributes pinned per harness.
    pub service_name: &'static str,
    pub harness_id: &'static str,
    pub harness_name: &'static str,
    /// `scope.name` on the OTLP instrumentation scope. Convention:
    /// `trove.adapters.<harness>`.
    pub scope_name: &'static str,
}

/// Histogram bucket bounds shared with the Cursor hook. Mirrors
/// `resources/hooks/cursor-otel-hook-impl.cjs` so cross-harness
/// duration panels render the same buckets regardless of which
/// harness emitted the observation.
const HISTOGRAM_BOUNDS: &[f64] = &[
    0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0, 120.0, 300.0, 600.0,
];

/// Parse one wrapper-emitted JSON-line and produce a Tier A metric
/// payload. Returns `None` when the line is unparseable or carries an
/// unrelated `tool` value.
#[must_use]
pub fn build_invocation_metrics(
    line: &str,
    opts: &ApplyOptions,
    spec: &WrapperMetricsSpec,
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

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
        .to_string();
    let start_ns = now_ns.clone();

    let mut metrics: Vec<Value> = Vec::new();

    metrics.push(json!({
        "name": "trove.harness.events",
        "unit": "1",
        "description": "Count of harness events processed by Trove.",
        "sum": {
            "aggregationTemporality": 1,
            "isMonotonic": true,
            "dataPoints": [{
                "startTimeUnixNano": start_ns,
                "timeUnixNano": now_ns,
                "asInt": "1",
                "attributes": [
                    {"key": "event.kind", "value": {"stringValue": "chat.turn"}},
                ],
            }]
        }
    }));

    metrics.push(turn_duration_metric(duration_s, &start_ns, &now_ns));

    if exit != 0 {
        metrics.push(json!({
            "name": "trove.harness.errors",
            "unit": "1",
            "description": "Count of harness errors observed by Trove.",
            "sum": {
                "aggregationTemporality": 1,
                "isMonotonic": true,
                "dataPoints": [{
                    "startTimeUnixNano": start_ns,
                    "timeUnixNano": now_ns,
                    "asInt": "1",
                    "attributes": [
                        {"key": "error.kind", "value": {"stringValue": "unknown"}},
                    ],
                }]
            }
        }));
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

/// Build a one-observation histogram for the duration metric. The
/// bucket-counts vector has `bounds.len() + 1` entries; all are zero
/// except the bucket the observation falls into, where it's 1.
fn turn_duration_metric(value_s: f64, start_ns: &str, time_ns: &str) -> Value {
    let mut bucket_counts: Vec<String> = vec!["0".to_string(); HISTOGRAM_BOUNDS.len() + 1];
    let mut placed = false;
    for (i, bound) in HISTOGRAM_BOUNDS.iter().enumerate() {
        if value_s <= *bound {
            bucket_counts[i] = "1".to_string();
            placed = true;
            break;
        }
    }
    if !placed {
        let last = bucket_counts.len() - 1;
        bucket_counts[last] = "1".to_string();
    }

    json!({
        "name": "trove.harness.turn.duration",
        "unit": "s",
        "description": "Wall-clock duration of a harness turn.",
        "histogram": {
            "aggregationTemporality": 1,
            "dataPoints": [{
                "startTimeUnixNano": start_ns,
                "timeUnixNano": time_ns,
                "count": "1",
                "sum": value_s,
                "bucketCounts": bucket_counts,
                "explicitBounds": HISTOGRAM_BOUNDS,
                "attributes": [
                    {"key": "event.kind", "value": {"stringValue": "chat.turn"}},
                ],
            }]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aider_spec() -> WrapperMetricsSpec {
        WrapperMetricsSpec {
            expected_tool: "aider",
            service_name: "aider",
            harness_id: "aider",
            harness_name: "Aider",
            scope_name: "trove.adapters.aider",
        }
    }

    #[test]
    fn returns_none_for_non_json() {
        assert!(
            build_invocation_metrics("not json", &ApplyOptions::default(), &aider_spec()).is_none()
        );
    }

    #[test]
    fn returns_none_for_mismatched_tool() {
        let line = r#"{"tool":"notaider","exit_code":0,"duration_ms":50}"#;
        assert!(build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec()).is_none());
    }

    #[test]
    fn emits_events_and_duration_for_a_successful_invocation() {
        let line = r#"{"tool":"aider","argc":1,"exit_code":0,"duration_ms":2500,"ts":"2026-05-09T00:00:00Z"}"#;
        let p = build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec()).unwrap();
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
        let p = build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec()).unwrap();
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
        let p = build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec()).unwrap();
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
        let p = build_invocation_metrics(line, &opts, &aider_spec()).unwrap();
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
        let p = build_invocation_metrics(line, &ApplyOptions::default(), &aider_spec()).unwrap();
        let attrs = p["resourceMetrics"][0]["resource"]["attributes"].as_array().unwrap();
        assert!(attrs
            .iter()
            .any(|a| a["key"] == "harness.id" && a["value"]["stringValue"] == "aider"));
        assert!(attrs
            .iter()
            .any(|a| a["key"] == "service.name" && a["value"]["stringValue"] == "aider"));
    }
}
