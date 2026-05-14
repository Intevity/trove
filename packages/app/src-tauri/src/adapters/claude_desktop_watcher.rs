//! Claude Desktop (Cowork) watcher — tails per-session `audit.jsonl`
//! files written by the Cowork host process to
//! `~/Library/Application Support/Claude/local-agent-mode-sessions/`
//! and synthesises Tier A metrics from each completed turn.
//!
//! Why a file watcher and not OTLP: Cowork's admin-managed OTLP path is
//! broken upstream (Anthropic issue #39471 — the in-VM Claude CLI
//! doesn't actually forward under the Anthropic-managed export, and the
//! egress allowlist UI in #38984 rejects `localhost`). `audit.jsonl` is
//! the authoritative ground truth Cowork writes natively for every
//! session — same `duration_ms`, `num_turns`, `total_cost_usd`, and
//! per-model token / cost breakdown the OTLP path would carry — so we
//! read it directly. Works on Max plans, survives the in-VM sandbox,
//! and exposes per-model `costUSD` which the OTLP surface doesn't.
//!
//! Schema: each line is one event with `type`
//! (`user`/`assistant`/`system`/`result`/`rate_limit_event`) and an
//! optional `subtype`. We synthesise Tier A only on `result` rows —
//! that's the canonical turn boundary, and it's where `modelUsage` /
//! `duration_ms` / `total_cost_usd` / `is_error` / `api_error_status`
//! all live. `system/init` rows are tracked only to remember the
//! session's primary model for `turn.duration` and `errors` data
//! points (whose `model` attribute Cowork doesn't carry on the result
//! row itself in some subtypes).
//!
//! Watermarking: append-only NDJSON, so a byte offset is sufficient. A
//! SHA-256 prefix of the first 256 bytes guards against rotation (we
//! don't expect Cowork to rotate per-session files, but if it does
//! we reset rather than emit garbage). First time we observe a
//! non-empty file we **skip to EOF** — historical sessions stay in the
//! file but Trove only emits for new turns, so re-launching the app
//! doesn't double-count completed turns. Tier A is for live
//! monitoring; session forensics belong to the audit log itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::adapters::ApplyOptions;
use crate::log_watcher::WatcherHandle;
use crate::otlp_emit;

/// Process-lifetime count of successful audit.jsonl → OTLP emissions.
/// Read by `ipc::commands::list_detected_harnesses` to flip the
/// Harnesses card's telemetry pill from "Unknown" → "On" once Cowork
/// activity has been observed. The diag-log counter used for native-
/// `OTel` harnesses is logs-only and never fires for this adapter because
/// the watcher synthesises metrics directly.
static OBSERVATION_COUNT: AtomicU64 = AtomicU64::new(0);

/// Snapshot of the lifetime observation counter. Monotonic; resets on
/// process restart (acceptable — the Harnesses pill is a runtime
/// signal, not a persisted one).
#[must_use]
pub fn observation_count() -> u64 {
    OBSERVATION_COUNT.load(Ordering::Relaxed)
}

/// How often the watcher polls every `audit.jsonl` file in the Cowork
/// sessions tree. One second matches the "live" feel of the dashboard
/// counters without hammering the filesystem.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Default sessions root: macOS host-side path Cowork writes per
/// session. `home` is passed in so tests can override.
#[must_use]
pub fn sessions_root(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("Claude")
        .join("local-agent-mode-sessions")
}

/// Per-file scan state. `offset` is the next byte to read; `head_hash`
/// detects file rotation (path reused with a different identity);
/// `primary_model` is the most recent `system/init.model` for this
/// session, attached to `turn.duration` / `errors` when the `result`
/// row doesn't carry one itself.
#[derive(Debug, Default, Clone)]
struct FileWatermark {
    offset: u64,
    head_hash: String,
    primary_model: Option<String>,
}

#[must_use]
pub fn spawn(
    sessions_root: impl Into<PathBuf>,
    opts: ApplyOptions,
    poll_interval: Duration,
) -> WatcherHandle {
    let sessions_root = sessions_root.into();
    let join = tokio::spawn(async move {
        run(
            sessions_root,
            opts,
            poll_interval,
            |payload: Value| async move { otlp_emit::post_metrics_json(&payload).await },
        )
        .await;
    });
    WatcherHandle::from_join(join)
}

