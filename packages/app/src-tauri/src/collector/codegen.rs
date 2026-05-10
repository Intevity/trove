//! Backend-specific `collector.yaml` codegen.
//!
//! [`render`] takes a persisted [`Backend`] and produces:
//!
//! 1. The YAML the supervisor will hand to the `trove-otelcol` child as
//!    its `--config`. For five of the six backend kinds the YAML is the
//!    matching static template under `packages/collector-presets/`,
//!    referenced via [`include_str!`] so it ships baked into the binary
//!    (no separate file the user can wedge). For OTLP-generic — the
//!    only variadic-headers kind — the YAML is generated inline so
//!    the headers block can be sized to the user's input.
//!
//! 2. A `HashMap<String, Zeroizing<String>>` of env vars the supervisor
//!    sets on the spawned child. The Collector's native `${env:VAR}`
//!    interpolation resolves these at startup. Secret values come from
//!    the OS keychain via [`crate::secrets::retrieve`] and are wrapped
//!    in [`Zeroizing`] so the in-process buffer is wiped on drop —
//!    only the child's address space holds the unwrapped value, and
//!    only after [`Command::env`](std::process::Command::env) has
//!    delivered it (never via argv).
//!
//! Pure-functional: no I/O on disk, no spawn — that lives in
//! `lifecycle.rs` and `lib.rs`. Hermetic golden-file tests in
//! `tests/codegen_*.rs` snapshot every variant.

use std::collections::HashMap;
use std::fmt::Write as _;

use thiserror::Error;
use zeroize::Zeroizing;

use crate::app_state::{Backend, OtlpProtocol};
use crate::secrets;

const SIGNOZ_TEMPLATE: &str =
    include_str!("../../../../collector-presets/templates/signoz.yaml");
const HONEYCOMB_TEMPLATE: &str =
    include_str!("../../../../collector-presets/templates/honeycomb.yaml");
const GRAFANA_CLOUD_TEMPLATE: &str =
    include_str!("../../../../collector-presets/templates/grafana-cloud.yaml");
const DATADOG_TEMPLATE: &str =
    include_str!("../../../../collector-presets/templates/datadog.yaml");
const OTELCOL_PASSTHROUGH_TEMPLATE: &str =
    include_str!("../../../../collector-presets/templates/otelcol-passthrough.yaml");

/// What [`render`] returns. The supervisor writes [`Self::yaml`] to
/// `collector.yaml` atomically and passes [`Self::env`] to
/// [`Command::env`](std::process::Command::env) on the child.
///
/// `Debug` derives a representation that prints the env keys but not
/// their values (Zeroizing's Debug impl prints `<redacted>`), so test
/// failures don't accidentally leak secret material via assertion
/// output.
#[derive(Debug)]
pub struct RenderedCollector {
    pub yaml: String,
    pub env: HashMap<String, Zeroizing<String>>,
}

/// Failures from [`render`]. Currently only keychain failures (the
/// template strings are baked in at compile time, so they cannot
/// fail to load at runtime).
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("could not retrieve keychain entry for {account}: {source}")]
    Keychain {
        account: String,
        #[source]
        source: secrets::SecretsError,
    },
}

/// Render the active `collector.yaml` and the env map the supervisor
/// must set on the spawned child. Production entry point: pulls every
/// secret from the OS keychain via [`crate::secrets::retrieve`].
pub fn render(backend: &Backend) -> Result<RenderedCollector, RenderError> {
    render_with(backend, &|account| {
        secrets::retrieve(account).map_err(|source| RenderError::Keychain {
            account: account.to_string(),
            source,
        })
    })
}

