//! Droid (factory.ai CLI) supplemental log watcher — tails
//! `~/.factory/logs/droid-log-single.log` for token and cost metrics.
//!
//! **Why a watcher in addition to Droid's native OTLP**: Droid's SDK
//! pushes tool / event / timing metrics via OTLP (Tier 1), but the SDK
//! does not emit any token or cost data over that path. The factory.ai
//! log file provides two line types that together cover all token directions.
//!
//! **Scope**: Two log-line types are parsed:
//!
//! - `[Agent] Streaming result | Context:` — per-call record with
//!   `contextCount` (full-price input), `outputTokens`, `cacheReadInputTokens`,
//!   and `tags.modelId`. Emitted in real-time after each API call.
//!
//! - `[Session] Saving session settings | Context:` — cumulative session
//!   totals including `thinkingTokens` and `cacheCreationTokens` (which do
//!   not appear on the per-call line). Because the values are cumulative,
//!   the watcher tracks the last-known totals per `sessionId` and emits
//!   only the delta. Confirmed format (live factory.ai CLI 0.57.2+):
//!   ```text
//!   [timestamp] INFO: [Session] Saving session settings | Context:
//!     {"value":{"sessionId":"…","hasTokenUsage":true,"tokenUsage":{
//!       "inputTokens":…,"outputTokens":…,"cacheCreationTokens":…,
//!       "cacheReadTokens":…,"thinkingTokens":…}},
//!      "tags":{…,"modelId":"claude-opus-4-6",…}}
//!   ```
//!
//! **Streaming result line format** (confirmed live CLI 0.133.0):
//! ```text
//! [2026-05-26T19:20:25.923Z] INFO: [Agent] Streaming result | Context: {"count":1,"cacheReadInputTokens":211512,"contextCount":310,"outputTokens":167,...,"tags":{"modelId":"claude-opus-4-6",...}}
//! ```
//! The JSON payload follows the literal `" | Context: "` separator.
//!
//! **Watermarking**: single rolling file; byte-offset + SHA-256 head-hash
//! (first 256 bytes) guard against truncation and rotation. First open
//! skips to EOF so a Trove restart does not re-emit historical turns.
//!
//! **Cost**: `contextCount` (full-price input) + `outputTokens` + `cacheReadInputTokens`
//! (billed at 0.1× input rate; Anthropic cache-read pricing confirmed May 2026) are
//! summed per API call and passed to `estimate_cost_usd`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::adapters::ApplyOptions;
use crate::log_watcher::WatcherHandle;
use crate::mappings::cost::estimate_cost_usd;
use crate::otlp_emit;

/// How often the watcher polls the log file.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Canonical path to the Droid rolling log. `home` is passed in so
/// tests can redirect to a temporary directory.
#[must_use]
pub fn log_path(home: &Path) -> PathBuf {
    home.join(".factory").join("logs").join("droid-log-single.log")
}

/// Byte-offset + SHA-256 head-hash watermark for the single rolling
/// log file.
#[derive(Debug, Default)]
struct FileWatermark {
    /// Next byte to read on the next tick.
    offset: u64,
    /// SHA-256 hex of the first 256 bytes of the file. Used to detect
    /// rotation (path reused with different content).
    head_hash: String,
    /// `true` until the first successful scan, which seeks to EOF and
    /// records the watermark without emitting historical turns.
    first_open: bool,
}

impl FileWatermark {
    fn fresh() -> Self {
        Self {
            first_open: true,
            ..Default::default()
        }
    }
}

/// Per-call token record deserialized from the JSON tail of a
/// `[Agent] Streaming result | Context:` log line.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamingResult {
    cache_read_input_tokens: u64,
    /// Full-price new input tokens (not the total context window size).
    context_count: u64,
    output_tokens: u64,
    tags: ResultTags,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResultTags {
    model_id: String,
    // `session_id` is present in the log but unused: watermarking at
    // the byte level already prevents re-emission on restart.
}

