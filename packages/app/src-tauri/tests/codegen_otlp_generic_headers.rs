//! Variadic-headers tests for the OTLP-generic preset.
//!
//! OTLP-generic is the only backend whose YAML is rendered inline
//! (the headers block is sized to the user's input rather than a
//! checked-in static template). These tests cover:
//!
//! - one env var per header, sanitized header names → env-var names
//! - BTreeMap-stable iteration so the YAML is byte-identical across
//!   re-renders (otelcol won't otherwise reload cleanly)
//! - HTTP vs gRPC produce the matching exporter component name
//! - an empty headers map produces a valid no-headers exporter

use std::collections::BTreeMap;

use trove_app::app_state::{Backend, BackendInstance, OtlpProtocol, SecretRef};
use trove_app::collector::codegen::{RenderError, RenderedCollector, render_with};
use zeroize::Zeroizing;

// The Result wrap is required by `render_with`'s closure type even
// though this resolver never errors.
#[allow(clippy::unnecessary_wraps)]
fn echo(account: &str) -> Result<Zeroizing<String>, RenderError> {
    Ok(Zeroizing::new(format!("test-secret-for[{account}]")))
}

/// Wrap a single `Backend` in the one-element `BackendInstance` list
/// the multi-platform renderer accepts. Fixed UUID id keeps env-var /
/// exporter-name suffixes deterministic across tests.
fn wrap_single(backend: &Backend) -> Vec<BackendInstance> {
    vec![BackendInstance {
        id: "11111111-2222-3333-4444-555566667777".to_string(),
        label: None,
        enabled: true,
        backend: backend.clone(),
    }]
}

fn render(backend: &Backend) -> RenderedCollector {
    let list = wrap_single(backend);
    render_with(&list, &echo).expect("render should succeed with canned resolver")
}

fn header(name: &str) -> SecretRef {
    SecretRef::for_account(format!("backend.otlp-generic.header.{name}"))
}

#[test]
fn http_exporter_uses_otlphttp_user_component() {
    let backend = Backend::OtlpGeneric {
        endpoint: "https://otel.example.com".into(),
        protocol: OtlpProtocol::Http,
        headers: BTreeMap::new(),
    };
    let r = render(&backend);
    assert!(
        r.yaml.contains("otlphttp/user-11111111:"),
        "yaml missing otlphttp/user exporter:\n{}",
        r.yaml
    );
    assert!(!r.yaml.contains("otlp/user-11111111:")); // no gRPC variant
}

#[test]
fn grpc_exporter_uses_otlp_user_component() {
    let backend = Backend::OtlpGeneric {
        endpoint: "https://otel.example.com".into(),
        protocol: OtlpProtocol::Grpc,
        headers: BTreeMap::new(),
    };
    let r = render(&backend);
    assert!(
        r.yaml.contains("otlp/user-11111111:"),
        "yaml missing otlp/user exporter:\n{}",
        r.yaml
    );
    assert!(!r.yaml.contains("otlphttp/user-11111111:"));
}

#[test]
fn empty_headers_map_yields_one_env_var_for_endpoint_only() {
    let backend = Backend::OtlpGeneric {
        endpoint: "https://otel.example.com".into(),
        protocol: OtlpProtocol::Http,
        headers: BTreeMap::new(),
    };
    let r = render(&backend);
    assert_eq!(r.env.len(), 1);
    assert!(r.env.contains_key("TROVE_OTLP_ENDPOINT_11111111"));
    // No `headers:` block emitted when the map is empty.
    assert!(!r.yaml.contains("    headers:"));
}

#[test]
fn each_header_becomes_one_env_var_with_sanitized_name() {
    let mut headers = BTreeMap::new();
    headers.insert("x-api-key".to_string(), header("x-api-key"));
    headers.insert("X-Tenant-ID".to_string(), header("X-Tenant-ID"));
    headers.insert("Authorization".to_string(), header("Authorization"));

    let backend = Backend::OtlpGeneric {
        endpoint: "https://otel.example.com".into(),
        protocol: OtlpProtocol::Http,
        headers,
    };
    let r = render(&backend);

    // 3 headers + 1 endpoint = 4 env vars.
    assert_eq!(r.env.len(), 4);
    assert!(r.env.contains_key("TROVE_OTLP_HEADER_X_API_KEY_11111111"));
    assert!(r.env.contains_key("TROVE_OTLP_HEADER_X_TENANT_ID_11111111"));
    assert!(r.env.contains_key("TROVE_OTLP_HEADER_AUTHORIZATION_11111111"));
}

#[test]
fn header_lines_are_sorted_for_byte_stable_yaml() {
    // Insert in a non-sorted order; BTreeMap re-sorts; the YAML must
    // match the sorted order so two equivalent saves produce the same
    // file (otelcol's reload semantics get confused otherwise).
    let mut headers = BTreeMap::new();
    headers.insert("z-last".to_string(), header("z-last"));
    headers.insert("a-first".to_string(), header("a-first"));
    headers.insert("m-middle".to_string(), header("m-middle"));

    let backend = Backend::OtlpGeneric {
        endpoint: "https://otel.example.com".into(),
        protocol: OtlpProtocol::Http,
        headers,
    };
    let r = render(&backend);

    let a = r.yaml.find("a-first:").unwrap();
    let m = r.yaml.find("m-middle:").unwrap();
    let z = r.yaml.find("z-last:").unwrap();
    assert!(a < m);
    assert!(m < z);
}

#[test]
fn rendered_yaml_does_not_contain_secret_values() {
    // The `echo` resolver returns "test-secret-for[<account>]" — the
    // canary value must not leak into the YAML, only into the env map.
    let mut headers = BTreeMap::new();
    headers.insert("x-api-key".to_string(), header("x-api-key"));

    let backend = Backend::OtlpGeneric {
        endpoint: "https://otel.example.com".into(),
        protocol: OtlpProtocol::Http,
        headers,
    };
    let r = render(&backend);

    assert!(
        !r.yaml.contains("test-secret-for"),
        "secret value leaked into yaml:\n{}",
        r.yaml,
    );
    // Header references the env var, not the inlined value.
    assert!(r.yaml.contains("x-api-key: ${env:TROVE_OTLP_HEADER_X_API_KEY_11111111}"));
}

#[test]
fn rendering_twice_produces_byte_identical_yaml() {
    let mut headers = BTreeMap::new();
    headers.insert("x-api-key".to_string(), header("x-api-key"));
    headers.insert("x-trace-id".to_string(), header("x-trace-id"));

    let backend = Backend::OtlpGeneric {
        endpoint: "https://otel.example.com".into(),
        protocol: OtlpProtocol::Http,
        headers,
    };
    let r1 = render(&backend);
    let r2 = render(&backend);
    assert_eq!(r1.yaml, r2.yaml);
}

#[test]
fn endpoint_is_passed_through_to_env_value_unchanged() {
    let backend = Backend::OtlpGeneric {
        endpoint: "https://my.collector.example.com:8443/v1".into(),
        protocol: OtlpProtocol::Http,
        headers: BTreeMap::new(),
    };
    let r = render(&backend);
    assert_eq!(
        r.env.get("TROVE_OTLP_ENDPOINT_11111111").unwrap().to_string(),
        "https://my.collector.example.com:8443/v1",
    );
}
