//! Integration tests for the `app_state` persistence module.
//!
//! These exercise the public `*_in(config_dir, ...)` entry points — the
//! same code path the Tauri commands hit via the `AppHandle` convenience
//! layer, just rooted at a `tempdir` instead of the OS-native config
//! directory. They run in CI by default and don't touch the keychain.

use std::collections::BTreeMap;

use tempfile::tempdir;
use trove_app::adapters::{ApplyOptions, TrovePatch};
use trove_app::app_state::{
    AppState, Backend, CURRENT_SCHEMA_VERSION, HarnessConfig, OtlpProtocol, SecretRef,
    load_from_dir, remove_harness_in, save_to_dir, state_path_in, upsert_harness_in,
};
use trove_app::harness::HarnessId;
use trove_app::safety::sentinels::Format;

fn signoz() -> Backend {
    Backend::Signoz {
        region: "us-east".into(),
        ingestion_key: SecretRef::for_account("backend.signoz.ingestion-key"),
    }
}

fn sample_config(id: HarnessId) -> HarnessConfig {
    HarnessConfig {
        id,
        enabled: true,
        config_path: "/tmp/x".into(),
        last_patched_at: "2026-05-07T00:00:00Z".into(),
        trove_patch: TrovePatch {
            managed_block_hash: "a".repeat(64),
            file_hash_at_last_write: "b".repeat(64),
            format: Format::Json,
            last_written_region_payload: r#"{"env":{"OTEL_FOO":"bar"}}"#.into(),
        },
        options: ApplyOptions::default(),
    }
}

#[test]
fn fresh_dir_loads_default_state() {
    let dir = tempdir().unwrap();
    let state = load_from_dir(dir.path()).unwrap();
    assert_eq!(state, AppState::default());
    assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn save_then_load_is_byte_identical_for_each_backend_kind() {
    // OTLP-generic exercises the variadic-headers branch, the trickiest
    // of the six. Round-tripping it covers the BTreeMap ordering too.
    let mut headers: BTreeMap<String, SecretRef> = BTreeMap::new();
    headers.insert(
        "x-honeycomb-team".into(),
        SecretRef::for_account("backend.otlp-generic.header.x-honeycomb-team"),
    );
    headers.insert(
        "x-trace-id".into(),
        SecretRef::for_account("backend.otlp-generic.header.x-trace-id"),
    );

    let backends = vec![
        signoz(),
        Backend::Honeycomb {
            team: SecretRef::for_account("backend.honeycomb.team"),
            dataset: "main".into(),
        },
        Backend::GrafanaCloud {
            endpoint: "https://otlp.grafana.net".into(),
            auth: SecretRef::for_account("backend.grafana-cloud.auth"),
        },
        Backend::Datadog {
            site: "datadoghq.eu".into(),
            api_key: SecretRef::for_account("backend.datadog.api-key"),
        },
        Backend::OtlpGeneric {
            endpoint: "https://otel.example.com".into(),
            protocol: OtlpProtocol::Http,
            headers,
        },
        Backend::OtelcolPassthrough {
            endpoint: "127.0.0.1:4318".into(),
        },
    ];

    for backend in backends {
        let dir = tempdir().unwrap();
        let state = AppState {
            schema_version: 3,
            backend: Some(backend.clone()),
            harnesses: vec![sample_config(HarnessId::ClaudeCode)],
        };
        save_to_dir(dir.path(), &state).unwrap();
        let revived = load_from_dir(dir.path()).unwrap();
        assert_eq!(state, revived, "round-trip failed for backend {backend:?}");
    }
}

#[test]
fn upsert_replaces_by_id_and_remove_targets_one_id() {
    let dir = tempdir().unwrap();
    upsert_harness_in(dir.path(), sample_config(HarnessId::ClaudeCode)).unwrap();
    upsert_harness_in(dir.path(), sample_config(HarnessId::GeminiCli)).unwrap();
    upsert_harness_in(dir.path(), sample_config(HarnessId::CodexCli)).unwrap();
    assert_eq!(load_from_dir(dir.path()).unwrap().harnesses.len(), 3);

    let mut updated = sample_config(HarnessId::ClaudeCode);
    updated.last_patched_at = "2026-05-08T01:02:03Z".into();
    upsert_harness_in(dir.path(), updated).unwrap();

    let state = load_from_dir(dir.path()).unwrap();
    assert_eq!(state.harnesses.len(), 3, "upsert should not append for an existing id");
    let claude = state
        .harnesses
        .iter()
        .find(|h| h.id == HarnessId::ClaudeCode)
        .unwrap();
    assert_eq!(claude.last_patched_at, "2026-05-08T01:02:03Z");

    remove_harness_in(dir.path(), HarnessId::GeminiCli).unwrap();
    let state = load_from_dir(dir.path()).unwrap();
    assert_eq!(state.harnesses.len(), 2);
    assert!(!state.harnesses.iter().any(|h| h.id == HarnessId::GeminiCli));
}

#[test]
fn save_creates_intermediate_directories() {
    let outer = tempdir().unwrap();
    let nested = outer.path().join("nope/yet");
    save_to_dir(&nested, &AppState::default()).unwrap();
    assert!(state_path_in(&nested).exists());
}
