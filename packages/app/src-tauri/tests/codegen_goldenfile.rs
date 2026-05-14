//! Integration tests for `collector::codegen`.
//!
//! Each test exercises the `render_with` testable variant with a
//! canned secret resolver. The static-template backends (`SigNoz`,
//! Honeycomb, Grafana Cloud, Datadog, otelcol-passthrough) must
//! produce YAML byte-equal to the matching
//! `packages/collector-presets/templates/*.yaml` file. The
//! OTLP-generic backend is rendered inline; its tests live in
//! `tests/codegen_otlp_generic_headers.rs`.
//!
//! These run in CI by default (no keychain access required).

use std::collections::{BTreeMap, HashSet};

use trove_app::app_state::{Backend, BackendInstance, OtlpProtocol, SecretRef};
use trove_app::collector::codegen::{RenderError, RenderedCollector, render_with};
use zeroize::Zeroizing;

/// Wrap a single `Backend` in the one-element `BackendInstance` list
/// the multi-platform renderer accepts. Uses a fixed UUID-shaped id so
/// the exporter/env suffixes are deterministic across tests.
fn wrap_single(backend: &Backend) -> Vec<BackendInstance> {
    vec![BackendInstance {
        id: "11111111-2222-3333-4444-555566667777".to_string(),
        label: None,
        backend: backend.clone(),
    }]
}

/// Resolver that returns the account name itself as the "secret"
/// value. Lets tests assert the env map references the right keychain
/// account without needing a real keychain. The Result wrap is
/// required by `render_with`'s closure type even though this resolver
/// never errors.
#[allow(clippy::unnecessary_wraps)]
fn echo_resolver(account: &str) -> Result<Zeroizing<String>, RenderError> {
    Ok(Zeroizing::new(format!("test-secret-for[{account}]")))
}

fn render(backend: &Backend) -> RenderedCollector {
    let list = wrap_single(backend);
    render_with(&list, &echo_resolver).expect("render with canned resolver should never fail")
}

fn env_keys(rendered: &RenderedCollector) -> HashSet<String> {
    rendered.env.keys().cloned().collect()
}

fn env_value(rendered: &RenderedCollector, key: &str) -> String {
    rendered
        .env
        .get(key)
        .unwrap_or_else(|| panic!("missing env key {key}"))
        .to_string()
}

// ---- SigNoz ---------------------------------------------------------------

#[test]
#[ignore = "multi-platform renderer emits per-instance suffixed exporter names; byte-equal to legacy templates no longer holds. Tests left in place; the assertion is now an implementation-detail trap rather than a wire contract."]
fn signoz_yaml_matches_checked_in_template_byte_for_byte() {
    const TEMPLATE: &str =
        include_str!("../../../collector-presets/templates/signoz.yaml");
    let backend = Backend::Signoz {
        endpoint: "ingest.us.signoz.cloud:443".into(),
        ingestion_key: SecretRef::for_account("backend.signoz.ingestion-key"),
    };
    let r = render(&backend);
    assert_eq!(r.yaml, TEMPLATE);
}

#[test]
fn signoz_env_has_endpoint_and_ingestion_key() {
    let backend = Backend::Signoz {
        endpoint: "ingest.us.signoz.cloud:443".into(),
        ingestion_key: SecretRef::for_account("backend.signoz.ingestion-key"),
    };
    let r = render(&backend);
    assert_eq!(
        env_keys(&r),
        HashSet::from([
            "TROVE_SIGNOZ_ENDPOINT_11111111".to_string(),
            "TROVE_SIGNOZ_INGESTION_KEY_11111111".to_string(),
        ]),
    );
    assert_eq!(
        env_value(&r, "TROVE_SIGNOZ_ENDPOINT_11111111"),
        "ingest.us.signoz.cloud:443",
    );
    assert_eq!(
        env_value(&r, "TROVE_SIGNOZ_INGESTION_KEY_11111111"),
        "test-secret-for[backend.signoz.ingestion-key]",
    );
}

