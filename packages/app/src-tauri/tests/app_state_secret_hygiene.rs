//! Secret-hygiene contract: nothing the user types into the wizard ever
//! lands in `state.json`.
//!
//! These tests exercise the [`drain_secrets_from_draft`] +
//! [`save_to_dir`] sequence the `save_backend` IPC command relies on,
//! using a recognisable canary value the wizard would never produce
//! organically. After the round-trip, we read the bytes off disk and
//! grep for the canary — its absence is the whole assertion.
//!
//! Keeping the keychain out of the loop here means the test runs in CI
//! on Ubuntu without Secret Service. The real keychain round-trip lives
//! in `tests/secrets_keyring.rs` and is gated with `#[ignore]`.

use std::collections::BTreeMap;
use std::fs;

use tempfile::tempdir;
use trove_app::app_state::{
    AppState, BackendDraft, OtlpProtocol, drain_secrets_from_draft, save_to_dir, state_path_in,
};

const CANARY: &str = "ok-secret-test-CAN4RY-please-do-not-leak";

fn save_and_read(state: &AppState) -> String {
    let dir = tempdir().unwrap();
    save_to_dir(dir.path(), state).unwrap();
    fs::read_to_string(state_path_in(dir.path())).unwrap()
}

#[test]
fn signoz_ingestion_key_does_not_land_in_state_json() {
    let draft = BackendDraft::Signoz {
        region: "us-east".into(),
        ingestion_key: CANARY.into(),
    };
    let (backend, secrets) = drain_secrets_from_draft(draft);

    // Sanity check: the raw secret made it into the DraftSecret bag,
    // which is what `save_backend` would push into the keychain.
    assert!(secrets.iter().any(|s| s.value.as_str() == CANARY));

    let state = AppState {
        schema_version: 4,
        backend: Some(backend),
        harnesses: Vec::new(),
        auto_update_enabled: false,
    };
    let on_disk = save_and_read(&state);
    assert!(
        !on_disk.contains(CANARY),
        "state.json contained the raw canary:\n{on_disk}",
    );
}

#[test]
fn honeycomb_team_does_not_land_in_state_json() {
    let draft = BackendDraft::Honeycomb {
        team: CANARY.into(),
        dataset: "main".into(),
    };
    let (backend, _) = drain_secrets_from_draft(draft);
    let state = AppState {
        schema_version: 4,
        backend: Some(backend),
        harnesses: Vec::new(),
        auto_update_enabled: false,
    };
    assert!(!save_and_read(&state).contains(CANARY));
}

#[test]
fn datadog_api_key_does_not_land_in_state_json() {
    let draft = BackendDraft::Datadog {
        site: "datadoghq.eu".into(),
        api_key: CANARY.into(),
    };
    let (backend, _) = drain_secrets_from_draft(draft);
    let state = AppState {
        schema_version: 4,
        backend: Some(backend),
        harnesses: Vec::new(),
        auto_update_enabled: false,
    };
    assert!(!save_and_read(&state).contains(CANARY));
}

#[test]
fn grafana_cloud_auth_does_not_land_in_state_json() {
    let draft = BackendDraft::GrafanaCloud {
        endpoint: "https://otlp.grafana.net".into(),
        auth: CANARY.into(),
    };
    let (backend, _) = drain_secrets_from_draft(draft);
    let state = AppState {
        schema_version: 4,
        backend: Some(backend),
        harnesses: Vec::new(),
        auto_update_enabled: false,
    };
    assert!(!save_and_read(&state).contains(CANARY));
}

#[test]
fn otlp_generic_header_values_do_not_land_in_state_json() {
    // The variadic case: every header value the user types becomes its
    // own keychain entry. None of the values may appear in state.json.
    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    headers.insert("x-honeycomb-team".into(), CANARY.into());
    headers.insert("x-trace-id".into(), format!("{CANARY}-2"));

    let draft = BackendDraft::OtlpGeneric {
        endpoint: "https://otel.example.com".into(),
        protocol: OtlpProtocol::Http,
        headers,
    };
    let (backend, secrets) = drain_secrets_from_draft(draft);
    assert_eq!(secrets.len(), 2);

    let state = AppState {
        schema_version: 4,
        backend: Some(backend),
        harnesses: Vec::new(),
        auto_update_enabled: false,
    };
    let on_disk = save_and_read(&state);
    assert!(!on_disk.contains(CANARY));
    assert!(!on_disk.contains(&format!("{CANARY}-2")));

    // The header *names* are part of the persisted state — by design,
    // so `clear_backend` can iterate them. Confirm they are present.
    assert!(on_disk.contains("x-honeycomb-team"));
    assert!(on_disk.contains("x-trace-id"));
}

#[test]
fn otelcol_passthrough_drain_yields_no_secrets() {
    let draft = BackendDraft::OtelcolPassthrough {
        endpoint: "127.0.0.1:4318".into(),
    };
    let (_backend, secrets) = drain_secrets_from_draft(draft);
    assert!(
        secrets.is_empty(),
        "otelcol-passthrough has no credentials; drain should yield zero secrets",
    );
}
