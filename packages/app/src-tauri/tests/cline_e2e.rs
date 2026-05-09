//! Cline end-to-end fixture test (Sprint 9 PR 2).
//!
//! Sets up a synthetic globalStorage tasks tree, drives the watcher
//! against it, and asserts at least one OTLP `LogRecord`-shaped payload
//! arrives at a captured-stub emitter — proving that the adapter can
//! detect, enable, and emit a signal end-to-end against a fixture run
//! (the Sprint 9 acceptance criterion for Tier 3).

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use trove_app::adapters::cline_watcher;
use trove_app::adapters::ApplyOptions;
use trove_app::otlp_emit::OtlpEmitError;

/// Synthetic globalStorage layout matching Cline's on-disk shape:
/// `<root>/<task-id>/ui_messages.json` per task.
fn write_task(root: &std::path::Path, task_id: &str, messages: &[Value]) {
    let dir = root.join(task_id);
    std::fs::create_dir_all(&dir).unwrap();
    let payload = serde_json::Value::Array(messages.to_vec());
    std::fs::write(
        dir.join("ui_messages.json"),
        serde_json::to_vec(&payload).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn cline_watcher_emits_otlp_log_for_a_fixture_task() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().to_path_buf();
    write_task(
        &tasks_dir,
        "task-fixture-1",
        &[
            json!({"type": "say", "text": "user prompt"}),
            json!({"type": "ask", "text": "approve?"}),
            json!({"type": "say", "text": "assistant response"}),
        ],
    );

    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let emit = move |p: Value| {
        let captured = captured_clone.clone();
        async move {
            captured.lock().await.push(p);
            Ok::<_, OtlpEmitError>(())
        }
    };

    let task = tokio::spawn(async move {
        cline_watcher::run(
            tasks_dir,
            ApplyOptions::default(),
            Duration::from_millis(50),
            emit,
        )
        .await;
    });

    // Wait for the first OTLP payload to land.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if !captured.lock().await.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "watcher did not emit a payload within 3s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    task.abort();
    let _ = task.await;

    let entries = captured.lock().await.clone();
    let log = &entries[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
    let attrs = log["attributes"].as_array().unwrap();
    let trove_source = attrs
        .iter()
        .find(|a| a["key"] == "trove.source")
        .expect("trove.source attribute present");
    assert_eq!(trove_source["value"]["stringValue"], "cline");

    let task_id = attrs
        .iter()
        .find(|a| a["key"] == "cline.task_id")
        .expect("cline.task_id attribute present");
    assert_eq!(task_id["value"]["stringValue"], "task-fixture-1");

    // log_user_prompts is false by default — body must be empty.
    assert_eq!(log["body"]["stringValue"].as_str().unwrap(), "");
}

#[tokio::test]
async fn cline_watcher_emits_only_once_per_unchanged_task() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().to_path_buf();
    write_task(&tasks_dir, "stable", &[json!({"text": "hi"})]);

    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let emit = move |p: Value| {
        let captured = captured_clone.clone();
        async move {
            captured.lock().await.push(p);
            Ok::<_, OtlpEmitError>(())
        }
    };

    let task = tokio::spawn(async move {
        cline_watcher::run(
            tasks_dir,
            ApplyOptions::default(),
            Duration::from_millis(50),
            emit,
        )
        .await;
    });

    // Wait long enough to observe several poll ticks. With one task and
    // no content change, we should see exactly one payload.
    tokio::time::sleep(Duration::from_millis(400)).await;
    task.abort();
    let _ = task.await;

    let entries = captured.lock().await.clone();
    assert_eq!(
        entries.len(),
        1,
        "expected one emission for an unchanging task, got {}",
        entries.len()
    );
}