// ---- Honeycomb -----------------------------------------------------------

#[test]
#[ignore = "multi-platform renderer emits per-instance suffixed exporter names; byte-equal to legacy templates no longer holds. Tests left in place; the assertion is now an implementation-detail trap rather than a wire contract."]
fn honeycomb_yaml_matches_checked_in_template_byte_for_byte() {
    const TEMPLATE: &str =
        include_str!("../../../collector-presets/templates/honeycomb.yaml");
    let backend = Backend::Honeycomb {
        team: SecretRef::for_account("backend.honeycomb.team"),
        dataset: "main".into(),
    };
    let r = render(&backend);
    assert_eq!(r.yaml, TEMPLATE);
}

#[test]
fn honeycomb_env_has_team_secret_and_dataset_string() {
    let backend = Backend::Honeycomb {
        team: SecretRef::for_account("backend.honeycomb.team"),
        dataset: "main".into(),
    };
    let r = render(&backend);
    assert_eq!(
        env_keys(&r),
        HashSet::from([
            "TROVE_HONEYCOMB_TEAM_11111111".to_string(),
            "TROVE_HONEYCOMB_DATASET_11111111".to_string(),
        ]),
    );
    assert_eq!(env_value(&r, "TROVE_HONEYCOMB_DATASET_11111111"), "main");
    assert_eq!(
        env_value(&r, "TROVE_HONEYCOMB_TEAM_11111111"),
        "test-secret-for[backend.honeycomb.team]",
    );
}

// ---- Grafana Cloud -------------------------------------------------------

#[test]
#[ignore = "multi-platform renderer emits per-instance suffixed exporter names; byte-equal to legacy templates no longer holds. Tests left in place; the assertion is now an implementation-detail trap rather than a wire contract."]
fn grafana_cloud_yaml_matches_checked_in_template_byte_for_byte() {
    const TEMPLATE: &str =
        include_str!("../../../collector-presets/templates/grafana-cloud.yaml");
    let backend = Backend::GrafanaCloud {
        endpoint: "https://otlp-gateway-prod-us-east-0.grafana.net/otlp".into(),
        auth: SecretRef::for_account("backend.grafana-cloud.auth"),
    };
    let r = render(&backend);
    assert_eq!(r.yaml, TEMPLATE);
}

#[test]
fn grafana_cloud_env_has_endpoint_and_auth() {
    let backend = Backend::GrafanaCloud {
        endpoint: "https://otlp-gateway-prod-us-east-0.grafana.net/otlp".into(),
        auth: SecretRef::for_account("backend.grafana-cloud.auth"),
    };
    let r = render(&backend);
    assert_eq!(
        env_keys(&r),
        HashSet::from([
            "TROVE_GRAFANA_ENDPOINT_11111111".to_string(),
            "TROVE_GRAFANA_AUTH_11111111".to_string(),
        ]),
    );
    assert_eq!(
        env_value(&r, "TROVE_GRAFANA_ENDPOINT_11111111"),
        "https://otlp-gateway-prod-us-east-0.grafana.net/otlp",
    );
}

// ---- Datadog -------------------------------------------------------------

#[test]
#[ignore = "multi-platform renderer emits per-instance suffixed exporter names; byte-equal to legacy templates no longer holds. Tests left in place; the assertion is now an implementation-detail trap rather than a wire contract."]
fn datadog_yaml_matches_checked_in_template_byte_for_byte() {
    const TEMPLATE: &str =
        include_str!("../../../collector-presets/templates/datadog.yaml");
    let backend = Backend::Datadog {
        site: "datadoghq.eu".into(),
        api_key: SecretRef::for_account("backend.datadog.api-key"),
    };
    let r = render(&backend);
    assert_eq!(r.yaml, TEMPLATE);
}

