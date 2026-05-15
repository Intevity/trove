//! Integration test for the v2 rules-driven emission path.
//!
//! Covers the two invariants the v2 refactor was supposed to deliver:
//!
//! 1. **Custom metrics flow through.** A user-defined metric added to
//!    the catalog gets emitted with the user-chosen wire name when a
//!    rule routes to it. Specifically, we run the Claude Desktop
//!    watcher against a synthetic `audit.jsonl` and assert the OTLP
//!    payload carries the custom name instead of the builtin one.
//! 2. **Live updates land without restart.** Publishing a new
//!    [`MappingState`] to the [`MappingStateStore`] during a running
//!    watcher causes the next scan to emit using the new rules — no
//!    watcher restart, no app restart.
//!
//! The unit tests in `mappings::runtime` and each watcher module
//! validate the per-call behavior; this test is the cross-module
//! integration anchor.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use trove_app::adapters::claude_desktop_watcher;
use trove_app::adapters::ApplyOptions;
use trove_app::harness::HarnessId;
use trove_app::mappings::{
    default_state, HookEmit, MappingSource, MappingStateStore,
    TroveMetricDefinition, TroveMetricKind,
};
use trove_app::otlp_emit::OtlpEmitError;

fn sample_result_row() -> Value {
    json!({
        "type": "result",
        "subtype": "success",
        "duration_ms": 1234.0,
        "total_cost_usd": 0.001,
        "is_error": false,
        "modelUsage": {
            "claude-opus-4-7[1m]": {
                "inputTokens": 5,
                "outputTokens": 10,
                "cacheReadInputTokens": 0,
                "cacheCreationInputTokens": 0,
                "costUSD": 0.001
            }
        }
    })
}

fn write_audit(root: &std::path::Path, rows: &[Value]) -> std::path::PathBuf {
    let dir = root.join("acct").join("ws").join("local_session-1");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("audit.jsonl");
    let body: String = rows
        .iter()
        .map(|r| serde_json::to_string(r).unwrap() + "\n")
        .collect();
    std::fs::write(&path, body).unwrap();
    path
}

fn append_audit(path: &std::path::Path, rows: &[Value]) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    for r in rows {
        writeln!(f, "{}", serde_json::to_string(r).unwrap()).unwrap();
    }
}