/// Per-session cumulative token record deserialized from the JSON tail of a
/// `[Session] Saving session settings | Context:` log line.
///
/// Values are cumulative session totals; callers must compute the delta
/// against the previous snapshot to get per-turn increments.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSettingsContext {
    value: SessionSettingsValue,
    tags: SessionSettingsTags,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSettingsValue {
    session_id: String,
    has_token_usage: bool,
    #[serde(default)]
    token_usage: Option<SessionTokenUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionTokenUsage {
    thinking_tokens: u64,
    cache_creation_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSettingsTags {
    #[serde(default)]
    model_id: Option<String>,
}

/// Delta record produced by `parse_session_delta` — the per-turn increment
/// of thinking and cache-creation tokens for one session save.
#[derive(Debug)]
struct SessionDeltaRecord {
    thinking_delta: u64,
    cache_creation_delta: u64,
    model_id: String,
}

/// Per-session state tracking last-seen cumulative totals for the fields
/// sourced from `[Session] Saving session settings` lines.
/// Key: `sessionId` string.  Value: `(last_thinking, last_cache_creation)`.
type SessionDeltaMap = std::collections::HashMap<String, (u64, u64)>;

/// Spawn the watcher on `log_path`. Errors during a poll are logged at
/// `warn!` and the loop continues — best-effort, consistent with the
/// other supplemental watchers.
#[must_use]
pub fn spawn(
    log_path: impl Into<PathBuf>,
    opts: ApplyOptions,
    mappings: crate::mappings::SharedMappingState,
    poll_interval: Duration,
) -> WatcherHandle {
    let path = log_path.into();
    let join = tokio::spawn(async move {
        run(
            path,
            opts,
            mappings,
            poll_interval,
            |payload: Value| async move { otlp_emit::post_metrics_json(&payload).await },
        )
        .await;
    });
    WatcherHandle::from_join(join)
}

/// Test-friendly entry point. Takes an explicit metric emitter so unit
/// tests can capture payloads instead of POST-ing to the collector.
pub async fn run<FMetric, FutMetric>(
    log_path: PathBuf,
    opts: ApplyOptions,
    mappings: crate::mappings::SharedMappingState,
    poll_interval: Duration,
    emit_metric: FMetric,
) where
    FMetric: Fn(Value) -> FutMetric + Send + Sync + 'static,
    FutMetric: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>> + Send,
{
    let mut watermark = FileWatermark::fresh();
    let mut session_state: SessionDeltaMap = SessionDeltaMap::new();
    loop {
        let mapping_snapshot = mappings.current();
        if let Err(e) = scan_once(
            &log_path,
            &opts,
            mapping_snapshot,
            &mut watermark,
            &mut session_state,
            &emit_metric,
        )
        .await
        {
            tracing::warn!(error = %e, ?log_path, "droid watcher tick errored");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

async fn scan_once<FMetric, FutMetric>(
    log_path: &Path,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
    watermark: &mut FileWatermark,
    session_state: &mut SessionDeltaMap,
    emit_metric: &FMetric,
) -> std::io::Result<()>
where
    FMetric: Fn(Value) -> FutMetric,
    FutMetric: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
{
    use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

    let metadata = match tokio::fs::metadata(log_path).await {
        Ok(m) => m,
        // Log file may not exist yet if Droid has never been run.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let file_size = metadata.len();

    // No new bytes since last tick — avoid the open() syscall.
    if !watermark.first_open && watermark.offset == file_size {
        return Ok(());
    }

    let mut file = tokio::fs::File::open(log_path).await?;

    // Read first 256 bytes to compute a rotation-detection hash.
    let mut head_buf = [0u8; 256];
    let head_n = file.read(&mut head_buf).await?;
    let head_hash = sha256_hex(&head_buf[..head_n]);

    let rotated = !watermark.head_hash.is_empty() && watermark.head_hash != head_hash;
    let truncated = watermark.offset > file_size;
    let mut start_offset = watermark.offset;

    if rotated || truncated {
        start_offset = 0;
        // File was replaced or truncated: cumulative session totals no longer
        // correspond to what we last saw, so the deltas would be wrong.
        session_state.clear();
    } else if watermark.first_open {
        // First open: record EOF position without reading historical
        // content so a Trove restart does not double-count past turns.
        watermark.head_hash = head_hash;
        watermark.offset = file_size;
        watermark.first_open = false;
        return Ok(());
    }

    if start_offset >= file_size {
        watermark.head_hash = head_hash;
        watermark.offset = file_size;
        watermark.first_open = false;
        return Ok(());
    }

    file.seek(SeekFrom::Start(start_offset)).await?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await?;

    // Only parse complete (newline-terminated) lines. A partial trailing
    // line is skipped — next tick picks it up once the newline arrives.
    let Some(last_nl_idx) = buf.iter().rposition(|&b| b == b'\n') else {
        watermark.head_hash = head_hash;
        watermark.first_open = false;
        return Ok(());
    };
    let last_newline = last_nl_idx + 1;
    let complete = &buf[..last_newline];

    let mut records: Vec<StreamingResult> = Vec::new();
    let mut session_deltas: Vec<SessionDeltaRecord> = Vec::new();
    for line in complete.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(s) = std::str::from_utf8(line) else {
            continue;
        };
        if let Some(r) = parse_streaming_result(s) {
            records.push(r);
        } else if let Some(d) = parse_session_delta(s, session_state) {
            session_deltas.push(d);
        }
    }

    if let Some(payload) = build_metrics_payload(&records, &session_deltas, opts, mappings) {
        if let Err(e) = emit_metric(payload).await {
            tracing::warn!(error = %e, "droid watcher: OTLP emit failed");
        }
    }

    watermark.head_hash = head_hash;
    watermark.offset = start_offset + last_newline as u64;
    watermark.first_open = false;
    Ok(())
}

/// Parse a `[Agent] Streaming result | Context: {…}` log line.
/// Returns `None` for all other line types or on JSON deserialisation failure.
fn parse_streaming_result(line: &str) -> Option<StreamingResult> {
    const MARKER: &str = "[Agent] Streaming result | Context: ";
    let json_tail = line.split(MARKER).nth(1)?;
    serde_json::from_str(json_tail).ok()
}

/// Parse a `[Session] Saving session settings | Context: {…}` log line and
/// return the per-turn **delta** of thinking and cache-creation tokens.
///
/// The log carries cumulative session totals; `session_state` is updated with
/// the latest totals so the next call can compute the next delta. Returns
/// `None` when the line does not match, has no token usage, or the delta is
/// zero for both thinking and cache-creation tokens.
fn parse_session_delta(
    line: &str,
    session_state: &mut SessionDeltaMap,
) -> Option<SessionDeltaRecord> {
    const MARKER: &str = "[Session] Saving session settings | Context: ";
    let json_tail = line.split(MARKER).nth(1)?;
    let ctx: SessionSettingsContext = serde_json::from_str(json_tail).ok()?;

    if !ctx.value.has_token_usage {
        return None;
    }
    let usage = ctx.value.token_usage.as_ref()?;
    let model_id = ctx.tags.model_id.unwrap_or_default();

    let entry = session_state.entry(ctx.value.session_id).or_insert((0, 0));
    let (last_thinking, last_cache_creation) = *entry;

    let thinking_delta = usage.thinking_tokens.saturating_sub(last_thinking);
    let cache_creation_delta = usage.cache_creation_tokens.saturating_sub(last_cache_creation);

    // Always advance the stored watermark so future saves compute the right delta.
    *entry = (usage.thinking_tokens, usage.cache_creation_tokens);

    if thinking_delta == 0 && cache_creation_delta == 0 {
        return None;
    }

    Some(SessionDeltaRecord { thinking_delta, cache_creation_delta, model_id })
}

/// Build an OTLP `resourceMetrics` payload from a batch of per-call streaming
/// results and session-level deltas. Returns `None` when both slices are empty
/// (or all-zero), or no active `HookRule`s are found.
fn build_metrics_payload(
    records: &[StreamingResult],
    session_deltas: &[SessionDeltaRecord],
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
) -> Option<Value> {
    let mut acc = crate::mappings::runtime::MetricsAccumulator::new(
        mappings,
        crate::harness::HarnessId::Droid,
    );

    for r in records {
        let extras = BTreeMap::from([("model".to_string(), r.tags.model_id.clone())]);

        // Emit each non-zero token direction under a distinct `when` key
        // so users can independently toggle / retarget them in the
        // Mappings editor. The HookRule's `inject_attributes` sets the
        // `direction` attribute; `extras` only carries `model`.
        #[allow(clippy::cast_possible_wrap)]
        if r.context_count > 0 {
            acc.observe_count("droid.tokens.input", r.context_count as i64, &extras);
        }
        #[allow(clippy::cast_possible_wrap)]
        if r.output_tokens > 0 {
            acc.observe_count("droid.tokens.output", r.output_tokens as i64, &extras);
        }
        #[allow(clippy::cast_possible_wrap)]
        if r.cache_read_input_tokens > 0 {
            acc.observe_count(
                "droid.tokens.cache_read",
                r.cache_read_input_tokens as i64,
                &extras,
            );
        }

        // Cost: full-price input + output + cache-read (0.1× input rate).
        // Guard against zero-token records emitting a spurious 0.0 data point.
        if r.context_count > 0 || r.output_tokens > 0 || r.cache_read_input_tokens > 0 {
            if let Some(usd) = estimate_cost_usd(
                &r.tags.model_id,
                r.context_count,
                r.output_tokens,
                r.cache_read_input_tokens,
                &BTreeMap::new(),
            ) {
                if usd > 0.0 {
                    acc.observe_double_sum("droid.cost", usd, &extras);
                }
            }
        }
    }

    for d in session_deltas {
        let extras = BTreeMap::from([("model".to_string(), d.model_id.clone())]);

        #[allow(clippy::cast_possible_wrap)]
        if d.thinking_delta > 0 {
            acc.observe_count("droid.tokens.thinking", d.thinking_delta as i64, &extras);
        }
        #[allow(clippy::cast_possible_wrap)]
        if d.cache_creation_delta > 0 {
            acc.observe_count(
                "droid.tokens.cache_write",
                d.cache_creation_delta as i64,
                &extras,
            );
        }
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
        json!({"key": "service.name", "value": {"stringValue": "droid"}}),
        json!({"key": "harness.id",   "value": {"stringValue": "droid"}}),
        json!({"key": "harness.name", "value": {"stringValue": "Droid"}}),
        json!({"key": "trove.source", "value": {"stringValue": "droid"}}),
    ];
    for (k, v) in &opts.custom_attributes {
        resource_attrs.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    Some(json!({
        "resourceMetrics": [{
            "resource": {"attributes": resource_attrs},
            "scopeMetrics": [{
                "scope": {
                    "name": "trove.adapters.droid_watcher",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "metrics": metrics,
            }]
        }]
    }))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::sync::Arc;

    use tempfile::NamedTempFile;
    use tokio::sync::Mutex;

    use super::*;

    fn rules_arc() -> Arc<crate::mappings::MappingState> {
        Arc::new(crate::mappings::default_state())
    }

    fn rules_sub() -> crate::mappings::SharedMappingState {
        crate::mappings::MappingStateStore::new(crate::mappings::default_state()).subscribe()
    }

    /// Build a realistic `[Agent] Streaming result | Context: {…}` log line.
    fn streaming_line(cache_read: u64, context: u64, output: u64, model: &str) -> String {
        format!(
            "[2026-05-26T19:20:25.923Z] INFO: [Agent] Streaming result | Context: \
             {{\"count\":1,\"cacheReadInputTokens\":{cache_read},\
             \"contextCount\":{context},\"outputTokens\":{output},\
             \"reason\":\"tool-calls\",\"hasReasoningContent\":false,\
             \"tags\":{{\"clientType\":\"cli\",\"modelId\":\"{model}\",\
             \"sessionId\":\"d4d06660-abc123\"}}}}"
        )
    }

    fn attr_eq(attrs: &Value, key: &str, value: &str) -> bool {
        attrs.as_array().is_some_and(|arr| {
            arr.iter().any(|a| {
                a.get("key").and_then(Value::as_str) == Some(key)
                    && a["value"]["stringValue"].as_str() == Some(value)
            })
        })
    }

    fn metric_by_name<'a>(payload: &'a Value, name: &str) -> Option<&'a Value> {
        payload["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()?
            .iter()
            .find(|m| m["name"].as_str() == Some(name))
    }

    fn direction_of(dp: &Value) -> Option<&str> {
        dp["attributes"].as_array()?.iter().find_map(|a| {
            if a["key"].as_str() == Some("direction") {
                a["value"]["stringValue"].as_str()
            } else {
                None
            }
        })
    }

    // ─── parse_streaming_result ───────────────────────────────────────────

    #[test]
    fn ignores_non_streaming_result_lines() {
        let non_matching = [
            "[2026-05-26T12:00:00Z] INFO: [ResourceMonitor] Resource usage snapshot | Context: {}",
            "[2026-05-26T12:00:00Z] INFO: [Session] Saving session settings | Context: {}",
            "totally unrelated log line",
            "",
        ];
        for line in non_matching {
            assert!(
                parse_streaming_result(line).is_none(),
                "expected None for: {line:?}"
            );
        }
    }

    #[test]
    fn parses_valid_streaming_result() {
        let line = streaming_line(211_512, 310, 167, "claude-opus-4-6");
        let r = parse_streaming_result(&line).unwrap();
        assert_eq!(r.cache_read_input_tokens, 211_512);
        assert_eq!(r.context_count, 310);
        assert_eq!(r.output_tokens, 167);
        assert_eq!(r.tags.model_id, "claude-opus-4-6");
    }

    #[test]
    fn ignores_malformed_json_after_context_marker() {
        let bad = "[2026-05-26T12:00:00Z] INFO: [Agent] Streaming result | Context: {truncated";
        assert!(
            parse_streaming_result(bad).is_none(),
            "truncated JSON must return None, not panic"
        );
    }

    // ─── build_metrics_payload ────────────────────────────────────────────

    #[test]
    fn build_payload_emits_three_token_directions() {
        let records = vec![StreamingResult {
            cache_read_input_tokens: 100,
            context_count: 50,
            output_tokens: 25,
            tags: ResultTags { model_id: "claude-opus-4-6".into() },
        }];
        let payload =
            build_metrics_payload(&records, &[], &ApplyOptions::default(), rules_arc()).unwrap();
        let tokens = metric_by_name(&payload, "trove.harness.tokens").unwrap();
        let dps = tokens["sum"]["dataPoints"].as_array().unwrap();
        let directions: Vec<&str> = dps.iter().filter_map(|dp| direction_of(dp)).collect();
        assert!(directions.contains(&"input"), "missing direction=input");
        assert!(directions.contains(&"output"), "missing direction=output");
        assert!(directions.contains(&"cacheRead"), "missing direction=cacheRead");
    }

    #[test]
    fn build_payload_omits_zero_token_buckets() {
        let records = vec![StreamingResult {
            cache_read_input_tokens: 0, // zero — must not produce a data point
            context_count: 50,
            output_tokens: 25,
            tags: ResultTags { model_id: "claude-opus-4-6".into() },
        }];
        let payload =
            build_metrics_payload(&records, &[], &ApplyOptions::default(), rules_arc()).unwrap();
        let tokens = metric_by_name(&payload, "trove.harness.tokens").unwrap();
        let dps = tokens["sum"]["dataPoints"].as_array().unwrap();
        assert!(
            !dps.iter().any(|dp| direction_of(dp) == Some("cacheRead")),
            "zero cache_read_input_tokens must not emit a cacheRead data point"
        );
    }

    #[test]
    fn build_payload_emits_cost_for_known_model() {
        let records = vec![StreamingResult {
            cache_read_input_tokens: 0,
            context_count: 1_000,
            output_tokens: 100,
            tags: ResultTags { model_id: "claude-opus-4-6".into() },
        }];
        let payload =
            build_metrics_payload(&records, &[], &ApplyOptions::default(), rules_arc()).unwrap();
        let cost = metric_by_name(&payload, "trove.harness.cost.usd")
            .expect("known model must emit trove.harness.cost.usd");
        let dp = &cost["sum"]["dataPoints"][0];
        assert!(
            attr_eq(&dp["attributes"], "cost.method", "estimated"),
            "cost.method must be 'estimated'"
        );
    }

    #[test]
    fn build_payload_skips_cost_for_unknown_model() {
        let records = vec![StreamingResult {
            cache_read_input_tokens: 0,
            context_count: 1_000,
            output_tokens: 100,
            tags: ResultTags { model_id: "unknown-future-model-xyz".into() },
        }];
        let payload =
            build_metrics_payload(&records, &[], &ApplyOptions::default(), rules_arc()).unwrap();
        // Token points still present; cost must be absent.
        assert!(metric_by_name(&payload, "trove.harness.tokens").is_some());
        assert!(
            metric_by_name(&payload, "trove.harness.cost.usd").is_none(),
            "unknown model must not emit a cost data point"
        );
    }

    #[test]
    fn build_payload_returns_none_when_all_tokens_zero() {
        let records = vec![StreamingResult {
            cache_read_input_tokens: 0,
            context_count: 0,
            output_tokens: 0,
            tags: ResultTags { model_id: "claude-opus-4-6".into() },
        }];
        assert!(
            build_metrics_payload(&records, &[], &ApplyOptions::default(), rules_arc()).is_none(),
            "all-zero record must return None"
        );
    }

    #[test]
    fn resource_attrs_carry_harness_id_and_custom_attrs() {
        let mut opts = ApplyOptions::default();
        opts.custom_attributes.insert("team".into(), "platform".into());
        let records = vec![StreamingResult {
            cache_read_input_tokens: 100,
            context_count: 50,
            output_tokens: 25,
            tags: ResultTags { model_id: "claude-opus-4-6".into() },
        }];
        let payload = build_metrics_payload(&records, &[], &opts, rules_arc()).unwrap();
        let attrs = &payload["resourceMetrics"][0]["resource"]["attributes"];
        assert!(attr_eq(attrs, "harness.id", "droid"));
        assert!(attr_eq(attrs, "service.name", "droid"));
        assert!(attr_eq(attrs, "harness.name", "Droid"));
        assert!(attr_eq(attrs, "trove.source", "droid"));
        assert!(attr_eq(attrs, "team", "platform"));
    }

    // ─── parse_session_delta ─────────────────────────────────────────────

    fn session_line(
        session_id: &str,
        thinking: u64,
        cache_creation: u64,
        model: &str,
    ) -> String {
        format!(
            "[2026-05-26T20:00:00.000Z] INFO: [Session] Saving session settings | Context: \
             {{\"value\":{{\"sessionId\":\"{session_id}\",\"path\":\"/tmp/x.json\",\
             \"hasTokenUsage\":true,\"tokenUsage\":{{\"inputTokens\":100,\"outputTokens\":50,\
             \"cacheCreationTokens\":{cache_creation},\"cacheReadTokens\":200,\
             \"thinkingTokens\":{thinking}}}}},\
             \"tags\":{{\"platform\":\"linux\",\"version\":\"0.57.2\",\
             \"modelId\":\"{model}\",\"isByok\":\"false\"}}}}"
        )
    }

    #[test]
    fn session_delta_returns_none_for_non_session_lines() {
        let mut state = SessionDeltaMap::new();
        let non_matching = [
            "[2026-05-26T12:00:00Z] INFO: [Agent] Streaming result | Context: {}",
            "totally unrelated log line",
            "",
        ];
        for line in non_matching {
            assert!(
                parse_session_delta(line, &mut state).is_none(),
                "expected None for: {line:?}"
            );
        }
    }

    #[test]
    fn session_delta_returns_none_when_no_token_usage() {
        let mut state = SessionDeltaMap::new();
        let no_usage = "[2026-05-26T20:00:00Z] INFO: [Session] Saving session settings | Context: \
             {\"value\":{\"sessionId\":\"s1\",\"path\":\"/tmp/x.json\",\"hasTokenUsage\":false},\
             \"tags\":{\"platform\":\"linux\"}}";
        assert!(parse_session_delta(no_usage, &mut state).is_none());
    }

    #[test]
    fn session_delta_emits_on_first_nonzero_save() {
        let mut state = SessionDeltaMap::new();
        let line = session_line("s1", 500, 1_000, "claude-opus-4-6");
        let d = parse_session_delta(&line, &mut state).unwrap();
        assert_eq!(d.thinking_delta, 500);
        assert_eq!(d.cache_creation_delta, 1_000);
        assert_eq!(d.model_id, "claude-opus-4-6");
    }

    #[test]
    fn session_delta_computes_incremental_delta() {
        let mut state = SessionDeltaMap::new();
        // Turn 1: thinking=100, cache_creation=500
        let d1 =
            parse_session_delta(&session_line("s2", 100, 500, "claude-sonnet-4"), &mut state)
                .unwrap();
        assert_eq!(d1.thinking_delta, 100);
        assert_eq!(d1.cache_creation_delta, 500);

        // Turn 2: cumulative now thinking=300, cache_creation=500 (no new cache creation)
        let d2 =
            parse_session_delta(&session_line("s2", 300, 500, "claude-sonnet-4"), &mut state)
                .unwrap();
        assert_eq!(d2.thinking_delta, 200, "should be the increment since turn 1");
        assert_eq!(d2.cache_creation_delta, 0, "cache_creation did not grow");

        // No delta — should return None, not emit a zero record.
        assert!(
            parse_session_delta(&session_line("s2", 300, 500, "claude-sonnet-4"), &mut state)
                .is_none(),
            "zero delta must return None"
        );
    }

    #[test]
    fn session_delta_tracks_multiple_sessions_independently() {
        let mut state = SessionDeltaMap::new();
        parse_session_delta(&session_line("sA", 100, 200, "claude-opus-4-6"), &mut state);
        parse_session_delta(&session_line("sB", 50, 0, "claude-sonnet-4"), &mut state);

        // Second save for sA — delta is incremental, not mixed with sB.
        let d = parse_session_delta(&session_line("sA", 150, 200, "claude-opus-4-6"), &mut state)
            .unwrap();
        assert_eq!(d.thinking_delta, 50);
        assert_eq!(d.cache_creation_delta, 0);
    }

    #[test]
    fn build_payload_emits_thinking_and_cache_write_from_session_deltas() {
        let deltas = vec![SessionDeltaRecord {
            thinking_delta: 400,
            cache_creation_delta: 2_000,
            model_id: "claude-opus-4-6".into(),
        }];
        let payload =
            build_metrics_payload(&[], &deltas, &ApplyOptions::default(), rules_arc()).unwrap();
        let tokens = metric_by_name(&payload, "trove.harness.tokens").unwrap();
        let dps = tokens["sum"]["dataPoints"].as_array().unwrap();
        let directions: Vec<&str> = dps.iter().filter_map(|dp| direction_of(dp)).collect();
        assert!(directions.contains(&"thinking"), "missing direction=thinking");
        assert!(directions.contains(&"cacheWrite"), "missing direction=cacheWrite");
    }

    #[test]
    fn build_payload_omits_zero_session_delta_buckets() {
        let deltas = vec![SessionDeltaRecord {
            thinking_delta: 0,
            cache_creation_delta: 500,
            model_id: "claude-opus-4-6".into(),
        }];
        let payload =
            build_metrics_payload(&[], &deltas, &ApplyOptions::default(), rules_arc()).unwrap();
        let tokens = metric_by_name(&payload, "trove.harness.tokens").unwrap();
        let dps = tokens["sum"]["dataPoints"].as_array().unwrap();
        assert!(
            !dps.iter().any(|dp| direction_of(dp) == Some("thinking")),
            "zero thinking_delta must not emit a thinking data point"
        );
    }

    #[test]
    fn build_payload_cache_read_tokens_contribute_to_cost() {
        // 1000 cache-read tokens @ opus-4 ($15/1k * 0.1) = $1.50
        let records = vec![StreamingResult {
            cache_read_input_tokens: 1_000,
            context_count: 0,
            output_tokens: 0,
            tags: ResultTags { model_id: "claude-opus-4-6".into() },
        }];
        let payload =
            build_metrics_payload(&records, &[], &ApplyOptions::default(), rules_arc()).unwrap();
        let cost = metric_by_name(&payload, "trove.harness.cost.usd")
            .expect("cache-read tokens must contribute to cost");
        let usd = cost["sum"]["dataPoints"][0]["asDouble"].as_f64().unwrap();
        assert!((usd - 1.5).abs() < 1e-9, "expected $1.50, got {usd}");
    }

    // ─── file I/O (async) ─────────────────────────────────────────────────

    #[tokio::test]
    async fn first_open_skips_to_eof_then_emits_appended_lines() {
        let mut tmp = NamedTempFile::new().unwrap();

        // Historical content — must NOT be emitted on first scan.
        writeln!(tmp, "{}", streaming_line(100, 50, 25, "claude-opus-4-6")).unwrap();

        let metrics: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = metrics.clone();
        let emit = move |p: Value| {
            let c = captured.clone();
            async move {
                c.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };

        let mut watermark = FileWatermark::fresh();
        let mut session_state = SessionDeltaMap::new();

        // First scan — should skip to EOF, emit nothing.
        scan_once(
            tmp.path(),
            &ApplyOptions::default(),
            rules_arc(),
            &mut watermark,
            &mut session_state,
            &emit,
        )
        .await
        .unwrap();
        assert_eq!(
            metrics.lock().await.len(),
            0,
            "historical line must not emit on first open"
        );
        assert!(!watermark.first_open, "first_open flag must clear after first scan");

        // Append a new line — second scan must emit it.
        writeln!(tmp, "{}", streaming_line(200, 60, 30, "claude-opus-4-6")).unwrap();

        scan_once(
            tmp.path(),
            &ApplyOptions::default(),
            rules_arc(),
            &mut watermark,
            &mut session_state,
            &emit,
        )
        .await
        .unwrap();
        assert_eq!(metrics.lock().await.len(), 1, "appended line must emit on next scan");
    }

    #[tokio::test]
    async fn rotation_resets_offset_and_reads_new_content() {
        let mut tmp = NamedTempFile::new().unwrap();

        // Write two lines and advance the watermark to EOF via first-open skip.
        writeln!(tmp, "{}", streaming_line(100, 50, 25, "claude-opus-4-6")).unwrap();
        writeln!(tmp, "{}", streaming_line(100, 50, 25, "claude-opus-4-6")).unwrap();

        let noop = |_: Value| async { Ok::<_, otlp_emit::OtlpEmitError>(()) };
        let mut watermark = FileWatermark::fresh();
        let mut session_state = SessionDeltaMap::new();
        scan_once(
            tmp.path(),
            &ApplyOptions::default(),
            rules_arc(),
            &mut watermark,
            &mut session_state,
            &noop,
        )
        .await
        .unwrap();
        let old_offset = watermark.offset; // = length of two-line file

        // Simulate rotation: replace with shorter content (new log file at same path).
        let short_line = streaming_line(10, 5, 2, "claude-haiku-4-5-20251001");
        std::fs::write(tmp.path(), format!("{short_line}\n")).unwrap();

        let metrics: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured = metrics.clone();
        let emit = move |p: Value| {
            let c = captured.clone();
            async move {
                c.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };

        scan_once(
            tmp.path(),
            &ApplyOptions::default(),
            rules_arc(),
            &mut watermark,
            &mut session_state,
            &emit,
        )
        .await
        .unwrap();

        // Rotation detected: new file is shorter than old_offset.
        assert!(
            watermark.offset < old_offset,
            "offset must reset after rotation (old={old_offset}, new={})",
            watermark.offset
        );
        assert_eq!(metrics.lock().await.len(), 1, "rotated file content must be emitted");
    }

    #[tokio::test]
    async fn tolerates_missing_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("droid-log-single.log");
        let mut watermark = FileWatermark::fresh();
        let mut session_state = SessionDeltaMap::new();
        let noop = |_: Value| async { Ok::<_, otlp_emit::OtlpEmitError>(()) };
        // Must return Ok(()) gracefully — not error — when file is absent.
        scan_once(
            &missing,
            &ApplyOptions::default(),
            rules_arc(),
            &mut watermark,
            &mut session_state,
            &noop,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn run_loop_starts_and_can_be_aborted() {
        // Smoke test: `spawn` (→ `run`) must start without panicking and
        // be abortable via the returned WatcherHandle. Uses a non-existent
        // path so no I/O happens.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("droid-log-single.log");
        let handle = spawn(path, ApplyOptions::default(), rules_sub(), Duration::from_millis(50));
        tokio::time::sleep(Duration::from_millis(120)).await;
        drop(handle); // dropping aborts the task
    }
}