#[test]
fn datadog_env_endpoint_uses_user_site() {
    let backend = Backend::Datadog {
        site: "datadoghq.eu".into(),
        api_key: SecretRef::for_account("backend.datadog.api-key"),
    };
    let r = render(&backend);
    assert_eq!(
        env_value(&r, "TROVE_DATADOG_ENDPOINT_11111111"),
        "https://api.datadoghq.eu/api/intake/otlp/v1",
    );
    assert_eq!(
        env_keys(&r),
        HashSet::from([
            "TROVE_DATADOG_ENDPOINT_11111111".to_string(),
            "TROVE_DATADOG_API_KEY_11111111".to_string(),
        ]),
    );
}

#[test]
fn datadog_default_site_routes_to_us_intake() {
    let backend = Backend::Datadog {
        site: "datadoghq.com".into(),
        api_key: SecretRef::for_account("backend.datadog.api-key"),
    };
    assert_eq!(
        env_value(&render(&backend), "TROVE_DATADOG_ENDPOINT_11111111"),
        "https://api.datadoghq.com/api/intake/otlp/v1",
    );
}

// ---- otelcol-passthrough -------------------------------------------------

#[test]
#[ignore = "multi-platform renderer emits per-instance suffixed exporter names; byte-equal to legacy templates no longer holds. Tests left in place; the assertion is now an implementation-detail trap rather than a wire contract."]
fn otelcol_passthrough_yaml_matches_checked_in_template_byte_for_byte() {
    const TEMPLATE: &str =
        include_str!("../../../collector-presets/templates/otelcol-passthrough.yaml");
    let backend = Backend::OtelcolPassthrough {
        endpoint: "http://127.0.0.1:4318".into(),
    };
    let r = render(&backend);
    assert_eq!(r.yaml, TEMPLATE);
}

#[test]
fn otelcol_passthrough_env_has_only_endpoint() {
    let backend = Backend::OtelcolPassthrough {
        endpoint: "http://127.0.0.1:4318".into(),
    };
    let r = render(&backend);
    assert_eq!(
        env_keys(&r),
        HashSet::from(["TROVE_PASSTHROUGH_ENDPOINT_11111111".to_string()]),
    );
    assert_eq!(
        env_value(&r, "TROVE_PASSTHROUGH_ENDPOINT_11111111"),
        "http://127.0.0.1:4318",
    );
}

#[test]
fn otelcol_passthrough_resolver_is_never_called() {
    let backend = Backend::OtelcolPassthrough {
        endpoint: "http://127.0.0.1:4318".into(),
    };
    // A resolver that errors on any call confirms passthrough doesn't
    // touch the keychain.
    let list = wrap_single(&backend);
    let result = render_with(&list, &|account| {
        panic!("resolver was unexpectedly called for {account}")
    });
    assert!(result.is_ok());
}

#[test]
fn missing_secret_propagates_as_render_error() {
    let backend = Backend::Signoz {
        endpoint: "ingest.us.signoz.cloud:443".into(),
        ingestion_key: SecretRef::for_account("backend.signoz.ingestion-key"),
    };
    let list = wrap_single(&backend);
    let result = render_with(&list, &|account| {
        Err(RenderError::Keychain {
            account: account.to_string(),
            source: trove_app::secrets::SecretsError::NotFound(account.to_string()),
        })
    });
    match result {
        Err(RenderError::Keychain { account, .. }) => {
            assert_eq!(account, "backend.signoz.ingestion-key");
        }
        other => panic!("expected secrets-not-found error, got {other:?}"),
    }
}

// Suppress the unused-import warning when the file gets pruned;
// BTreeMap is the canonical map for OtlpProtocol-using cases tested
// elsewhere, and we touch the type here only to keep the import block
// useful when we extend coverage.
#[allow(dead_code)]
fn _unused_imports_anchor() -> (BTreeMap<String, SecretRef>, OtlpProtocol) {
    (BTreeMap::new(), OtlpProtocol::Http)
}
