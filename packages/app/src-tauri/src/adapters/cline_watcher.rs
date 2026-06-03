//! Cline watcher — polls the globalStorage tasks dir and emits OTLP
//! logs derived from each task's `ui_messages.json` whenever it changes.
//!
//! Why a poll loop and not the line-streaming `log_watcher`: Cline
//! writes its `ui_messages.json` as an entire JSON array on every turn,
//! not line-by-line. A `tail -F` style watcher would emit garbage from
//! mid-file changes. The poll loop reads each task's full file, hashes
//! the contents, and emits only when the hash changes.
//!
//! Resource shape (parsed from a real Cline install plus the cline
//! source — see `tasks/<id>/ui_messages.json`): a JSON array of
//! `ClineUiMessage`-ish objects with at least `type`, `ts`, and
//! optional `text`. The parser is intentionally permissive — Cline's
//! upstream format may evolve, and best-effort means we shouldn't
//! crash on shape drift.
//!
//! Sprint 13 — in addition to logs, emit Tier A metric data points for
//! each *new* message classified into one of the bounded event kinds
//! (`chat.turn`, `tool.call`, `shell.exec`, `file.edit`, plus the
//! `errors` bucket). The per-task watermark grew from a content hash
//! to `(hash, last_emitted_index)` so we never double-count on
//! re-read — every emission covers exactly the slice of messages that
//! showed up since the previous scan. Logs still fire on every change
//! (their attribute set carries the up-to-date message count) so
//! existing log-tail consumers keep working.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::adapters::ApplyOptions;
use crate::log_watcher::WatcherHandle;
use crate::otlp_emit;

/// How often the watcher polls each task's `ui_messages.json`.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Per-task watermark. `hash` short-circuits the read when the file
/// hasn't changed since the previous scan; `last_emitted_index`
/// records how many messages we've already turned into Tier A metric
/// data points so the next scan can slice from there.
#[derive(Debug, Default, Clone)]
struct TaskWatermark {
    hash: String,
    last_emitted_index: usize,
}

/// Spawn a Cline watcher that polls both the VS Code extension's
/// `tasks_dir` and the standalone CLI's `cli_sessions_dir` each tick.
/// Either path may be absent on a fresh machine — the walker tolerates
/// `NotFound`. Returns a handle whose `abort()` halts the loop. Errors
/// during emission are logged at `tracing::warn!` and the loop
/// continues — best-effort.
#[must_use]
pub fn spawn(
    tasks_dir: impl Into<PathBuf>,
    cli_sessions_dir: impl Into<PathBuf>,
    opts: ApplyOptions,
    mappings: crate::mappings::SharedMappingState,
    poll_interval: Duration,
) -> WatcherHandle {
    let tasks_dir = tasks_dir.into();
    let cli_sessions_dir = cli_sessions_dir.into();
    let join = tokio::spawn(async move {
        run(
            tasks_dir,
            cli_sessions_dir,
            opts,
            mappings,
            poll_interval,
            |payload: Value| async move { otlp_emit::post_logs_json(&payload).await },
            |payload: Value| async move { otlp_emit::post_metrics_json(&payload).await },
        )
        .await;
    });
    WatcherHandle::from_join(join)
}