/// Test-friendly variant of [`render`] that takes an explicit resolver
/// for secret-bearing fields. The resolver is called once per
/// `SecretRef.account` referenced by the backend; whatever it returns
/// is what the env map ends up holding (typically a canned value in
/// tests, the real keychain entry in production).
pub fn render_with(
    backend: &Backend,
    resolver: &dyn Fn(&str) -> Result<Zeroizing<String>, RenderError>,
) -> Result<RenderedCollector, RenderError> {
    match backend {
        Backend::Signoz {
            endpoint,
            ingestion_key,
        } => {
            let mut env = HashMap::new();
            env.insert(
                "TROVE_SIGNOZ_ENDPOINT".to_string(),
                Zeroizing::new(endpoint.clone()),
            );
            env.insert(
                "TROVE_SIGNOZ_INGESTION_KEY".to_string(),
                resolver(&ingestion_key.account)?,
            );
            Ok(RenderedCollector {
                yaml: SIGNOZ_TEMPLATE.to_string(),
                env,
            })
        }

        Backend::Honeycomb { team, dataset } => {
            let mut env = HashMap::new();
            env.insert(
                "TROVE_HONEYCOMB_TEAM".to_string(),
                resolver(&team.account)?,
            );
            env.insert(
                "TROVE_HONEYCOMB_DATASET".to_string(),
                Zeroizing::new(dataset.clone()),
            );
            Ok(RenderedCollector {
                yaml: HONEYCOMB_TEMPLATE.to_string(),
                env,
            })
        }

        Backend::GrafanaCloud { endpoint, auth } => {
            let mut env = HashMap::new();
            env.insert(
                "TROVE_GRAFANA_ENDPOINT".to_string(),
                Zeroizing::new(endpoint.clone()),
            );
            env.insert("TROVE_GRAFANA_AUTH".to_string(), resolver(&auth.account)?);
            Ok(RenderedCollector {
                yaml: GRAFANA_CLOUD_TEMPLATE.to_string(),
                env,
            })
        }

        Backend::Datadog { site, api_key } => {
            let mut env = HashMap::new();
            env.insert(
                "TROVE_DATADOG_ENDPOINT".to_string(),
                // Datadog OTLP intake URL is per-site. Construct it
                // here rather than baking it into the template so
                // adding a new site is a one-line change in Rust
                // rather than a fanout of YAML files.
                Zeroizing::new(format!("https://api.{site}/api/intake/otlp/v1")),
            );
            env.insert(
                "TROVE_DATADOG_API_KEY".to_string(),
                resolver(&api_key.account)?,
            );
            Ok(RenderedCollector {
                yaml: DATADOG_TEMPLATE.to_string(),
                env,
            })
        }

        Backend::OtlpGeneric {
            endpoint,
            protocol,
            headers,
        } => {
            // The headers block is variadic, so we render the YAML
            // inline rather than from a static template. Each header
            // value is still routed through `${env:...}` so the secret
            // bytes never appear in the on-disk YAML.
            let mut env: HashMap<String, Zeroizing<String>> = HashMap::new();
            env.insert(
                "TROVE_OTLP_ENDPOINT".to_string(),
                Zeroizing::new(endpoint.clone()),
            );

            let mut header_lines = String::new();
            // Iterate in BTreeMap order so the YAML is byte-stable
            // across re-renders.
            for (name, secret_ref) in headers {
                let env_key = format!("TROVE_OTLP_HEADER_{}", sanitize_for_env(name));
                env.insert(env_key.clone(), resolver(&secret_ref.account)?);
                let _ = writeln!(header_lines, "      {name}: ${{env:{env_key}}}");
            }

            let yaml = render_otlp_generic_yaml(*protocol, &header_lines);
            Ok(RenderedCollector { yaml, env })
        }

        Backend::OtelcolPassthrough { endpoint } => {
            let mut env = HashMap::new();
            env.insert(
                "TROVE_PASSTHROUGH_ENDPOINT".to_string(),
                Zeroizing::new(endpoint.clone()),
            );
            Ok(RenderedCollector {
                yaml: OTELCOL_PASSTHROUGH_TEMPLATE.to_string(),
                env,
            })
        }
    }
}

/// Sanitize a header name into a POSIX-safe env var suffix:
/// uppercase, with any non-`[A-Z0-9_]` byte replaced by `_`.
fn sanitize_for_env(name: &str) -> String {
    name.chars()
        .map(|c| {
            let upper = c.to_ascii_uppercase();
            if upper.is_ascii_alphanumeric() || upper == '_' {
                upper
            } else {
                '_'
            }
        })
        .collect()
}