fn metric_names(payload: &Value) -> Vec<String> {
    payload["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| m["name"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Replace the default Claude Desktop events rule with one targeting a
/// user-defined catalog entry, and assert the emitted OTLP wire name
/// matches the custom definition (not the builtin `trove.harness.events`).
#[tokio::test]
async fn custom_metric_flows_through_to_emitted_otlp() {
    let dir = tempfile::tempdir().unwrap();
    let sessions_root = dir.path().to_path_buf();

    // Seed a non-empty audit.jsonl so the watcher will skip-to-EOF on
    // first observation; appended rows after that fire emissions.
    let audit_path = write_audit(
        &sessions_root,
        &[json!({"type": "system", "subtype": "init", "model": "claude-opus-4-7[1m]"})],
    );

    // Build a custom-catalog state: add a `my.team.chat_turns` metric
    // and rewire the Claude Desktop chat-turn rule onto it.
    let mut state = default_state();
    state.metrics.push(TroveMetricDefinition {
        id: "chat_turns".into(),
        name: "my.team.chat_turns".into(),
        kind: TroveMetricKind::Counter,
        description: String::new(),
        required_attributes: vec![],
        builtin: false,
    });
    for h in &mut state.harnesses {
        if h.harness_id != HarnessId::ClaudeDesktop {
            continue;
        }
        // Drop existing api_request rules, install one pointing at the
        // custom metric.
        h.sources.retain(|s| {
            !matches!(
                s,
                MappingSource::HookRule { when, emit: Some(e) }
                    if when == "api_request" && e.metric == "events"
            )
        });
        h.sources.push(MappingSource::HookRule {
            when: "api_request".into(),
            emit: Some(HookEmit {
                metric: "chat_turns".into(),
                attributes: {
                    let mut a = BTreeMap::new();
                    a.insert("event.kind".into(), "chat.turn".into());
                    a
                },
            }),
        });
    }

    let store = MappingStateStore::new(state);
    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let emit = move |p: Value| {
        let captured = captured_clone.clone();
        async move {
            captured.lock().await.push(p);
            Ok::<_, OtlpEmitError>(())
        }
    };

    let mappings = store.subscribe();
    let task = tokio::spawn(async move {
        claude_desktop_watcher::run(
            sessions_root,
            ApplyOptions::default(),
            mappings,
            Duration::from_millis(30),
            emit,
        )
        .await;
    });

    // First tick: register watermark at EOF, no emissions.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Now append a result row; this should produce one OTLP payload
    // whose metric name is the custom wire name.
    append_audit(&audit_path, &[sample_result_row()]);

    // Wait for the emission.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if !captured.lock().await.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "watcher did not emit a payload within 3s"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    task.abort();
    let _ = task.await;

    let payloads = captured.lock().await.clone();
    assert!(!payloads.is_empty(), "expected at least one OTLP payload");
    let names = metric_names(&payloads[0]);
    assert!(
        names.contains(&"my.team.chat_turns".to_string()),
        "expected custom wire name in metric set, got {names:?}",
    );
    // And the builtin events name should NOT appear — we redirected the rule.
    assert!(
        !names.contains(&"trove.harness.events".to_string()),
        "expected builtin trove.harness.events to be absent after rewire, got {names:?}",
    );
}

/// While the watcher is running, publish a new `MappingState` that
/// disables Claude Desktop. The next emission attempt produces nothing.
#[tokio::test]
async fn live_update_disables_emission_without_restart() {
    let dir = tempfile::tempdir().unwrap();
    let sessions_root = dir.path().to_path_buf();
    let audit_path = write_audit(
        &sessions_root,
        &[json!({"type": "system", "subtype": "init", "model": "claude-opus-4-7[1m]"})],
    );

    let store = MappingStateStore::new(default_state());
    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let emit = move |p: Value| {
        let captured = captured_clone.clone();
        async move {
            captured.lock().await.push(p);
            Ok::<_, OtlpEmitError>(())
        }
    };
    let mappings = store.subscribe();
    let task = tokio::spawn(async move {
        claude_desktop_watcher::run(
            sessions_root,
            ApplyOptions::default(),
            mappings,
            Duration::from_millis(30),
            emit,
        )
        .await;
    });

    // First tick — watermark at EOF.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Live-update: clone the current state, disable claude_desktop,
    // publish. No watcher restart.
    let mut disabled = (*store.current()).clone();
    for h in &mut disabled.harnesses {
        if h.harness_id == HarnessId::ClaudeDesktop {
            h.enabled = false;
        }
    }
    store.publish(disabled);
    // Give the publish a tick to propagate.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Append a result row. With the harness now disabled in the live
    // state, the accumulator finds no matching rules and emits nothing.
    append_audit(&audit_path, &[sample_result_row()]);

    // Wait long enough for several poll ticks. Nothing should arrive.
    tokio::time::sleep(Duration::from_millis(300)).await;
    task.abort();
    let _ = task.await;

    let payloads = captured.lock().await.clone();
    assert!(
        payloads.is_empty(),
        "watcher should emit nothing once the harness is disabled live; got {} payloads",
        payloads.len()
    );
}

/// Re-enabling a rule via publish brings emission back on the next tick.
#[tokio::test]
async fn live_update_re_enables_emission_without_restart() {
    let dir = tempfile::tempdir().unwrap();
    let sessions_root = dir.path().to_path_buf();
    let audit_path = write_audit(
        &sessions_root,
        &[json!({"type": "system", "subtype": "init", "model": "claude-opus-4-7[1m]"})],
    );

    // Start with claude_desktop disabled.
    let mut initial = default_state();
    for h in &mut initial.harnesses {
        if h.harness_id == HarnessId::ClaudeDesktop {
            h.enabled = false;
        }
    }
    let store = MappingStateStore::new(initial);

    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let emit = move |p: Value| {
        let captured = captured_clone.clone();
        async move {
            captured.lock().await.push(p);
            Ok::<_, OtlpEmitError>(())
        }
    };
    let mappings = store.subscribe();
    let task = tokio::spawn(async move {
        claude_desktop_watcher::run(
            sessions_root,
            ApplyOptions::default(),
            mappings,
            Duration::from_millis(30),
            emit,
        )
        .await;
    });

    // Settle on the watermark.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Live-update: re-enable claude_desktop.
    let mut enabled = default_state();
    for h in &mut enabled.harnesses {
        if h.harness_id == HarnessId::ClaudeDesktop {
            h.enabled = true;
        }
    }
    store.publish(enabled);
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Append a result row — should now emit.
    append_audit(&audit_path, &[sample_result_row()]);

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if !captured.lock().await.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "watcher did not emit after live re-enable within 3s"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    task.abort();
    let _ = task.await;

    let payloads = captured.lock().await.clone();
    assert!(
        !payloads.is_empty(),
        "watcher should emit again once the harness is re-enabled live",
    );
    // Default rules → builtin metric names.
    let names = metric_names(&payloads[0]);
    assert!(
        names.contains(&"trove.harness.events".to_string()),
        "expected builtin metric after live re-enable, got {names:?}",
    );
}