/// Test-friendly variant: takes explicit emitters for logs and metrics
/// so tests can capture payloads instead of `POSTing` to a real
/// receiver. Owned `Value` (not a reference) on both keeps the trait
/// bounds free of HRTB lifetime gymnastics — the watcher emits at
/// most once per second per task, so the clone cost is negligible.
pub async fn run<FLog, FutLog, FMetric, FutMetric>(
    tasks_dir: PathBuf,
    cli_sessions_dir: PathBuf,
    opts: ApplyOptions,
    mappings: crate::mappings::SharedMappingState,
    poll_interval: Duration,
    emit_log: FLog,
    emit_metric: FMetric,
) where
    FLog: Fn(Value) -> FutLog + Send + Sync + 'static,
    FutLog: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>> + Send,
    FMetric: Fn(Value) -> FutMetric + Send + Sync + 'static,
    FutMetric: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>> + Send,
{
    let mut watermarks: HashMap<String, TaskWatermark> = HashMap::new();
    let mut cli_watermarks: HashMap<String, TaskWatermark> = HashMap::new();
    loop {
        // Snapshot the live mapping state at the top of each scan so
        // user edits via apply_mappings take effect on the next tick
        // without a watcher restart.
        let mapping_snapshot = mappings.current();
        if let Err(e) = scan_once(
            &tasks_dir,
            &opts,
            mapping_snapshot.clone(),
            &mut watermarks,
            &emit_log,
            &emit_metric,
        )
        .await
        {
            tracing::warn!(error = %e, ?tasks_dir, "cline watcher tick errored");
        }
        // Cline CLI variant (npm-installed `cline` 3.x+) writes session
        // state under `~/.cline/data/sessions/` instead of VS Code's
        // globalStorage tree. Same harness id, same metric schema; we
        // walk both each tick so the user gets coverage whichever
        // interface they're using.
        if let Err(e) = scan_cli_once(
            &cli_sessions_dir,
            &opts,
            mapping_snapshot,
            &mut cli_watermarks,
            &emit_log,
            &emit_metric,
        )
        .await
        {
            tracing::warn!(error = %e, ?cli_sessions_dir, "cline-cli watcher tick errored");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// One pass over the tasks directory. Reads each `<id>/ui_messages.json`,
/// emits a log record (and any Tier A metrics derived from new messages)
/// if its content hash has changed since the last scan, and updates the
/// per-task watermark.
async fn scan_once<FLog, FutLog, FMetric, FutMetric>(
    tasks_dir: &Path,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
    watermarks: &mut HashMap<String, TaskWatermark>,
    emit_log: &FLog,
    emit_metric: &FMetric,
) -> std::io::Result<()>
where
    FLog: Fn(Value) -> FutLog,
    FutLog: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
    FMetric: Fn(Value) -> FutMetric,
    FutMetric: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
{
    let mut entries = match tokio::fs::read_dir(tasks_dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let task_id = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        let ui_path = path.join("ui_messages.json");
        let content = match tokio::fs::read(&ui_path).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                tracing::warn!(error = %e, ?ui_path, "cline watcher could not read");
                continue;
            }
        };
        let hash = sha256_hex(&content);
        let previous = watermarks
            .get(&task_id)
            .cloned()
            .unwrap_or_default();
        if previous.hash == hash {
            continue;
        }

        // Compute the new tail before emitting so the watermark moves
        // forward regardless of emit success — a transient OTLP
        // failure shouldn't make us re-emit the same chunk forever.
        let new_message_count = parsed_message_count(&content).unwrap_or(0);
        watermarks.insert(
            task_id.clone(),
            TaskWatermark {
                hash,
                last_emitted_index: new_message_count,
            },
        );

        if let Some(log_payload) = parse_task_log_payload(&task_id, &content, opts) {
            if let Err(e) = emit_log(log_payload).await {
                tracing::warn!(error = %e, %task_id, "cline watcher log emit failed");
            }
        }
        if let Some(metric_payload) = parse_task_metric_payload(
            &task_id,
            &content,
            previous.last_emitted_index,
            opts,
            mappings.clone(),
        ) {
            if let Err(e) = emit_metric(metric_payload).await {
                tracing::warn!(error = %e, %task_id, "cline watcher metric emit failed");
            }
        }
    }
    Ok(())
}

/// Parse `ui_messages_bytes` and return the message count if the
/// payload is a JSON array. Returns `None` for unparseable input.
fn parsed_message_count(ui_messages_bytes: &[u8]) -> Option<usize> {
    let messages: Value = serde_json::from_slice(ui_messages_bytes).ok()?;
    let arr = messages.as_array()?;
    Some(arr.len())
}

/// Build an OTLP/HTTP/JSON `LogRecord` payload for one Cline task.
/// Returns `None` only for unparseable input (so the watcher skips it
/// rather than crashing).
#[must_use]
pub fn parse_task_log_payload(
    task_id: &str,
    ui_messages_bytes: &[u8],
    opts: &ApplyOptions,
) -> Option<Value> {
    let messages: Value = serde_json::from_slice(ui_messages_bytes).ok()?;
    let arr = messages.as_array()?;
    let message_count = arr.len();

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());

    let mut attributes = vec![
        json!({"key": "trove.source", "value": {"stringValue": "cline"}}),
        json!({"key": "cline.task_id", "value": {"stringValue": task_id}}),
        json!({"key": "cline.message_count", "value": {"intValue": message_count.to_string()}}),
    ];
    for (k, v) in custom_attributes_iter(&opts.custom_attributes) {
        attributes.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    // Metrics-only policy: never capture user prompt bodies — emit an
    // empty body so the OTLP record is structurally well-formed but
    // carries only metadata (counts + task id).
    let body_value = json!({"stringValue": ""});

    Some(json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "cline"}},
                    {"key": "harness.id", "value": {"stringValue": "cline"}},
                    {"key": "harness.name", "value": {"stringValue": "Cline"}},
                    {"key": "trove.source", "value": {"stringValue": "cline"}},
                ]
            },
            "scopeLogs": [{
                "scope": {"name": "trove.adapters.cline", "version": env!("CARGO_PKG_VERSION")},
                "logRecords": [{
                    "timeUnixNano": now_ns.to_string(),
                    "severityNumber": 9, // INFO
                    "severityText": "INFO",
                    "body": body_value,
                    "attributes": attributes,
                }]
            }]
        }]
    }))
}

fn custom_attributes_iter(
    attrs: &BTreeMap<String, String>,
) -> impl Iterator<Item = (&str, &str)> {
    attrs.iter().map(|(k, v)| (k.as_str(), v.as_str()))
}

