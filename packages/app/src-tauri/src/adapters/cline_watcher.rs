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

/// Spawn a Cline watcher rooted at `tasks_dir`. Returns a handle whose
/// `abort()` halts the loop. Errors during emission are logged at
/// `tracing::warn!` and the loop continues — best-effort.
#[must_use]
pub fn spawn(
    tasks_dir: impl Into<PathBuf>,
    opts: ApplyOptions,
    poll_interval: Duration,
) -> WatcherHandle {
    let tasks_dir = tasks_dir.into();
    let join = tokio::spawn(async move {
        run(tasks_dir, opts, poll_interval, |payload: Value| async move {
            otlp_emit::post_logs_json(&payload).await
        })
        .await;
    });
    WatcherHandle::from_join(join)
}

/// Test-friendly variant: takes an explicit emitter so tests can
/// capture payloads instead of `POSTing` to a real receiver. The
/// emitter takes an owned `Value` (not a reference) to keep the trait
/// bounds free of HRTB lifetime gymnastics — the watcher emits at
/// most once per second per task, so the clone cost is negligible.
pub async fn run<F, Fut>(
    tasks_dir: PathBuf,
    opts: ApplyOptions,
    poll_interval: Duration,
    emit: F,
) where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>> + Send,
{
    let mut last_seen: HashMap<String, String> = HashMap::new();
    loop {
        if let Err(e) = scan_once(&tasks_dir, &opts, &mut last_seen, &emit).await {
            tracing::warn!(error = %e, ?tasks_dir, "cline watcher tick errored");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// One pass over the tasks directory. Reads each `<id>/ui_messages.json`,
/// emits a log record if its content hash has changed since the last
/// scan, and updates the per-task watermark.
async fn scan_once<F, Fut>(
    tasks_dir: &Path,
    opts: &ApplyOptions,
    last_seen: &mut HashMap<String, String>,
    emit: &F,
) -> std::io::Result<()>
where
    F: Fn(Value) -> Fut,
    Fut: std::future::Future<Output = Result<(), otlp_emit::OtlpEmitError>>,
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
        if last_seen.get(&task_id) == Some(&hash) {
            continue;
        }
        last_seen.insert(task_id.clone(), hash);

        let Some(payload) = parse_task_log_payload(&task_id, &content, opts) else {
            continue;
        };
        if let Err(e) = emit(payload).await {
            tracing::warn!(error = %e, %task_id, "cline watcher OTLP emit failed");
        }
    }
    Ok(())
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
    let last_text = arr
        .iter()
        .rev()
        .find_map(|m| m.get("text").and_then(Value::as_str));

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

    let body_value = if opts.log_user_prompts {
        json!({"stringValue": last_text.unwrap_or("")})
    } else {
        json!({"stringValue": ""})
    };

    Some(json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "cline"}},
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
        // log_user_prompts default false → body is empty string
        assert_eq!(log["body"]["stringValue"].as_str().unwrap(), "");
    }

    #[test]
    fn parser_includes_last_text_when_log_user_prompts_is_true() {
        let messages = serde_json::to_vec(&json!([
            {"type": "say", "text": "first"},
            {"type": "say", "text": "second"},
        ]))
        .unwrap();
        let opts = ApplyOptions {
            log_user_prompts: true,
            ..ApplyOptions::default()
        };
        let payload = parse_task_log_payload("t", &messages, &opts).unwrap();
        let log = &payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log["body"]["stringValue"].as_str().unwrap(), "second");
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
        let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        // Pre-populate one task before spawn so we hit the "first scan
        // emits" path immediately.
        write_task_messages(&tasks_dir, "t1", &[json!({"text": "init"})]);

        let emit = move |p: Value| {
            let captured = captured_clone.clone();
            async move {
                captured.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };

        let handle = tokio::spawn(async move {
            run(tasks_dir.clone(), ApplyOptions::default(), Duration::from_millis(50), emit).await;
            // run() loops forever; we abort it from the test below.
        });

        // First emission for t1.
        wait_for_count(&captured, 1, Duration::from_secs(2)).await;

        // Add t2; expect a second emission. (No change to t1 → no
        // duplicate.)
        let dir_path = dir.path().to_path_buf();
        write_task_messages(&dir_path, "t2", &[json!({"text": "second"})]);
        wait_for_count(&captured, 2, Duration::from_secs(2)).await;

        // Re-emit by mutating t1.
        write_task_messages(
            &dir_path,
            "t1",
            &[json!({"text": "init"}), json!({"text": "next"})],
        );
        wait_for_count(&captured, 3, Duration::from_secs(2)).await;

        handle.abort();
        let _ = handle.await;

        let entries = captured.lock().await.clone();
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
        let emit = move |p: Value| {
            let captured = captured_clone.clone();
            async move {
                captured.lock().await.push(p);
                Ok::<_, otlp_emit::OtlpEmitError>(())
            }
        };
        let handle = tokio::spawn(async move {
            run(tasks_dir, ApplyOptions::default(), Duration::from_millis(50), emit).await;
        });
        // Let the loop tick a few times — it should not panic.
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.abort();
        assert_eq!(captured.lock().await.len(), 0);
    }
}
