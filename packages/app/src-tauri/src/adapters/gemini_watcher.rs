//! Gemini CLI chat-log watcher — reads
//! `~/.gemini/tmp/<project>/chats/session-*.jsonl` and emits Tier A
//! metrics from the rich per-turn data Gemini writes locally.
//!
//! **Why a watcher in addition to the metricstransform synthesis from
//! Gemini's native OTLP emission**: Gemini's OTLP signal is incomplete
//! for what dashboards want. In particular, `gen_ai.client.token.usage`
//! flushes only at certain checkpoints and a short session may exit
//! before any token observation reaches the collector; `cost.usd`
//! needs a per-model rate × token-count multiplication that the
//! `OTel` `metricstransform` processor cannot express. Reading the chat
//! log lets us emit a complete Tier A bundle (`events`, `tokens`,
//! `cost.usd`, `turn.duration`) the moment Gemini finishes writing a
//! turn — independent of whether Gemini's own OTLP exporter ever
//! flushes the corresponding histogram observation.
//!
//! File format (gemini-cli ≥0.41, eyeballed from a live install): each
//! `.jsonl` is append-only with one JSON doc per line. The first line
//! is session metadata (`sessionId`, `projectHash`, `startTime`).
//! Subsequent lines alternate between turn records and ephemeral
//! `$set` updates:
//!
//! ```text
//! {"id":..., "timestamp":"…", "type":"user", "content":[{"text":...}]}
//! {"$set":{"lastUpdated":"…"}}
//! {"id":..., "timestamp":"…", "type":"gemini",
//!   "content":"…",
//!   "tokens":{"input":9956,"output":33,"cached":3749,"thoughts":134,"tool":0,"total":10123},
//!   "model":"gemini-3-flash-preview"}
//! {"$set":{"lastUpdated":"…"}}
//! ```
//!
//! Each `type: "gemini"` record carries everything Tier A needs:
//! tokens (already split by direction), model (for cost rate lookup),
//! and a timestamp (for duration calculation against the immediately
//! preceding `type: "user"` line).
//!
//! Watermark strategy: keep `(byte_offset, last_user_timestamp)` per
//! session file. Byte offset is robust because the file is append-only
//! — the only mutation Gemini makes is appending new lines. (The
//! `$set` records are appended too, not in-place edits.) The
//! last-user-timestamp is the pairing key for duration; when a
//! `type: "gemini"` record arrives, its duration is measured against
//! that timestamp and the field is cleared.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::DateTime;
use serde_json::{json, Value};

use crate::adapters::ApplyOptions;
use crate::log_watcher::WatcherHandle;
use crate::mappings::cost::estimate_cost_usd;
use crate::otlp_emit;

/// How often the watcher polls each chat-session file.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Per-session-file watermark. `bytes_read` is the count of bytes the
/// watcher has already consumed (so the next read starts there);
/// `pending_user_ts` carries the timestamp of the last `type: "user"`
/// record so the next `type: "gemini"` record can compute its duration.
#[derive(Debug, Default, Clone)]
struct SessionWatermark {
    bytes_read: u64,
    pending_user_ts: Option<DateTime<chrono::Utc>>,
}

/// Spawn the watcher rooted at `gemini_tmp_root` (typically
/// `~/.gemini/tmp`). Errors during emission log at `warn!` and the
/// loop continues — best-effort, like the Cline watcher.
#[must_use]
pub fn spawn(
    gemini_tmp_root: impl Into<PathBuf>,
    opts: ApplyOptions,
    mappings: crate::mappings::SharedMappingState,
    poll_interval: Duration,
) -> WatcherHandle {
    let root = gemini_tmp_root.into();
    let join = tokio::spawn(async move {
        run(
            root,
            opts,
            mappings,
            poll_interval,
            |payload: Value| async move { otlp_emit::post_metrics_json(&payload).await },
        )
        .await;
    });
    WatcherHandle::from_join(join)
}