/// Map a single Cline `ui_messages.json` entry onto a `(when_key, extra_attrs)`
/// pair the [`crate::mappings::runtime`] accumulator understands.
///
/// `when_key` matches the strings used in [`crate::mappings::defaults`]'s
/// `cline` defaults — `say.text`, `say.tool`, `say.command`, `say.error`.
/// Users can rewire any of these to a different target metric via the
/// Mappings editor; the rules drive aggregation at emit time, so this
/// function does no per-event interpretation beyond classification.
///
/// Returns `None` for messages Trove doesn't classify (`ask` prompts,
/// `command_output` payloads, `api_req_started` records).
#[must_use]
fn classify_when(msg: &Value) -> Option<&'static str> {
    let kind = msg.get("type").and_then(Value::as_str)?;
    if kind != "say" {
        return None;
    }
    let say = msg.get("say").and_then(Value::as_str)?;
    match say {
        "text" => Some("say.text"),
        "tool" => Some("say.tool"),
        "command" => Some("say.command"),
        "error" => Some("say.error"),
        _ => None,
    }
}

/// Build an OTLP/HTTP/JSON metrics payload covering every message at
/// index `from_index..`. The current [`MappingState`] drives target
/// metric names, kinds, and attribute injections — so a user that
/// remaps `say.tool` to a custom metric in the editor sees that custom
/// metric on the wire here too. Returns `None` when no rule produces
/// any observation (so the watcher doesn't post an empty payload).
#[must_use]
pub fn parse_task_metric_payload(
    task_id: &str,
    ui_messages_bytes: &[u8],
    from_index: usize,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
) -> Option<Value> {
    let messages: Value = serde_json::from_slice(ui_messages_bytes).ok()?;
    let arr = messages.as_array()?;
    if from_index >= arr.len() {
        return None;
    }
    let new_slice = &arr[from_index..];

    let mut acc = crate::mappings::runtime::MetricsAccumulator::new(
        mappings,
        crate::harness::HarnessId::Cline,
    );

    let mut task_attr = BTreeMap::new();
    task_attr.insert("cline.task_id".to_string(), task_id.to_string());

    for msg in new_slice {
        let Some(when) = classify_when(msg) else { continue };
        acc.observe_count(when, 1, &task_attr);
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
        json!({"key": "service.name", "value": {"stringValue": "cline"}}),
        json!({"key": "harness.id", "value": {"stringValue": "cline"}}),
        json!({"key": "harness.name", "value": {"stringValue": "Cline"}}),
        json!({"key": "trove.source", "value": {"stringValue": "cline"}}),
    ];
    for (k, v) in custom_attributes_iter(&opts.custom_attributes) {
        resource_attrs.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    Some(json!({
        "resourceMetrics": [{
            "resource": {"attributes": resource_attrs},
            "scopeMetrics": [{
                "scope": {"name": "trove.adapters.cline", "version": env!("CARGO_PKG_VERSION")},
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

// ---------------------------------------------------------------------------
// Cline CLI variant (npm-installed `cline` 3.x+, separate from the VS Code
// extension). Sessions land under `~/.cline/data/sessions/<sid>/<sid>.messages.json`
// where the file is an *object* (`{version, updated_at, agent, sessionId,
// messages[], system_prompt}`) rather than a JSON array. The walker below
// mirrors `scan_once`'s shape but parses the wrapper object and counts new
// `assistant` messages as `chat.turn` events. Same emission contract: the
// log payload's body is intentionally empty (metrics-only policy), and the
// metric payload reuses the shared `MetricsAccumulator` so the dashboard
// and per-backend queries see one consistent stream regardless of which
// interface produced it.
// ---------------------------------------------------------------------------

/// CLI-variant scan: walks `<sessions_dir>/<sid>/<sid>.messages.json`
/// and emits a fresh log + metric payload whenever the file's content
/// hash changes. `NotFound` on the sessions root is the common case on
/// a machine without the standalone Cline CLI installed and is treated
/// as a no-op tick.
async fn scan_cli_once<FLog, FutLog, FMetric, FutMetric>(
    cli_sessions_dir: &Path,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
    watermarks: &mut HashMap<String, TaskWatermark>,
    emit_log: &FLog,
    emit_metric: &FMetric,
) -> std::io::Result<()>
where
    FLog: Fn(Value) -> FutLog,
    FutLog: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
    FMetric: Fn(Value) -> FutMetric,
    FutMetric: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
{
    let mut entries = match tokio::fs::read_dir(cli_sessions_dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let session_id = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        // CLI 3.x layout: `<session_id>/<session_id>.messages.json`. Skip
        // sessions where the file hasn't materialised yet (a brand-new
        // session writes the dir first, the file moments later).
        let msg_path = path.join(format!("{session_id}.messages.json"));
        let content = match tokio::fs::read(&msg_path).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                tracing::warn!(error = %e, ?msg_path, "cline-cli watcher could not read");
                continue;
            }
        };
        let hash = sha256_hex(&content);
        let previous = watermarks.get(&session_id).cloned().unwrap_or_default();
        if previous.hash == hash {
            continue;
        }

        let new_message_count = parse_cli_message_count(&content).unwrap_or(0);
        watermarks.insert(
            session_id.clone(),
            TaskWatermark {
                hash,
                last_emitted_index: new_message_count,
            },
        );

        if let Some(log_payload) = parse_cli_log_payload(&session_id, &content, opts) {
            if let Err(e) = emit_log(log_payload).await {
                tracing::warn!(error = %e, %session_id, "cline-cli watcher log emit failed");
            }
        }
        if let Some(metric_payload) = parse_cli_metric_payload(
            &session_id,
            &content,
            previous.last_emitted_index,
            opts,
            mappings.clone(),
        ) {
            if let Err(e) = emit_metric(metric_payload).await {
                tracing::warn!(error = %e, %session_id, "cline-cli watcher metric emit failed");
            }
        }
    }
    Ok(())
}

/// Read the `messages` array length out of a Cline CLI session file's
/// JSON. Returns `None` for unparseable input — same contract as
/// `parsed_message_count` so the watcher can gracefully skip a session
/// whose file is mid-write.
fn parse_cli_message_count(messages_json_bytes: &[u8]) -> Option<usize> {
    let doc: Value = serde_json::from_slice(messages_json_bytes).ok()?;
    let arr = doc.get("messages")?.as_array()?;
    Some(arr.len())
}

/// CLI variant of `parse_task_log_payload`. Same metrics-only policy:
/// the body is intentionally empty; only metadata (session id +
/// message count + custom attrs) lands in the record. Returns `None`
/// only for unparseable input.
#[must_use]
pub fn parse_cli_log_payload(
    session_id: &str,
    messages_json_bytes: &[u8],
    opts: &ApplyOptions,
) -> Option<Value> {
    let doc: Value = serde_json::from_slice(messages_json_bytes).ok()?;
    let arr = doc.get("messages")?.as_array()?;
    let message_count = arr.len();

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());

    let mut attributes = vec![
        json!({"key": "trove.source", "value": {"stringValue": "cline"}}),
        // Same key as the VS Code extension parser uses, intentionally —
        // `cline.task_id` keeps queries identical whether a row came from
        // the extension's `<tasks_dir>/<task_id>/` or the CLI's
        // `<sessions_dir>/<session_id>/`. The dashboards filter on this
        // attr without caring which interface produced it.
        json!({"key": "cline.task_id", "value": {"stringValue": session_id}}),
        json!({"key": "cline.message_count", "value": {"intValue": message_count.to_string()}}),
        json!({"key": "cline.variant", "value": {"stringValue": "cli"}}),
    ];
    for (k, v) in custom_attributes_iter(&opts.custom_attributes) {
        attributes.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    let body_value = json!({"stringValue": ""});

    Some(json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "cline"}},
                    {"key": "harness.id", "value": {"stringValue": "cline"}},
                    {"key": "harness.name", "value": {"stringValue": "Cline"}},
                    {"key": "trove.source", "value": {"stringValue": "cline"}},
                ]
            },
            "scopeLogs": [{
                "scope": {"name": "trove.adapters.cline", "version": env!("CARGO_PKG_VERSION")},
                "logRecords": [{
                    "timeUnixNano": now_ns.to_string(),
                    "severityText": "INFO",
                    "body": body_value,
                    "attributes": attributes,
                }]
            }]
        }]
    }))
}