/// Test-friendly entrypoint. Takes an explicit metric emitter so unit
/// tests can capture payloads instead of `POST`ing to a collector.
pub async fn run<FMetric, FutMetric>(
    sessions_root: PathBuf,
    opts: ApplyOptions,
    poll_interval: Duration,
    emit_metric: FMetric,
) where
    FMetric: Fn(Value) -> FutMetric + Send + Sync + 'static,
    FutMetric: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>> + Send,
{
    let mut watermarks: HashMap<PathBuf, FileWatermark> = HashMap::new();
    loop {
        if let Err(e) = scan_once(&sessions_root, &opts, &mut watermarks, &emit_metric).await {
            tracing::warn!(error = %e, ?sessions_root, "claude-desktop watcher tick errored");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

async fn scan_once<FMetric, FutMetric>(
    sessions_root: &Path,
    opts: &ApplyOptions,
    watermarks: &mut HashMap<PathBuf, FileWatermark>,
    emit_metric: &FMetric,
) -> std::io::Result<()>
where
    FMetric: Fn(Value) -> FutMetric,
    FutMetric: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
{
    let audit_files = match discover_audit_files(sessions_root).await {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for audit_path in audit_files {
        if let Err(e) = process_one_file(&audit_path, opts, watermarks, emit_metric).await {
            tracing::warn!(
                error = %e,
                ?audit_path,
                "claude-desktop watcher could not process file"
            );
        }
    }
    Ok(())
}

/// Walk `<root>/<account>/<workspace>/local_<sid>/audit.jsonl`. The
/// `skills-plugin` directory at the account level is included
/// implicitly — its children are normal sessions on inspection.
async fn discover_audit_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut accounts = tokio::fs::read_dir(root).await?;
    while let Some(account) = accounts.next_entry().await? {
        if !account.file_type().await?.is_dir() {
            continue;
        }
        let Ok(mut workspaces) = tokio::fs::read_dir(account.path()).await else {
            continue;
        };
        while let Some(workspace) = workspaces.next_entry().await? {
            if !workspace.file_type().await?.is_dir() {
                continue;
            }
            let Ok(mut sessions) = tokio::fs::read_dir(workspace.path()).await else {
                continue;
            };
            while let Some(session) = sessions.next_entry().await? {
                if !session.file_type().await?.is_dir() {
                    continue;
                }
                let name = match session.file_name().to_str() {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if !name.starts_with("local_") {
                    continue;
                }
                let audit = session.path().join("audit.jsonl");
                if tokio::fs::metadata(&audit).await.is_ok() {
                    found.push(audit);
                }
            }
        }
    }
    Ok(found)
}

async fn process_one_file<FMetric, FutMetric>(
    audit_path: &Path,
    opts: &ApplyOptions,
    watermarks: &mut HashMap<PathBuf, FileWatermark>,
    emit_metric: &FMetric,
) -> std::io::Result<()>
where
    FMetric: Fn(Value) -> FutMetric,
    FutMetric: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
{
    use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

    let metadata = tokio::fs::metadata(audit_path).await?;
    let file_size = metadata.len();
    let is_first_observation = !watermarks.contains_key(audit_path);
    let previous = watermarks.get(audit_path).cloned().unwrap_or_default();

    // No new bytes since last scan — skip the open() cost.
    if !is_first_observation && previous.offset == file_size {
        return Ok(());
    }

    let mut file = tokio::fs::File::open(audit_path).await?;

    let mut head_buf = [0u8; 256];
    let head_n = file.read(&mut head_buf).await?;
    let head_hash = sha256_hex(&head_buf[..head_n]);

    let mut start_offset = previous.offset;
    let mut primary_model = previous.primary_model.clone();
    let rotated = !previous.head_hash.is_empty() && previous.head_hash != head_hash;
    let truncated = previous.offset > file_size;
    if rotated || truncated {
        start_offset = 0;
        primary_model = None;
    } else if is_first_observation {
        // First time we see this file: skip historical content so a
        // Trove restart doesn't double-count completed turns. We still
        // capture the head_hash so rotation is detectable later.
        start_offset = file_size;
    }

    if start_offset >= file_size {
        watermarks.insert(
            audit_path.to_path_buf(),
            FileWatermark {
                offset: file_size,
                head_hash,
                primary_model,
            },
        );
        return Ok(());
    }

    file.seek(SeekFrom::Start(start_offset)).await?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await?;

    // Only operate on complete (newline-terminated) lines. The
    // remainder is retained by leaving `offset` short of `file_size`,
    // so the next tick picks up from where the line started.
    let last_newline = match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => i + 1,
        None => 0,
    };
    let complete = &buf[..last_newline];

    let mut metrics_payloads: Vec<Value> = Vec::new();
    for line in complete.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let row: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if row.get("type").and_then(Value::as_str) == Some("system")
            && row.get("subtype").and_then(Value::as_str) == Some("init")
        {
            if let Some(m) = row.get("model").and_then(Value::as_str) {
                primary_model = Some(m.to_string());
            }
        }
        if let Some(payload) = parse_result_metric_payload(&row, primary_model.as_deref(), opts) {
            metrics_payloads.push(payload);
        }
    }

    let new_offset = start_offset + last_newline as u64;
    watermarks.insert(
        audit_path.to_path_buf(),
        FileWatermark {
            offset: new_offset,
            head_hash,
            primary_model,
        },
    );

    for payload in metrics_payloads {
        match emit_metric(payload).await {
            Ok(()) => {
                OBSERVATION_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    ?audit_path,
                    "claude-desktop watcher metric emit failed"
                );
            }
        }
    }
    Ok(())
}

/// Build an OTLP/HTTP/JSON metrics payload from one `result` row.
/// Returns `None` for rows that aren't a turn-boundary `result`.
#[must_use]
pub fn parse_result_metric_payload(
    row: &Value,
    fallback_model: Option<&str>,
    opts: &ApplyOptions,
) -> Option<Value> {
    if row.get("type").and_then(Value::as_str) != Some("result") {
        return None;
    }

    let duration_ms = row.get("duration_ms").and_then(Value::as_f64).unwrap_or(0.0);
    let total_cost_usd = row
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let is_error = row.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    let api_error_status = row
        .get("api_error_status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let model_usage = row.get("modelUsage").and_then(Value::as_object);

    let primary_model = pick_primary_model(model_usage)
        .or_else(|| fallback_model.map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
        .to_string();
    let start_ns = now_ns.clone();

    let mut metrics: Vec<Value> = Vec::new();

    // events { event.kind=chat.turn } += 1.
    metrics.push(sum_metric(
        "trove.harness.events",
        "Count of harness events processed by Trove.",
        "1",
        &[sum_data_point_int(
            &start_ns,
            &now_ns,
            1,
            &[("event.kind", "chat.turn")],
        )],
    ));

    // tokens — per-model breakdown if Cowork provided one, otherwise
    // fall back to the top-level usage block (rare; assistant rows are
    // the usual carrier).
    let token_points = build_token_data_points(
        model_usage,
        row.get("usage").and_then(Value::as_object),
        &primary_model,
        &start_ns,
        &now_ns,
    );
    if !token_points.is_empty() {
        metrics.push(sum_metric(
            "trove.harness.tokens",
            "Tier A token count.",
            "1",
            &token_points,
        ));
    }

    // cost.usd — per-model costUSD if Cowork provided one; otherwise
    // emit total_cost_usd against the primary model.
    let cost_points = build_cost_data_points(
        model_usage,
        total_cost_usd,
        &primary_model,
        &start_ns,
        &now_ns,
    );
    if !cost_points.is_empty() {
        metrics.push(sum_metric(
            "trove.harness.cost.usd",
            "Tier A model spend (USD), exact figure from Cowork.",
            "USD",
            &cost_points,
        ));
    }

    // turn.duration — one observation per turn.
    metrics.push(histogram_metric_ms(
        "trove.harness.turn.duration",
        "Tier A chat-turn duration histogram (milliseconds).",
        duration_ms,
        &[("event.kind", "chat.turn"), ("model", primary_model.as_str())],
        &start_ns,
        &now_ns,
    ));

    if is_error {
        let error_kind = api_error_status
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown");
        metrics.push(sum_metric(
            "trove.harness.errors",
            "Tier A error count.",
            "1",
            &[sum_data_point_int(
                &start_ns,
                &now_ns,
                1,
                &[("error.kind", error_kind), ("model", primary_model.as_str())],
            )],
        ));
    }

    Some(wrap_metrics_payload(&metrics, opts))
}

/// Build the standard resource-attribute list for Claude Desktop's
/// synthesised OTLP payload, merging in any `opts.custom_attributes`
/// the caller provided.
fn build_resource_attrs(opts: &ApplyOptions) -> Vec<Value> {
    let mut resource_attrs = vec![
        json!({"key": "service.name", "value": {"stringValue": "claude-desktop"}}),
        json!({"key": "harness.id", "value": {"stringValue": "claude-desktop"}}),
        json!({"key": "harness.name", "value": {"stringValue": "Claude Desktop"}}),
        json!({"key": "trove.source", "value": {"stringValue": "claude-desktop"}}),
    ];
    for (k, v) in &opts.custom_attributes {
        resource_attrs.push(json!({"key": k, "value": {"stringValue": v}}));
    }
    resource_attrs
}

/// Wrap a list of metric values in the OTLP `resourceMetrics` envelope.
fn wrap_metrics_payload(metrics: &[Value], opts: &ApplyOptions) -> Value {
    json!({
        "resourceMetrics": [{
            "resource": {"attributes": build_resource_attrs(opts)},
            "scopeMetrics": [{
                "scope": {
                    "name": "trove.adapters.claude_desktop",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "metrics": metrics,
            }]
        }]
    })
}

/// Pick the model that consumed the most tokens this turn. Used for
/// the single-model attributes on `turn.duration` and `errors` (which
/// the connector schema declares as scalar `model`, not a list).
fn pick_primary_model(model_usage: Option<&serde_json::Map<String, Value>>) -> Option<String> {
    let mu = model_usage?;
    let mut best: Option<(String, u64)> = None;
    for (k, v) in mu {
        let input = v.get("inputTokens").and_then(Value::as_u64).unwrap_or(0);
        let output = v.get("outputTokens").and_then(Value::as_u64).unwrap_or(0);
        let cache_read = v
            .get("cacheReadInputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cache_create = v
            .get("cacheCreationInputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let total = input + output + cache_read + cache_create;
        match &best {
            Some((_, prev)) if *prev >= total => {}
            _ => best = Some((k.clone(), total)),
        }
    }
    best.map(|(k, _)| k)
}

fn build_token_data_points(
    model_usage: Option<&serde_json::Map<String, Value>>,
    top_level_usage: Option<&serde_json::Map<String, Value>>,
    primary_model: &str,
    start_ns: &str,
    now_ns: &str,
) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(mu) = model_usage {
        for (model, v) in mu {
            let input = v.get("inputTokens").and_then(Value::as_u64).unwrap_or(0);
            let output = v.get("outputTokens").and_then(Value::as_u64).unwrap_or(0);
            let cache_read = v
                .get("cacheReadInputTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let cache_create = v
                .get("cacheCreationInputTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let input_total = input + cache_read + cache_create;
            if input_total > 0 {
                out.push(sum_data_point_int(
                    start_ns,
                    now_ns,
                    input_total,
                    &[("direction", "input"), ("model", model.as_str())],
                ));
            }
            if output > 0 {
                out.push(sum_data_point_int(
                    start_ns,
                    now_ns,
                    output,
                    &[("direction", "output"), ("model", model.as_str())],
                ));
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    if let Some(u) = top_level_usage {
        let input = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
        let output = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
        let cache_read = u
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cache_create = u
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let input_total = input + cache_read + cache_create;
        if input_total > 0 {
            out.push(sum_data_point_int(
                start_ns,
                now_ns,
                input_total,
                &[("direction", "input"), ("model", primary_model)],
            ));
        }
        if output > 0 {
            out.push(sum_data_point_int(
                start_ns,
                now_ns,
                output,
                &[("direction", "output"), ("model", primary_model)],
            ));
        }
    }
    out
}

fn build_cost_data_points(
    model_usage: Option<&serde_json::Map<String, Value>>,
    total_cost_usd: f64,
    primary_model: &str,
    start_ns: &str,
    now_ns: &str,
) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(mu) = model_usage {
        for (model, v) in mu {
            let cost = v.get("costUSD").and_then(Value::as_f64).unwrap_or(0.0);
            if cost > 0.0 {
                out.push(sum_data_point_double(
                    start_ns,
                    now_ns,
                    cost,
                    &[("cost.method", "exact"), ("model", model.as_str())],
                ));
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    if total_cost_usd > 0.0 {
        out.push(sum_data_point_double(
            start_ns,
            now_ns,
            total_cost_usd,
            &[("cost.method", "exact"), ("model", primary_model)],
        ));
    }
    out
}

fn sum_metric(name: &str, desc: &str, unit: &str, data_points: &[Value]) -> Value {
    json!({
        "name": name,
        "unit": unit,
        "description": desc,
        "sum": {
            "aggregationTemporality": 1,
            "isMonotonic": true,
            "dataPoints": data_points,
        }
    })
}

fn sum_data_point_int(
    start_ns: &str,
    now_ns: &str,
    value: u64,
    attrs: &[(&str, &str)],
) -> Value {
    let attributes: Vec<Value> = attrs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| json!({"key": *k, "value": {"stringValue": *v}}))
        .collect();
    json!({
        "startTimeUnixNano": start_ns,
        "timeUnixNano": now_ns,
        "asInt": value.to_string(),
        "attributes": attributes,
    })
}

fn sum_data_point_double(
    start_ns: &str,
    now_ns: &str,
    value: f64,
    attrs: &[(&str, &str)],
) -> Value {
    let attributes: Vec<Value> = attrs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| json!({"key": *k, "value": {"stringValue": *v}}))
        .collect();
    json!({
        "startTimeUnixNano": start_ns,
        "timeUnixNano": now_ns,
        "asDouble": value,
        "attributes": attributes,
    })
}

fn histogram_metric_ms(
    name: &str,
    desc: &str,
    value_ms: f64,
    attrs: &[(&str, &str)],
    start_ns: &str,
    now_ns: &str,
) -> Value {
    let attributes: Vec<Value> = attrs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| json!({"key": *k, "value": {"stringValue": *v}}))
        .collect();
    // Single-bucket histogram covering one observation. Backends that
    // need bucket boundaries can derive them from the explicit min/max.
    json!({
        "name": name,
        "unit": "ms",
        "description": desc,
        "histogram": {
            "aggregationTemporality": 1,
            "dataPoints": [{
                "startTimeUnixNano": start_ns,
                "timeUnixNano": now_ns,
                "count": "1",
                "sum": value_ms,
                "min": value_ms,
                "max": value_ms,
                "bucketCounts": ["1"],
                "explicitBounds": [],
                "attributes": attributes,
            }],
        }
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    fn sample_result_row() -> Value {
        json!({
            "type": "result",
            "subtype": "success",
            "session_id": "abc",
            "duration_ms": 2593.0,
            "duration_api_ms": 3815.0,
            "num_turns": 1,
            "total_cost_usd": 0.094_126,
            "is_error": false,
            "stop_reason": "end_turn",
            "api_error_status": null,
            "modelUsage": {
                "claude-opus-4-7[1m]": {
                    "inputTokens": 6,
                    "outputTokens": 47,
                    "cacheReadInputTokens": 20942,
                    "cacheCreationInputTokens": 13124,
                    "costUSD": 0.093_701
                },
                "claude-haiku-4-5-20251001": {
                    "inputTokens": 355,
                    "outputTokens": 14,
                    "cacheReadInputTokens": 0,
                    "cacheCreationInputTokens": 0,
                    "costUSD": 0.000_425
                }
            }
        })
    }

    fn metric_by_name<'a>(payload: &'a Value, name: &str) -> Option<&'a Value> {
        payload["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()?
            .iter()
            .find(|m| m["name"].as_str() == Some(name))
    }

    fn attr_eq(attrs: &Value, key: &str, value: &str) -> bool {
        attrs.as_array().is_some_and(|a| {
            a.iter()
                .any(|x| x["key"] == key && x["value"]["stringValue"] == value)
        })
    }

    #[test]
    fn ignores_non_result_rows() {
        let user = json!({"type": "user", "message": {"role": "user", "content": "hi"}});
        let init = json!({"type": "system", "subtype": "init", "model": "claude-opus-4-7[1m]"});
        let rate = json!({"type": "rate_limit_event", "rate_limit_info": {}});
        let assistant = json!({"type": "assistant", "message": {"usage": {"input_tokens": 1}}});
        for row in [user, init, rate, assistant] {
            assert!(parse_result_metric_payload(&row, None, &ApplyOptions::default()).is_none());
        }
    }

    #[test]
    fn emits_one_event_per_result_with_chat_turn_kind() {
        let payload =
            parse_result_metric_payload(&sample_result_row(), None, &ApplyOptions::default())
                .unwrap();
        let events = metric_by_name(&payload, "trove.harness.events").unwrap();
        let dps = events["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(dps.len(), 1);
        assert_eq!(dps[0]["asInt"], "1");
        assert!(attr_eq(&dps[0]["attributes"], "event.kind", "chat.turn"));
    }

    #[test]
    fn tokens_metric_emits_input_and_output_per_model() {
        let payload =
            parse_result_metric_payload(&sample_result_row(), None, &ApplyOptions::default())
                .unwrap();
        let tokens = metric_by_name(&payload, "trove.harness.tokens").unwrap();
        let dps = tokens["sum"]["dataPoints"].as_array().unwrap();
        // Four points: input+output × two models.
        assert_eq!(dps.len(), 4);
        // Opus input = 6 + 20942 + 13124 = 34072.
        let opus_input = dps.iter().find(|dp| {
            attr_eq(&dp["attributes"], "direction", "input")
                && attr_eq(&dp["attributes"], "model", "claude-opus-4-7[1m]")
        });
        assert_eq!(opus_input.unwrap()["asInt"], "34072");
        // Haiku output = 14.
        let haiku_output = dps.iter().find(|dp| {
            attr_eq(&dp["attributes"], "direction", "output")
                && attr_eq(&dp["attributes"], "model", "claude-haiku-4-5-20251001")
        });
        assert_eq!(haiku_output.unwrap()["asInt"], "14");
    }

    #[test]
    fn cost_metric_emits_per_model_cost_usd_doubles() {
        let payload =
            parse_result_metric_payload(&sample_result_row(), None, &ApplyOptions::default())
                .unwrap();
        let cost = metric_by_name(&payload, "trove.harness.cost.usd").unwrap();
        let dps = cost["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(dps.len(), 2);
        let sum: f64 = dps.iter().map(|dp| dp["asDouble"].as_f64().unwrap()).sum();
        assert!((sum - 0.094_126).abs() < 1e-9, "got {sum}");
        for dp in dps {
            assert!(attr_eq(&dp["attributes"], "cost.method", "exact"));
        }
    }

    #[test]
    fn turn_duration_is_a_single_observation_histogram_in_ms() {
        let payload =
            parse_result_metric_payload(&sample_result_row(), None, &ApplyOptions::default())
                .unwrap();
        let dur = metric_by_name(&payload, "trove.harness.turn.duration").unwrap();
        assert_eq!(dur["unit"], "ms");
        let dp = &dur["histogram"]["dataPoints"][0];
        assert_eq!(dp["count"], "1");
        assert_eq!(dp["sum"], 2593.0);
        // Primary model = opus (more tokens).
        assert!(attr_eq(&dp["attributes"], "model", "claude-opus-4-7[1m]"));
        assert!(attr_eq(&dp["attributes"], "event.kind", "chat.turn"));
    }

    #[test]
    fn errors_metric_emits_only_when_is_error_true() {
        // is_error=false → no errors metric.
        let payload =
            parse_result_metric_payload(&sample_result_row(), None, &ApplyOptions::default())
                .unwrap();
        assert!(metric_by_name(&payload, "trove.harness.errors").is_none());

        // is_error=true → one error data point with the api_error_status kind.
        let mut errored = sample_result_row();
        errored["is_error"] = json!(true);
        errored["api_error_status"] = json!("rate_limited");
        let payload =
            parse_result_metric_payload(&errored, None, &ApplyOptions::default()).unwrap();
        let errors = metric_by_name(&payload, "trove.harness.errors").unwrap();
        let dps = errors["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(dps.len(), 1);
        assert!(attr_eq(&dps[0]["attributes"], "error.kind", "rate_limited"));
    }

    #[test]
    fn errors_metric_falls_back_to_unknown_when_api_error_status_missing() {
        let mut errored = sample_result_row();
        errored["is_error"] = json!(true);
        errored["api_error_status"] = json!(null);
        let payload =
            parse_result_metric_payload(&errored, None, &ApplyOptions::default()).unwrap();
        let errors = metric_by_name(&payload, "trove.harness.errors").unwrap();
        let dp = &errors["sum"]["dataPoints"][0];
        assert!(attr_eq(&dp["attributes"], "error.kind", "unknown"));
    }

    #[test]
    fn missing_model_usage_falls_back_to_top_level_usage_and_fallback_model() {
        let row = json!({
            "type": "result",
            "subtype": "success",
            "duration_ms": 100.0,
            "total_cost_usd": 0.01,
            "is_error": false,
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20,
                "cache_read_input_tokens": 5,
                "cache_creation_input_tokens": 3
            }
        });
        let payload = parse_result_metric_payload(
            &row,
            Some("claude-opus-4-7[1m]"),
            &ApplyOptions::default(),
        )
        .unwrap();
        let tokens = metric_by_name(&payload, "trove.harness.tokens").unwrap();
        let dps = tokens["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(dps.len(), 2);
        let input = dps
            .iter()
            .find(|dp| attr_eq(&dp["attributes"], "direction", "input"))
            .unwrap();
        // 10 + 5 + 3.
        assert_eq!(input["asInt"], "18");
        assert!(attr_eq(&input["attributes"], "model", "claude-opus-4-7[1m]"));
        // No modelUsage → cost falls back to total_cost_usd against the fallback model.
        let cost = metric_by_name(&payload, "trove.harness.cost.usd").unwrap();
        let cost_dps = cost["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(cost_dps.len(), 1);
        assert_eq!(cost_dps[0]["asDouble"], 0.01);
        assert!(attr_eq(&cost_dps[0]["attributes"], "model", "claude-opus-4-7[1m]"));
    }

    #[test]
    fn resource_attributes_include_harness_id_and_service_name() {
        let payload =
            parse_result_metric_payload(&sample_result_row(), None, &ApplyOptions::default())
                .unwrap();
        let attrs = &payload["resourceMetrics"][0]["resource"]["attributes"];
        assert!(attr_eq(attrs, "harness.id", "claude-desktop"));
        assert!(attr_eq(attrs, "service.name", "claude-desktop"));
        assert!(attr_eq(attrs, "harness.name", "Claude Desktop"));
        assert!(attr_eq(attrs, "trove.source", "claude-desktop"));
    }

    #[test]
    fn custom_attributes_propagate_onto_resource() {
        let mut opts = ApplyOptions::default();
        opts.custom_attributes
            .insert("team".into(), "platform".into());
        let payload = parse_result_metric_payload(&sample_result_row(), None, &opts).unwrap();
        let attrs = &payload["resourceMetrics"][0]["resource"]["attributes"];
        assert!(attr_eq(attrs, "team", "platform"));
    }

    // --- end-to-end watcher tests ---

    fn write_audit(root: &Path, account: &str, workspace: &str, session: &str, rows: &[Value]) -> PathBuf {
        let dir = root.join(account).join(workspace).join(session);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audit.jsonl");
        let mut body = String::new();
        for r in rows {
            body.push_str(&serde_json::to_string(r).unwrap());
            body.push('\n');
        }
        std::fs::write(&path, body).unwrap();
        path
    }

    fn append_audit(path: &Path, rows: &[Value]) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        for r in rows {
            writeln!(f, "{}", serde_json::to_string(r).unwrap()).unwrap();
        }
    }

    #[tokio::test]
    async fn watcher_skips_historical_content_on_first_observation_then_emits_for_appends() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // Pre-existing session with a historical result row — we must
        // NOT emit for this on first scan.
        let session_path = write_audit(
            &root,
            "acct-1",
            "ws-1",
            "local_session-1",
            &[
                json!({"type": "system", "subtype": "init", "model": "claude-opus-4-7[1m]"}),
                sample_result_row(),
            ],
        );

        let metrics: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = metrics.clone();
        let emit = move |p: Value| {
            let c = captured.clone();
            async move {
                c.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };

        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            run(
                root_clone,
                ApplyOptions::default(),
                Duration::from_millis(30),
                emit,
            )
            .await;
        });

        // Let it tick once to register the watermark at EOF.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            metrics.lock().await.len(),
            0,
            "historical results must not be emitted on first observation"
        );

        // Now append a fresh result — this one MUST be emitted.
        append_audit(&session_path, &[sample_result_row()]);
        wait_for_count(&metrics, 1, Duration::from_secs(2)).await;
        handle.abort();
        let _ = handle.await;

        let payloads = metrics.lock().await.clone();
        assert_eq!(payloads.len(), 1);
        let events = metric_by_name(&payloads[0], "trove.harness.events").unwrap();
        assert_eq!(events["sum"]["dataPoints"][0]["asInt"], "1");
    }

    #[tokio::test]
    async fn watcher_tolerates_missing_sessions_root() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope");
        let emit = |_p: Value| async { Ok::<_, otlp_emit::OtlpEmitError>(()) };
        let handle = tokio::spawn(async move {
            run(missing, ApplyOptions::default(), Duration::from_millis(30), emit).await;
        });
        tokio::time::sleep(Duration::from_millis(150)).await;
        handle.abort();
    }

    #[tokio::test]
    async fn watcher_handles_partial_line_at_eof_until_newline_arrives() {
        use std::io::Write;
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // Seed the session with one historical result so the first scan
        // skips to EOF; then write a partial JSON line (no trailing
        // newline). The watcher must hold onto the offset and not parse
        // until a newline arrives.
        let session_path = write_audit(
            &root,
            "acct",
            "ws",
            "local_s",
            &[json!({"type": "system", "subtype": "init", "model": "x"})],
        );

        let metrics: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = metrics.clone();
        let emit = move |p: Value| {
            let c = captured.clone();
            async move {
                c.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };
        let root_clone = root.clone();
        let handle = tokio::spawn(async move {
            run(root_clone, ApplyOptions::default(), Duration::from_millis(30), emit).await;
        });
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Append a partial line.
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&session_path).unwrap();
            let partial = serde_json::to_string(&sample_result_row()).unwrap();
            // Write only the first half — no newline yet.
            let half = &partial[..partial.len() / 2];
            f.write_all(half.as_bytes()).unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(metrics.lock().await.len(), 0, "partial line must not emit");

        // Now finish the line.
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&session_path).unwrap();
            let partial = serde_json::to_string(&sample_result_row()).unwrap();
            let second_half = &partial[partial.len() / 2..];
            f.write_all(second_half.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
        }
        wait_for_count(&metrics, 1, Duration::from_secs(2)).await;
        handle.abort();
        let _ = handle.await;
    }

    async fn wait_for_count(
        captured: &Arc<Mutex<Vec<Value>>>,
        target: usize,
        budget: Duration,
    ) {
        let deadline = std::time::Instant::now() + budget;
        loop {
            if captured.lock().await.len() >= target {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {target} captured payloads (have {})",
                captured.lock().await.len()
            );
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}