/// Test-friendly variant: takes an explicit metric emitter.
pub async fn run<F, Fut>(
    gemini_tmp_root: PathBuf,
    opts: ApplyOptions,
    mappings: crate::mappings::SharedMappingState,
    poll_interval: Duration,
    emit_metric: F,
) where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>> + Send,
{
    let mut watermarks: HashMap<PathBuf, SessionWatermark> = HashMap::new();
    loop {
        let mapping_snapshot = mappings.current();
        if let Err(e) =
            scan_once(&gemini_tmp_root, &opts, mapping_snapshot, &mut watermarks, &emit_metric).await
        {
            tracing::warn!(error = %e, ?gemini_tmp_root, "gemini watcher tick errored");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// One pass: walk `~/.gemini/tmp/<project>/chats/*.jsonl`, read any
/// new bytes from each, parse the new lines, classify them, and emit
/// metrics for each `type: "gemini"` record. Updates watermarks even
/// when emission fails so a transient OTLP error doesn't make us
/// re-emit the same turn forever.
async fn scan_once<F, Fut>(
    gemini_tmp_root: &Path,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
    watermarks: &mut HashMap<PathBuf, SessionWatermark>,
    emit_metric: &F,
) -> std::io::Result<()>
where
    F: Fn(Value) -> Fut,
    Fut: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
{
    let mut projects = match tokio::fs::read_dir(gemini_tmp_root).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    while let Some(project_entry) = projects.next_entry().await? {
        let chats_dir = project_entry.path().join("chats");
        let Ok(mut sessions) = tokio::fs::read_dir(&chats_dir).await else {
            continue; // No chats/ subdir under this project.
        };
        while let Some(session_entry) = sessions.next_entry().await? {
            let session_path = session_entry.path();
            if !is_jsonl(&session_path) {
                continue;
            }
            if let Err(e) =
                process_session(&session_path, opts, mappings.clone(), watermarks, emit_metric).await
            {
                tracing::warn!(error = %e, ?session_path, "gemini watcher: session read errored");
            }
        }
    }
    Ok(())
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("jsonl")
}

async fn process_session<F, Fut>(
    session_path: &Path,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
    watermarks: &mut HashMap<PathBuf, SessionWatermark>,
    emit_metric: &F,
) -> std::io::Result<()>
where
    F: Fn(Value) -> Fut,
    Fut: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
{
    let bytes = tokio::fs::read(session_path).await?;
    let total_len = bytes.len() as u64;
    let mark = watermarks
        .entry(session_path.to_path_buf())
        .or_default()
        .clone();
    if mark.bytes_read >= total_len {
        return Ok(()); // No growth.
    }

    #[allow(clippy::cast_possible_truncation)]
    let tail = &bytes[mark.bytes_read as usize..];
    // Only consume up through the last full newline — a partial trailing
    // line may be the in-flight write of a turn record. Leave it for
    // the next tick.
    let last_newline = tail.iter().rposition(|b| *b == b'\n');
    let consume_end = match last_newline {
        Some(idx) => idx + 1,
        None => return Ok(()), // No complete line yet.
    };
    let consumable = &tail[..consume_end];

    let (emissions, new_pending_user_ts) =
        build_emissions(consumable, mark.pending_user_ts, opts, mappings);

    watermarks.insert(
        session_path.to_path_buf(),
        SessionWatermark {
            bytes_read: mark.bytes_read + consume_end as u64,
            pending_user_ts: new_pending_user_ts,
        },
    );

    if let Some(payload) = emissions {
        if let Err(e) = emit_metric(payload).await {
            tracing::warn!(error = %e, ?session_path, "gemini watcher metric emit failed");
        }
    }
    Ok(())
}

/// One turn observation extracted from a `type: "gemini"` record.
#[derive(Debug, Clone)]
struct GeminiTurn {
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    /// `Some(secs)` if we can compute duration against a preceding
    /// `type: "user"` timestamp. `None` for orphaned turns (e.g.
    /// the file's first emitted line is a gemini record because the
    /// watcher started mid-session and never saw the user message).
    duration_s: Option<f64>,
}

/// Parse the new tail, classify each line, and assemble an OTLP metric
/// payload covering every gemini turn it contains. Also returns the
/// trailing `pending_user_ts` so the next call can compute duration
/// for a gemini record whose paired user line lived in this tail but
/// whose response lands in the next.
fn build_emissions(
    tail: &[u8],
    mut pending_user_ts: Option<DateTime<chrono::Utc>>,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
) -> (Option<Value>, Option<DateTime<chrono::Utc>>) {
    let Ok(text) = std::str::from_utf8(tail) else {
        return (None, pending_user_ts);
    };

    let mut turns: Vec<GeminiTurn> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // Skip unparseable lines.
        };
        let kind = value.get("type").and_then(Value::as_str);
        match kind {
            Some("user") => {
                pending_user_ts = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc));
            }
            Some("gemini") => {
                if let Some(turn) = parse_gemini_record(&value, pending_user_ts) {
                    turns.push(turn);
                }
                pending_user_ts = None;
            }
            _ => {} // $set / metadata / other — ignore.
        }
    }
    if turns.is_empty() {
        return (None, pending_user_ts);
    }
    let payload = build_metrics_payload(&turns, opts, mappings);
    (payload, pending_user_ts)
}