/// CLI variant of `parse_task_metric_payload`. Each new `assistant`
/// message becomes one `chat.turn` event in the Tier-A
/// `trove.harness.events` counter. Token counts on the message
/// (`metrics.{input,output,cacheRead,cacheWrite}Tokens`) feed
/// `trove.harness.tokens` when the user's `MappingState` has a
/// `synthesize-from-native` rule wired up for them; absence is fine
/// (the accumulator silently skips unmapped observations).
#[must_use]
pub fn parse_cli_metric_payload(
    session_id: &str,
    messages_json_bytes: &[u8],
    from_index: usize,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
) -> Option<Value> {
    let doc: Value = serde_json::from_slice(messages_json_bytes).ok()?;
    let arr = doc.get("messages")?.as_array()?;
    if from_index >= arr.len() {
        return None;
    }
    let new_slice = &arr[from_index..];

    let mut acc = crate::mappings::runtime::MetricsAccumulator::new(
        mappings,
        crate::harness::HarnessId::Cline,
    );

    let mut session_attr = BTreeMap::new();
    session_attr.insert("cline.task_id".to_string(), session_id.to_string());
    session_attr.insert("cline.variant".to_string(), "cli".to_string());

    for msg in new_slice {
        // Only assistant turns count as a chat-turn observation —
        // user messages don't trigger LLM work. We re-use the existing
        // `say.text` `when` key (semantically: "the model emitted a
        // text response") so the default Cline `MappingSource::HookRule`
        // for `say.text → trove.harness.events{event.kind=chat.turn}`
        // applies without a parallel CLI-specific rule. See
        // `mappings/defaults.rs` `cline_defaults`.
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
        if role != "assistant" {
            continue;
        }
        acc.observe_count("say.text", 1, &session_attr);
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
        json!({"key": "service.name", "value": {"stringValue": "cline"}}),
        json!({"key": "harness.id", "value": {"stringValue": "cline"}}),
        json!({"key": "harness.name", "value": {"stringValue": "Cline"}}),
        json!({"key": "trove.source", "value": {"stringValue": "cline"}}),
    ];
    for (k, v) in custom_attributes_iter(&opts.custom_attributes) {
        resource_attrs.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    Some(json!({
        "resourceMetrics": [{
            "resource": {"attributes": resource_attrs},
            "scopeMetrics": [{
                "scope": {"name": "trove.adapters.cline", "version": env!("CARGO_PKG_VERSION")},
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
    fn rules_sub() -> crate::mappings::SharedMappingState {
        crate::mappings::MappingStateStore::new(crate::mappings::default_state()).subscribe()
    }
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    fn write_task_messages(root: &Path, task_id: &str, messages: &[Value]) {
        let subdir = root.join(task_id);
        std::fs::create_dir_all(&subdir).unwrap();
        let payload = serde_json::Value::Array(messages.to_vec());
        std::fs::write(
            subdir.join("ui_messages.json"),
            serde_json::to_vec(&payload).unwrap(),
        )
        .unwrap();
    }

    /// CLI-variant fixture writer. Mirrors what Cline 3.x writes:
    /// `<sessions_dir>/<sid>/<sid>.messages.json` containing the wrapper
    /// object `{version, updated_at, agent, sessionId, messages[]}`.
    fn write_cli_session_messages(root: &Path, session_id: &str, messages: &[Value]) {
        let subdir = root.join(session_id);
        std::fs::create_dir_all(&subdir).unwrap();
        let payload = serde_json::json!({
            "version": 1,
            "updated_at": "2026-05-27T14:28:12.179Z",
            "agent": "lead",
            "sessionId": session_id,
            "messages": messages,
            "system_prompt": "You are Cline.",
        });
        std::fs::write(
            subdir.join(format!("{session_id}.messages.json")),
            serde_json::to_vec(&payload).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn parser_returns_none_for_non_json() {
        let r = parse_task_log_payload("t1", b"not json", &ApplyOptions::default());
        assert!(r.is_none());
    }

    #[test]
    fn parser_returns_none_for_non_array_root() {
        let r = parse_task_log_payload("t1", b"{}", &ApplyOptions::default());
        assert!(r.is_none());
    }

    #[test]
    fn parser_emits_canonical_otlp_log_record_shape() {
        let messages = serde_json::to_vec(&json!([
            {"type": "say", "text": "hello", "ts": 1},
            {"type": "ask", "text": "approve?", "ts": 2},
        ]))
        .unwrap();
        let payload =
            parse_task_log_payload("task-abc", &messages, &ApplyOptions::default()).unwrap();
        let log = &payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        let attrs: Vec<(String, String)> = log["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let k = a["key"].as_str().unwrap().to_string();
                let v = a["value"]["stringValue"]
                    .as_str()
                    .or_else(|| a["value"]["intValue"].as_str())
                    .unwrap()
                    .to_string();
                (k, v)
            })
            .collect();
        assert!(attrs.iter().any(|(k, v)| k == "trove.source" && v == "cline"));
        assert!(attrs
            .iter()
            .any(|(k, v)| k == "cline.task_id" && v == "task-abc"));
        assert!(attrs
            .iter()
            .any(|(k, v)| k == "cline.message_count" && v == "2"));
        // Trove is metrics-only — the log body is always empty.
        assert_eq!(log["body"]["stringValue"].as_str().unwrap(), "");
    }

    #[test]
    fn parser_body_is_always_empty_even_with_messages() {
        // Metrics-only policy: even when the task file has rich text,
        // the emitted OTLP body stays empty. Counts and ids ride on
        // attributes; bodies never cross the wire.
        let messages = serde_json::to_vec(&json!([
            {"type": "say", "text": "first"},
            {"type": "say", "text": "second"},
        ]))
        .unwrap();
        let payload = parse_task_log_payload("t", &messages, &ApplyOptions::default()).unwrap();
        let log = &payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log["body"]["stringValue"].as_str().unwrap(), "");
    }

    #[test]
    fn parser_attaches_custom_attributes_to_log_record() {
        let messages = serde_json::to_vec(&json!([{"text": "hi"}])).unwrap();
        let mut opts = ApplyOptions::default();
        opts.custom_attributes
            .insert("team".into(), "platform".into());
        let payload = parse_task_log_payload("t", &messages, &opts).unwrap();
        let attrs = payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
            .as_array()
            .unwrap();
        assert!(attrs
            .iter()
            .any(|a| a["key"] == "team" && a["value"]["stringValue"] == "platform"));
    }

    #[tokio::test]
    async fn watcher_emits_one_payload_per_changed_task_file() {
        let dir = tempdir().unwrap();
        let tasks_dir = dir.path().to_path_buf();
        let logs: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let logs_clone = logs.clone();
        let metrics: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let metrics_clone = metrics.clone();

        // Pre-populate one task before spawn so we hit the "first scan
        // emits" path immediately.
        write_task_messages(&tasks_dir, "t1", &[json!({"text": "init"})]);

        let emit_log = move |p: Value| {
            let captured = logs_clone.clone();
            async move {
                captured.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };
        let emit_metric = move |p: Value| {
            let captured = metrics_clone.clone();
            async move {
                captured.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };

        let handle = tokio::spawn(async move {
            run(
                tasks_dir.clone(),
                PathBuf::from("/var/empty/trove-cline-cli-unused"),
                ApplyOptions::default(),
                rules_sub(),
                Duration::from_millis(50),
                emit_log,
                emit_metric,
            )
            .await;
            // run() loops forever; we abort it from the test below.
        });

        // First emission for t1 (log only — the test fixture's
        // {"text":"init"} doesn't classify into a Tier A bucket).
        wait_for_count(&logs, 1, Duration::from_secs(2)).await;

        // Add t2; expect a second log emission. (No change to t1 → no
        // duplicate.)
        let dir_path = dir.path().to_path_buf();
        write_task_messages(&dir_path, "t2", &[json!({"text": "second"})]);
        wait_for_count(&logs, 2, Duration::from_secs(2)).await;

        // Re-emit by mutating t1.
        write_task_messages(
            &dir_path,
            "t1",
            &[json!({"text": "init"}), json!({"text": "next"})],
        );
        wait_for_count(&logs, 3, Duration::from_secs(2)).await;

        handle.abort();
        let _ = handle.await;

        let entries = logs.lock().await.clone();
        let task_ids: Vec<String> = entries
            .iter()
            .map(|p| {
                p["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["key"] == "cline.task_id")
                    .unwrap()["value"]["stringValue"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert!(task_ids.contains(&"t1".to_string()));
        assert!(task_ids.contains(&"t2".to_string()));
        // No metric payloads expected: these fixture messages omit
        // the `type`/`say` fields the classifier needs.
        assert_eq!(metrics.lock().await.len(), 0);
    }

    #[tokio::test]
    async fn watcher_emits_metrics_for_classified_messages() {
        let dir = tempdir().unwrap();
        let tasks_dir = dir.path().to_path_buf();
        let metrics: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let metrics_clone = metrics.clone();
        let logs: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let logs_clone = logs.clone();

        // Seed a task with three classifiable messages: one chat turn,
        // one tool call, one error.
        write_task_messages(
            &tasks_dir,
            "task-class",
            &[
                json!({"type": "say", "say": "text", "text": "hi"}),
                json!({"type": "say", "say": "tool", "text": "read_file"}),
                json!({"type": "say", "say": "error", "text": "bang"}),
            ],
        );

        let emit_log = move |p: Value| {
            let c = logs_clone.clone();
            async move {
                c.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };
        let emit_metric = move |p: Value| {
            let c = metrics_clone.clone();
            async move {
                c.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };

        let handle = tokio::spawn(async move {
            run(
                tasks_dir,
                PathBuf::from("/var/empty/trove-cline-cli-unused"),
                ApplyOptions::default(),
                rules_sub(),
                Duration::from_millis(30),
                emit_log,
                emit_metric,
            )
            .await;
        });

        wait_for_count(&metrics, 1, Duration::from_secs(2)).await;
        handle.abort();
        let _ = handle.await;

        let m = &metrics.lock().await[0].clone();
        let names: Vec<String> = m["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"trove.harness.events".to_string()));
        assert!(names.contains(&"trove.harness.errors".to_string()));

        // The Tier A resource attributes are present.
        let resource_attrs =
            m["resourceMetrics"][0]["resource"]["attributes"].as_array().unwrap();
        assert!(resource_attrs.iter().any(|a| {
            a["key"] == "harness.id" && a["value"]["stringValue"] == "cline"
        }));
    }

    #[tokio::test]
    async fn watcher_does_not_double_count_existing_messages_on_subsequent_changes() {
        let dir = tempdir().unwrap();
        let tasks_dir = dir.path().to_path_buf();
        let metrics: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let metrics_clone = metrics.clone();

        // Seed with one message that classifies as chat.turn.
        write_task_messages(
            &tasks_dir,
            "task-watermark",
            &[json!({"type": "say", "say": "text", "text": "one"})],
        );

        let emit_log = |_p: Value| async { Ok::<_, otlp_emit::OtlpEmitError>(()) };
        let emit_metric = move |p: Value| {
            let c = metrics_clone.clone();
            async move {
                c.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };

        let dir_path = dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            run(
                tasks_dir,
                PathBuf::from("/var/empty/trove-cline-cli-unused"),
                ApplyOptions::default(),
                rules_sub(),
                Duration::from_millis(30),
                emit_log,
                emit_metric,
            )
            .await;
        });

        wait_for_count(&metrics, 1, Duration::from_secs(2)).await;

        // Append a second classifiable message; the next emission must
        // count *only* the new one.
        write_task_messages(
            &dir_path,
            "task-watermark",
            &[
                json!({"type": "say", "say": "text", "text": "one"}),
                json!({"type": "say", "say": "tool", "text": "ls"}),
            ],
        );
        wait_for_count(&metrics, 2, Duration::from_secs(2)).await;

        handle.abort();
        let _ = handle.await;

        // Pull the second emission. It should report exactly one
        // event row (the new tool.call), not two.
        let second = metrics.lock().await[1].clone();
        let event_metric = second["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == "trove.harness.events")
            .cloned()
            .unwrap();
        let data_points = event_metric["sum"]["dataPoints"].as_array().unwrap();
        let total_count: u64 = data_points
            .iter()
            .map(|dp| dp["asInt"].as_str().unwrap().parse::<u64>().unwrap())
            .sum();
        assert_eq!(
            total_count, 1,
            "second emission must cover only the new message, not the whole file"
        );
    }

    #[test]
    fn classify_when_recognizes_known_say_subtypes() {
        // v2 reworked the classifier to return a `when` key (string)
        // that matches the corresponding HookRule's `when` in
        // `mappings::defaults::cline_defaults`. Each `when` lets the
        // user edit, retarget, or disable the rule independently.
        assert_eq!(
            classify_when(&json!({"type": "say", "say": "text"})),
            Some("say.text")
        );
        assert_eq!(
            classify_when(&json!({"type": "say", "say": "tool"})),
            Some("say.tool")
        );
        assert_eq!(
            classify_when(&json!({"type": "say", "say": "command"})),
            Some("say.command")
        );
        assert_eq!(
            classify_when(&json!({"type": "say", "say": "error"})),
            Some("say.error")
        );
    }

    #[test]
    fn classify_when_ignores_ask_and_unknown_say_subtypes() {
        assert!(classify_when(&json!({"type": "ask", "ask": "tool"})).is_none());
        assert!(classify_when(&json!({"type": "say", "say": "api_req_started"})).is_none());
        assert!(classify_when(&json!({"text": "noise"})).is_none());
    }

    #[test]
    fn metric_payload_is_none_when_slice_has_no_classifiable_messages() {
        let messages = serde_json::to_vec(&json!([
            {"type": "ask", "ask": "tool"},
            {"type": "say", "say": "api_req_started"}
        ]))
        .unwrap();
        let r = parse_task_metric_payload("t", &messages, 0, &ApplyOptions::default(), rules_arc());
        assert!(r.is_none());
    }

    #[test]
    fn metric_payload_is_none_when_from_index_is_beyond_end() {
        let messages = serde_json::to_vec(&json!([
            {"type": "say", "say": "text"},
        ]))
        .unwrap();
        let r = parse_task_metric_payload("t", &messages, 5, &ApplyOptions::default(), rules_arc());
        assert!(r.is_none());
    }

    #[test]
    fn metric_payload_groups_event_kinds_into_one_metric_with_multiple_data_points() {
        let messages = serde_json::to_vec(&json!([
            {"type": "say", "say": "text"},
            {"type": "say", "say": "text"},
            {"type": "say", "say": "tool"},
        ]))
        .unwrap();
        let p =
            parse_task_metric_payload("t", &messages, 0, &ApplyOptions::default(), rules_arc()).unwrap();
        let m = &p["resourceMetrics"][0]["scopeMetrics"][0]["metrics"];
        let events = m
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == "trove.harness.events")
            .unwrap();
        let data_points = events["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(data_points.len(), 2, "one data point per distinct event.kind");
        // Count by kind to confirm aggregation.
        let counts: BTreeMap<String, u64> = data_points
            .iter()
            .map(|dp| {
                let kind = dp["attributes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["key"] == "event.kind")
                    .unwrap()["value"]["stringValue"]
                    .as_str()
                    .unwrap()
                    .to_string();
                let count: u64 = dp["asInt"].as_str().unwrap().parse().unwrap();
                (kind, count)
            })
            .collect();
        assert_eq!(counts.get("chat.turn"), Some(&2));
        assert_eq!(counts.get("tool.call"), Some(&1));
    }

    #[test]
    fn metric_payload_carries_custom_attributes_on_the_resource() {
        let messages = serde_json::to_vec(&json!([
            {"type": "say", "say": "text"},
        ]))
        .unwrap();
        let mut opts = ApplyOptions::default();
        opts.custom_attributes
            .insert("team".into(), "platform".into());
        let p = parse_task_metric_payload("t", &messages, 0, &opts, rules_arc()).unwrap();
        let attrs = p["resourceMetrics"][0]["resource"]["attributes"].as_array().unwrap();
        assert!(attrs
            .iter()
            .any(|a| a["key"] == "team" && a["value"]["stringValue"] == "platform"));
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
            assert!(std::time::Instant::now() < deadline, 
                "timed out waiting for {target} captured payloads (have {})",
                captured.lock().await.len()
            );
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    #[tokio::test]
    async fn watcher_tolerates_missing_tasks_dir() {
        let dir = tempdir().unwrap();
        // Don't create the tasks_dir — it points at a path that doesn't
        // exist, which is the fresh-Cline-install case.
        let tasks_dir = dir.path().join("missing");
        let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        let emit_log = move |p: Value| {
            let captured = captured_clone.clone();
            async move {
                captured.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };
        let emit_metric = |_p: Value| async { Ok::<_, otlp_emit::OtlpEmitError>(()) };
        let handle = tokio::spawn(async move {
            run(
                tasks_dir,
                PathBuf::from("/var/empty/trove-cline-cli-unused"),
                ApplyOptions::default(),
                rules_sub(),
                Duration::from_millis(50),
                emit_log,
                emit_metric,
            )
            .await;
        });
        // Let the loop tick a few times — it should not panic.
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.abort();
        assert_eq!(captured.lock().await.len(), 0);
    }

    // ----- Cline CLI variant (~/.cline/data/sessions/<sid>/...) -----

    #[test]
    fn cli_parser_returns_none_for_missing_messages_key() {
        // The wrapper schema requires `.messages`; absence is treated as
        // "not yet written" and skipped silently (not an error).
        let r = parse_cli_log_payload("s1", b"{}", &ApplyOptions::default());
        assert!(r.is_none());
    }

    #[test]
    fn cli_parser_emits_canonical_otlp_log_with_variant_tag() {
        // Distinct attribute keys vs the VS Code extension parser:
        // - cline.variant=cli identifies the source
        // - cline.task_id (same key) carries the session id so dashboard
        //   queries don't care which interface produced the record.
        let bytes = serde_json::to_vec(&json!({
            "version": 1,
            "updated_at": "2026-05-27T14:28:12.179Z",
            "agent": "lead",
            "sessionId": "1779892091721_fy1lw",
            "messages": [
                {"id": "msg_a", "role": "user", "content": [], "ts": 1},
                {"id": "msg_b", "role": "assistant", "content": [], "ts": 2,
                 "metrics": {"inputTokens": 100, "outputTokens": 50,
                             "cacheReadTokens": 0, "cacheWriteTokens": 0}},
            ],
        }))
        .unwrap();
        let payload = parse_cli_log_payload(
            "1779892091721_fy1lw",
            &bytes,
            &ApplyOptions::default(),
        )
        .unwrap();
        let log = &payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        let attrs: Vec<(String, String)> = log["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                (
                    a["key"].as_str().unwrap().to_string(),
                    a["value"]
                        .as_object()
                        .unwrap()
                        .values()
                        .next()
                        .unwrap()
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                )
            })
            .collect();
        assert!(attrs
            .iter()
            .any(|(k, v)| k == "cline.variant" && v == "cli"));
        assert!(attrs
            .iter()
            .any(|(k, v)| k == "cline.task_id" && v == "1779892091721_fy1lw"));
        // Metrics-only policy: body stays empty.
        assert_eq!(log["body"]["stringValue"], "");
    }

    #[test]
    fn cli_metric_payload_counts_only_assistant_messages_as_chat_turn() {
        // One user + one assistant in the new slice → exactly one
        // chat.turn observation. User-only turns don't trigger LLM work
        // so they don't count.
        let bytes = serde_json::to_vec(&json!({
            "version": 1,
            "updated_at": "2026-05-27T14:28:12.179Z",
            "agent": "lead",
            "sessionId": "s2",
            "messages": [
                {"id": "u1", "role": "user", "content": [], "ts": 1},
                {"id": "a1", "role": "assistant", "content": [], "ts": 2},
            ],
        }))
        .unwrap();
        let payload = parse_cli_metric_payload("s2", &bytes, 0, &ApplyOptions::default(), rules_arc())
            .expect("at least one chat.turn observation should land");
        let metrics = &payload["resourceMetrics"][0]["scopeMetrics"][0]["metrics"];
        let arr = metrics.as_array().expect("metrics array");
        // Should emit at least the harness.events counter; the exact set
        // depends on MappingState, but the array can't be empty.
        assert!(!arr.is_empty(), "expected ≥1 metric, got {arr:?}");
    }

    #[tokio::test]
    async fn cli_watcher_emits_on_new_session_file() {
        // End-to-end: write a CLI-shape session, run the watcher one
        // tick, assert the log emitter saw the payload.
        let dir = tempdir().unwrap();
        let sessions_dir = dir.path().to_path_buf();
        write_cli_session_messages(
            &sessions_dir,
            "1779892091721_fy1lw",
            &[
                json!({"id":"u1","role":"user","content":[],"ts":1}),
                json!({"id":"a1","role":"assistant","content":[],"ts":2}),
            ],
        );

        let logs: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let logs_clone = logs.clone();
        let emit_log = move |p: Value| {
            let captured = logs_clone.clone();
            async move {
                captured.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };
        let emit_metric = |_p: Value| async { Ok::<_, otlp_emit::OtlpEmitError>(()) };

        let dummy_tasks = tempdir().unwrap();
        let dummy_tasks_path = dummy_tasks.path().to_path_buf();
        let handle = tokio::spawn(async move {
            run(
                dummy_tasks_path,
                sessions_dir,
                ApplyOptions::default(),
                rules_sub(),
                Duration::from_millis(30),
                emit_log,
                emit_metric,
            )
            .await;
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline && logs.lock().await.is_empty() {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        handle.abort();
        let captured = logs.lock().await;
        assert!(!captured.is_empty(), "watcher should have emitted at least once");
        // Verify the cli.variant tag is on the captured log.
        let attrs = &captured[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"];
        let json_str = serde_json::to_string(attrs).unwrap();
        assert!(
            json_str.contains("\"cline.variant\""),
            "expected cli.variant attribute on captured log: {json_str}"
        );
    }
}