/// Build the `otlp-generic` YAML on the fly. The receiver/processor
/// sections are identical to the other presets; only the exporter
/// changes (gRPC vs HTTP, dynamic headers).
fn render_otlp_generic_yaml(protocol: OtlpProtocol, header_lines: &str) -> String {
    let exporter_name = match protocol {
        OtlpProtocol::Grpc => "otlp/user",
        OtlpProtocol::Http => "otlphttp/user",
    };

    let headers_block = if header_lines.is_empty() {
        String::new()
    } else {
        format!("    headers:\n{header_lines}")
    };

    format!(
        "# trove-otelcol — generic OTLP preset (rendered).\n\
         #\n\
         # Generated by trove from a user-configured Backend::OtlpGeneric.\n\
         # Headers come from the keychain via ${{env:...}}; the endpoint and\n\
         # protocol are fixed at codegen time.\n\
         \n\
         extensions:\n\
         \x20\x20health_check:\n\
         \x20\x20\x20\x20endpoint: 127.0.0.1:13133\n\
         \x20\x20\x20\x20path: /health\n\
         \n\
         receivers:\n\
         \x20\x20otlp:\n\
         \x20\x20\x20\x20protocols:\n\
         \x20\x20\x20\x20\x20\x20grpc:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20endpoint: 127.0.0.1:4317\n\
         \x20\x20\x20\x20\x20\x20http:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20endpoint: 127.0.0.1:4318\n\
         \n\
         processors:\n\
         \x20\x20batch:\n\
         \x20\x20\x20\x20timeout: 5s\n\
         \x20\x20attributes/redact:\n\
         \x20\x20\x20\x20actions:\n\
         \x20\x20\x20\x20\x20\x20- {{ key: user.prompt, action: delete }}\n\
         \x20\x20\x20\x20\x20\x20- {{ key: prompt.text, action: delete }}\n\
         \x20\x20resource/source:\n\
         \x20\x20\x20\x20attributes:\n\
         \x20\x20\x20\x20\x20\x20- {{ key: trove.source, value: otlp-generic, action: insert }}\n\
         \n\
         exporters:\n\
         \x20\x20{exporter_name}:\n\
         \x20\x20\x20\x20endpoint: ${{env:TROVE_OTLP_ENDPOINT}}\n\
         {headers_block}\
         \n\
         service:\n\
         \x20\x20extensions: [health_check]\n\
         \x20\x20pipelines:\n\
         \x20\x20\x20\x20metrics:\n\
         \x20\x20\x20\x20\x20\x20receivers: [otlp]\n\
         \x20\x20\x20\x20\x20\x20processors: [batch, attributes/redact, resource/source]\n\
         \x20\x20\x20\x20\x20\x20exporters: [{exporter_name}]\n\
         \x20\x20\x20\x20logs:\n\
         \x20\x20\x20\x20\x20\x20receivers: [otlp]\n\
         \x20\x20\x20\x20\x20\x20processors: [batch, attributes/redact, resource/source]\n\
         \x20\x20\x20\x20\x20\x20exporters: [{exporter_name}]\n\
         \x20\x20\x20\x20traces:\n\
         \x20\x20\x20\x20\x20\x20receivers: [otlp]\n\
         \x20\x20\x20\x20\x20\x20processors: [batch, attributes/redact, resource/source]\n\
         \x20\x20\x20\x20\x20\x20exporters: [{exporter_name}]\n\
         \x20\x20telemetry:\n\
         \x20\x20\x20\x20logs:\n\
         \x20\x20\x20\x20\x20\x20level: info\n\
         \x20\x20\x20\x20\x20\x20encoding: console\n\
         \x20\x20\x20\x20# Sprint 6: Prometheus endpoint for trove's metrics_tap (loopback only).\n\
         \x20\x20\x20\x20# Use the `readers` schema; collector v0.151+ rejects the legacy\n\
         \x20\x20\x20\x20# `address` scalar via its migration helper.\n\
         \x20\x20\x20\x20metrics:\n\
         \x20\x20\x20\x20\x20\x20level: basic\n\
         \x20\x20\x20\x20\x20\x20readers:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20- pull:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20exporter:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20prometheus:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20host: '127.0.0.1'\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20port: 8888\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_uppercases_and_replaces_dashes() {
        assert_eq!(sanitize_for_env("x-honeycomb-team"), "X_HONEYCOMB_TEAM");
        assert_eq!(sanitize_for_env("Authorization"), "AUTHORIZATION");
        assert_eq!(sanitize_for_env("trace.id"), "TRACE_ID");
        assert_eq!(sanitize_for_env("api/key"), "API_KEY");
    }

    #[test]
    fn sanitize_passes_through_safe_chars() {
        assert_eq!(sanitize_for_env("X_TOKEN_123"), "X_TOKEN_123");
    }

    #[test]
    fn passthrough_yaml_template_contains_env_var_marker() {
        assert!(OTELCOL_PASSTHROUGH_TEMPLATE.contains("${env:TROVE_PASSTHROUGH_ENDPOINT}"));
        assert!(!OTELCOL_PASSTHROUGH_TEMPLATE.contains("INGESTION_KEY"));
    }

    #[test]
    fn signoz_template_includes_ingestion_key_env_var() {
        assert!(SIGNOZ_TEMPLATE.contains("${env:TROVE_SIGNOZ_INGESTION_KEY}"));
        assert!(SIGNOZ_TEMPLATE.contains("${env:TROVE_SIGNOZ_ENDPOINT}"));
    }
}