fn parse_gemini_record(
    record: &Value,
    paired_user_ts: Option<DateTime<chrono::Utc>>,
) -> Option<GeminiTurn> {
    let timestamp = record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))?;
    let model = record
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let tokens = record.get("tokens").and_then(Value::as_object);
    let input_tokens = tokens
        .and_then(|t| t.get("input"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = tokens
        .and_then(|t| t.get("output"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    #[allow(clippy::cast_precision_loss)]
    let duration_s =
        paired_user_ts.map(|u| (timestamp - u).num_milliseconds() as f64 / 1000.0);
    Some(GeminiTurn {
        model,
        input_tokens,
        output_tokens,
        duration_s,
    })
}

#[allow(clippy::too_many_lines)]
fn build_metrics_payload(
    turns: &[GeminiTurn],
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
) -> Option<Value> {
    let mut acc = crate::mappings::runtime::MetricsAccumulator::new(
        mappings,
        crate::harness::HarnessId::GeminiCli,
    );

    for t in turns {
        let mut model_attr = std::collections::BTreeMap::new();
        model_attr.insert("model".to_string(), t.model.clone());

        // One chat.turn event per turn — `gemini.turn` rules with a
        // counter target fire here.
        acc.observe_count("gemini.turn", 1, &model_attr);

        // Tokens fan out by direction. Use `gemini.tokens.input` and
        // `gemini.tokens.output` as separate when keys so users see
        // them as distinct rules in the editor and can independently
        // toggle / retarget either direction.
        if t.input_tokens > 0 {
            #[allow(clippy::cast_possible_wrap)]
            acc.observe_count(
                "gemini.tokens.input",
                t.input_tokens as i64,
                &model_attr,
            );
        }
        if t.output_tokens > 0 {
            #[allow(clippy::cast_possible_wrap)]
            acc.observe_count(
                "gemini.tokens.output",
                t.output_tokens as i64,
                &model_attr,
            );
        }

        if let Some(usd) = estimate_cost_usd(
            &t.model,
            t.input_tokens,
            t.output_tokens,
            &std::collections::BTreeMap::new(),
        ) {
            acc.observe_double_sum("gemini.cost", usd, &model_attr);
        }

        if let Some(d) = t.duration_s {
            if d >= 0.0 {
                acc.observe_histogram("gemini.turn", d, &model_attr);
            }
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
        json!({"key": "service.name", "value": {"stringValue": "gemini-cli"}}),
        json!({"key": "harness.id", "value": {"stringValue": "gemini-cli"}}),
        json!({"key": "harness.name", "value": {"stringValue": "Gemini CLI"}}),
        json!({"key": "trove.source", "value": {"stringValue": "gemini-cli"}}),
    ];
    for (k, v) in &opts.custom_attributes {
        resource_attrs.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    Some(json!({
        "resourceMetrics": [{
            "resource": {"attributes": resource_attrs},
            "scopeMetrics": [{
                "scope": {"name": "trove.adapters.gemini_watcher", "version": env!("CARGO_PKG_VERSION")},
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
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    fn write_session(root: &Path, project: &str, session: &str, lines: &[&str]) -> PathBuf {
        let chats = root.join(project).join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        let path = chats.join(session);
        let mut content = String::new();
        for l in lines {
            content.push_str(l);
            content.push('\n');
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    fn user_line(ts: &str, text: &str) -> String {
        format!(
            r#"{{"id":"u1","timestamp":"{ts}","type":"user","content":[{{"text":"{text}"}}]}}"#
        )
    }

    fn gemini_line(ts: &str, input: u64, output: u64, model: &str) -> String {
        format!(
            r#"{{"id":"g1","timestamp":"{ts}","type":"gemini","content":"hi","tokens":{{"input":{input},"output":{output},"cached":0,"thoughts":0,"tool":0,"total":{total}}},"model":"{model}"}}"#,
            total = input + output
        )
    }

    fn meta_line(session: &str) -> String {
        format!(
            r#"{{"sessionId":"{session}","projectHash":"abc","startTime":"2026-05-12T18:00:00.000Z","lastUpdated":"2026-05-12T18:00:00.000Z","kind":"main"}}"#
        )
    }

    #[allow(clippy::type_complexity)]
    fn collector() -> (
        Arc<Mutex<Vec<Value>>>,
        impl Fn(Value) -> std::future::Ready<Result<(), otlp_emit::OtlpEmitError>>
            + Send
            + Sync
            + 'static,
    ) {
        let bucket: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let b2 = bucket.clone();
        let emit = move |p: Value| {
            // Synchronous capture is fine for tests; tokio runtime ticks
            // around the await of the returned ready future.
            let b3 = b2.clone();
            tokio::spawn(async move {
                b3.lock().await.push(p);
            });
            std::future::ready(Ok(()))
        };
        (bucket, emit)
    }

    #[tokio::test]
    async fn no_chats_dir_is_a_no_op() {
        let root = tempdir().unwrap();
        let (bucket, emit) = collector();
        let mut wm = HashMap::new();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        // give tokio a moment to drain the no-op
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(bucket.lock().await.is_empty());
    }

    #[tokio::test]
    async fn emits_events_tokens_cost_and_duration_for_a_completed_turn() {
        let root = tempdir().unwrap();
        write_session(
            root.path(),
            "proj-a",
            "session-1.jsonl",
            &[
                &meta_line("s1"),
                &user_line("2026-05-12T18:00:01.000Z", "hi"),
                &gemini_line(
                    "2026-05-12T18:00:04.000Z",
                    1000,
                    250,
                    "claude-sonnet-4",
                ),
            ],
        );
        let (bucket, emit) = collector();
        let mut wm = HashMap::new();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        let payloads = bucket.lock().await.clone();
        assert_eq!(payloads.len(), 1, "exactly one emission per scan");
        let names: Vec<&str> = payloads[0]["resourceMetrics"][0]["scopeMetrics"][0]
            ["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"trove.harness.events"));
        assert!(names.contains(&"trove.harness.tokens"));
        assert!(names.contains(&"trove.harness.cost.usd"));
        assert!(names.contains(&"trove.harness.turn.duration"));

        // Resource attributes pin harness.id.
        let attrs = payloads[0]["resourceMetrics"][0]["resource"]["attributes"]
            .as_array()
            .unwrap();
        assert!(attrs
            .iter()
            .any(|a| a["key"] == "harness.id" && a["value"]["stringValue"] == "gemini-cli"));
    }

    #[tokio::test]
    async fn watermark_prevents_re_emit_on_unchanged_file() {
        let root = tempdir().unwrap();
        write_session(
            root.path(),
            "proj-a",
            "session-1.jsonl",
            &[
                &meta_line("s1"),
                &user_line("2026-05-12T18:00:01.000Z", "hi"),
                &gemini_line(
                    "2026-05-12T18:00:04.000Z",
                    1000,
                    250,
                    "claude-sonnet-4",
                ),
            ],
        );
        let (bucket, emit) = collector();
        let mut wm = HashMap::new();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let payloads = bucket.lock().await.clone();
        assert_eq!(payloads.len(), 1, "second scan must be a no-op");
    }

    #[tokio::test]
    async fn appended_turn_emits_only_the_new_record() {
        let root = tempdir().unwrap();
        let path = write_session(
            root.path(),
            "proj-a",
            "session-1.jsonl",
            &[
                &meta_line("s1"),
                &user_line("2026-05-12T18:00:01.000Z", "hi"),
                &gemini_line(
                    "2026-05-12T18:00:04.000Z",
                    1000,
                    250,
                    "claude-sonnet-4",
                ),
            ],
        );
        let (bucket, emit) = collector();
        let mut wm = HashMap::new();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        // Append a second turn.
        let new_lines = format!(
            "{}\n{}\n",
            user_line("2026-05-12T18:00:10.000Z", "another"),
            gemini_line("2026-05-12T18:00:14.000Z", 200, 40, "claude-sonnet-4")
        );
        let mut existing = std::fs::read_to_string(&path).unwrap();
        existing.push_str(&new_lines);
        std::fs::write(&path, existing).unwrap();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let payloads = bucket.lock().await.clone();
        assert_eq!(payloads.len(), 2);
        // The second emission should cover exactly one turn.
        let second_events = payloads[1]["resourceMetrics"][0]["scopeMetrics"][0]
            ["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == "trove.harness.events")
            .unwrap();
        let points = second_events["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(points.len(), 1);
    }

    #[tokio::test]
    async fn ignores_partial_trailing_line() {
        let root = tempdir().unwrap();
        let chats = root.path().join("proj").join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        let path = chats.join("session-1.jsonl");
        // Final line is incomplete (no newline) — must NOT be parsed.
        let content = format!(
            "{}\n{}\n{}",
            meta_line("s1"),
            user_line("2026-05-12T18:00:01.000Z", "hi"),
            r#"{"id":"g1","timestamp":"2026-05-12T18:00:04.000Z","type":"gemini","#
        );
        std::fs::write(&path, content).unwrap();
        let (bucket, emit) = collector();
        let mut wm = HashMap::new();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        // Only the user line is complete; no gemini turn yet ⇒ no emission.
        assert!(bucket.lock().await.is_empty());
    }

    #[tokio::test]
    async fn user_line_in_one_tick_and_gemini_in_the_next_pairs_correctly() {
        let root = tempdir().unwrap();
        let chats = root.path().join("proj").join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        let path = chats.join("session-1.jsonl");
        let first = format!(
            "{}\n{}\n",
            meta_line("s1"),
            user_line("2026-05-12T18:00:01.000Z", "hi"),
        );
        std::fs::write(&path, &first).unwrap();
        let (bucket, emit) = collector();
        let mut wm = HashMap::new();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        // Append the gemini response.
        let mut more = first;
        more.push_str(&gemini_line(
            "2026-05-12T18:00:07.500Z",
            500,
            120,
            "gemini-2.5-pro",
        ));
        more.push('\n');
        std::fs::write(&path, &more).unwrap();
        scan_once(root.path(), &ApplyOptions::default(), rules_arc(), &mut wm, &emit)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let payloads = bucket.lock().await.clone();
        assert_eq!(payloads.len(), 1, "user-only tick must not emit");

        // Duration histogram must record ~6.5s in the 10s bucket.
        let duration = payloads[0]["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == "trove.harness.turn.duration")
            .expect("duration metric");
        let sum = duration["histogram"]["dataPoints"][0]["sum"].as_f64().unwrap();
        assert!((sum - 6.5).abs() < 0.01);
    }

    #[test]
    fn parse_gemini_record_with_no_paired_user_yields_no_duration() {
        let v: Value = serde_json::from_str(
            r#"{"timestamp":"2026-05-12T18:00:04.000Z","type":"gemini","tokens":{"input":10,"output":2},"model":"gemini-2.5-pro"}"#,
        )
        .unwrap();
        let turn = parse_gemini_record(&v, None).unwrap();
        assert!(turn.duration_s.is_none());
        assert_eq!(turn.input_tokens, 10);
        assert_eq!(turn.output_tokens, 2);
        assert_eq!(turn.model, "gemini-2.5-pro");
    }

    #[test]
    fn missing_tokens_object_falls_back_to_zero() {
        let v: Value = serde_json::from_str(
            r#"{"timestamp":"2026-05-12T18:00:04.000Z","type":"gemini","model":"gemini-2.5-pro"}"#,
        )
        .unwrap();
        let turn = parse_gemini_record(&v, None).unwrap();
        assert_eq!(turn.input_tokens, 0);
        assert_eq!(turn.output_tokens, 0);
    }
}
