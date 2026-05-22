//! Multi-backend `collector.yaml` codegen.
//!
//! [`render`] takes a non-empty slice of [`BackendInstance`] and produces:
//!
//! 1. The YAML the supervisor will hand to the `trove-otelcol` child as
//!    its `--config`. The shared receivers / processors / service blocks
//!    are emitted byte-stably; each backend contributes one exporter
//!    block (with an instance-id-suffixed name so two instances of the
//!    same kind cannot collide), and every pipeline (`metrics`, `logs`,
//!    `traces`) lists every exporter so each signal fans out to every
//!    configured platform.
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
//! `lifecycle.rs` and `lib.rs`. The static preset templates in
//! `packages/collector-presets/templates/` remain as documentation /
//! reference (and as the source for the synthetic-span hints UI); the
//! production YAML is always assembled inline so multi-backend fan-out
//! and per-instance env scoping work uniformly.

use std::collections::HashMap;
use std::fmt::Write as _;

use thiserror::Error;
use zeroize::Zeroizing;

use crate::app_state::{Backend, BackendInstance, OtlpProtocol};
use crate::harness::HarnessId;
use crate::identity::{Resolved, ResolvedSource};
use crate::mappings::{HarnessMapping, MappingSource, MappingState};
use crate::secrets;

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
///
/// **Precondition:** `backends` must be non-empty. Callers (lib.rs's
/// `prepare_collector_runtime`, the IPC reload paths) check the list
/// length first and fall back to [`crate::smoke_config_yaml`] when no
/// platform is configured — the `OTel` collector refuses to start if a
/// pipeline references zero exporters, so emitting an empty pipeline
/// list here would always fail at spawn.
pub fn render(backends: &[BackendInstance]) -> Result<RenderedCollector, RenderError> {
    render_with(backends, &|account| {
        secrets::retrieve(account).map_err(|source| RenderError::Keychain {
            account: account.to_string(),
            source,
        })
    })
}

/// Test-friendly variant of [`render`] that takes an explicit resolver
/// for secret-bearing fields. The resolver is called once per
/// `SecretRef.account` referenced by any backend in the list; whatever
/// it returns is what the env map ends up holding (typically a canned
/// value in tests, the real keychain entry in production).
pub fn render_with(
    backends: &[BackendInstance],
    resolver: &dyn Fn(&str) -> Result<Zeroizing<String>, RenderError>,
) -> Result<RenderedCollector, RenderError> {
    assert!(
        !backends.is_empty(),
        "render requires at least one backend; callers must check is_empty() first",
    );

    let mut env: HashMap<String, Zeroizing<String>> = HashMap::new();
    let mut exporter_yaml = String::new();
    let mut exporter_names: Vec<String> = Vec::new();

    for instance in backends {
        let suffix = env_suffix_for(&instance.id);
        let (name, block, instance_env) =
            render_exporter_block(&instance.backend, &suffix, resolver)?;
        exporter_names.push(name);
        exporter_yaml.push_str(&block);
        for (k, v) in instance_env {
            env.insert(k, v);
        }
    }

    let exporters_list = exporter_names.join(", ");
    let backend_count = backends.len();

    let yaml = format!(
        "# trove-otelcol — auto-generated multi-backend config.\n\
         #\n\
         # Forwards every signal to {backend_count} configured platform(s).\n\
         # Pipelines list every exporter name so each signal type fans out\n\
         # to every destination; ${{env:...}} placeholders are populated by\n\
         # the Rust supervisor at spawn time (keychain secrets + persisted\n\
         # non-secret fields). Trove never writes raw secrets into this file.\n\
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
         \x20\x20\x20\x20\x20\x20- {{ key: trove.source, value: trove, action: insert }}\n\
         \n\
         exporters:\n\
         {exporter_yaml}\
         \n\
         service:\n\
         \x20\x20extensions: [health_check]\n\
         \x20\x20pipelines:\n\
         \x20\x20\x20\x20metrics:\n\
         \x20\x20\x20\x20\x20\x20receivers: [otlp]\n\
         \x20\x20\x20\x20\x20\x20processors: [batch, attributes/redact, resource/source]\n\
         \x20\x20\x20\x20\x20\x20exporters: [{exporters_list}]\n\
         \x20\x20\x20\x20logs:\n\
         \x20\x20\x20\x20\x20\x20receivers: [otlp]\n\
         \x20\x20\x20\x20\x20\x20processors: [batch, attributes/redact, resource/source]\n\
         \x20\x20\x20\x20\x20\x20exporters: [{exporters_list}]\n\
         \x20\x20\x20\x20traces:\n\
         \x20\x20\x20\x20\x20\x20receivers: [otlp]\n\
         \x20\x20\x20\x20\x20\x20processors: [batch, attributes/redact, resource/source]\n\
         \x20\x20\x20\x20\x20\x20exporters: [{exporters_list}]\n\
         \x20\x20telemetry:\n\
         \x20\x20\x20\x20logs:\n\
         \x20\x20\x20\x20\x20\x20level: info\n\
         \x20\x20\x20\x20\x20\x20encoding: console\n\
         \x20\x20\x20\x20metrics:\n\
         \x20\x20\x20\x20\x20\x20level: basic\n\
         \x20\x20\x20\x20\x20\x20readers:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20- pull:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20exporter:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20prometheus:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20host: '127.0.0.1'\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20port: 18888\n",
    );

    Ok(RenderedCollector { yaml, env })
}

/// Derive the per-instance env-var / exporter-name suffix from a UUID
/// `id`. Uses the first 8 hex chars (32-bit namespace; birthday-bound
/// collision probability is ~0.001% at 100 backends), uppercased for
/// env var keys and lowercased for exporter names. Strips dashes so
/// POSIX `[A-Z_][A-Z0-9_]*` is satisfied even if the caller passed a
/// UUID with hyphens.
fn env_suffix_for(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

/// Resolve an `OTel` collector component id (e.g. `otlphttp/openobserve-93eb10f1`)
/// back to the [`BackendInstance::id`] it was derived from. Returns `None`
/// for component ids that don't follow Trove's exporter naming scheme or
/// that name an exporter not currently in `backends`.
///
/// Used by the per-backend health pipeline to attribute metrics-tap counters
/// and stderr error lines back to a specific destination row.
#[must_use] 
pub fn backend_id_from_component_id<'a>(
    component_id: &str,
    backends: &'a [BackendInstance],
) -> Option<&'a str> {
    // Trove exporter component ids are always `<kind>/<short>-<suffix>`.
    // Reject anything missing the slash so a bare suffix string from a
    // caller bug doesn't silently match.
    let (_, after_slash) = component_id.split_once('/')?;
    let suffix = after_slash.rsplit('-').next()?;
    if suffix.is_empty() || suffix.len() > 8 {
        return None;
    }
    backends
        .iter()
        .find(|b| env_suffix_for(&b.id) == suffix.to_ascii_lowercase())
        .map(|b| b.id.as_str())
}

/// `(exporter_name, yaml_block, env_map)` — the tuple
/// [`render_exporter_block`] returns. Pulled into a `type` to keep
/// clippy's `type_complexity` quiet.
type ExporterBlock = (String, String, HashMap<String, Zeroizing<String>>);

/// Render the exporter YAML block + env-var contributions for a single
/// backend instance. Returns `(exporter_name, yaml_block, env_map)`
/// where `exporter_name` is the unique identifier used in the
/// pipelines' `exporters: [...]` list, `yaml_block` is the chunk
/// inserted under the top-level `exporters:` map (pre-indented), and
/// `env_map` carries every `${env:...}` placeholder the block
/// references.
#[allow(clippy::too_many_lines)]
fn render_exporter_block(
    backend: &Backend,
    suffix: &str,
    resolver: &dyn Fn(&str) -> Result<Zeroizing<String>, RenderError>,
) -> Result<ExporterBlock, RenderError> {
    let env_suffix_upper = suffix.to_ascii_uppercase();
    let suffix_lower = suffix.to_ascii_lowercase();
    let mut env: HashMap<String, Zeroizing<String>> = HashMap::new();

    let (name, block) = match backend {
        Backend::Signoz {
            endpoint,
            ingestion_key,
        } => {
            let endpoint_var = format!("TROVE_SIGNOZ_ENDPOINT_{env_suffix_upper}");
            let key_var = format!("TROVE_SIGNOZ_INGESTION_KEY_{env_suffix_upper}");
            env.insert(endpoint_var.clone(), Zeroizing::new(endpoint.clone()));
            env.insert(key_var.clone(), resolver(&ingestion_key.account)?);
            let name = format!("otlp/signoz-{suffix_lower}");
            // SigNoz Cloud always serves TLS; a vanilla self-hosted
            // `docker compose up` of SigNoz OSS serves plaintext gRPC
            // on 4317. Auto-detect by inspecting the endpoint host —
            // loopback ⇒ insecure, anything else ⇒ TLS-on. Users with
            // an internal SigNoz behind a non-loopback hostname that
            // still serves plaintext should put a TLS-terminating
            // proxy in front; we don't expose a UI override yet.
            let insecure = endpoint_is_loopback(endpoint);
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    tls:\n      insecure: {insecure}\n    headers:\n      signoz-ingestion-key: ${{env:{key_var}}}\n",
            );
            (name, block)
        }
        Backend::Honeycomb { team, dataset } => {
            let team_var = format!("TROVE_HONEYCOMB_TEAM_{env_suffix_upper}");
            let dataset_var = format!("TROVE_HONEYCOMB_DATASET_{env_suffix_upper}");
            env.insert(team_var.clone(), resolver(&team.account)?);
            env.insert(dataset_var.clone(), Zeroizing::new(dataset.clone()));
            let name = format!("otlphttp/honeycomb-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: https://api.honeycomb.io\n    headers:\n      x-honeycomb-team: ${{env:{team_var}}}\n      x-honeycomb-dataset: ${{env:{dataset_var}}}\n",
            );
            (name, block)
        }
        Backend::GrafanaCloud { endpoint, auth } => {
            let endpoint_var = format!("TROVE_GRAFANA_ENDPOINT_{env_suffix_upper}");
            let auth_var = format!("TROVE_GRAFANA_AUTH_{env_suffix_upper}");
            env.insert(endpoint_var.clone(), Zeroizing::new(endpoint.clone()));
            env.insert(auth_var.clone(), resolver(&auth.account)?);
            let name = format!("otlphttp/grafana-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      Authorization: ${{env:{auth_var}}}\n",
            );
            (name, block)
        }
        Backend::Datadog { site, api_key } => {
            let endpoint_var = format!("TROVE_DATADOG_ENDPOINT_{env_suffix_upper}");
            let key_var = format!("TROVE_DATADOG_API_KEY_{env_suffix_upper}");
            // Datadog OTLP intake URL is per-site. Construct it here
            // rather than baking it into a template so adding a new site
            // is a one-line change.
            env.insert(
                endpoint_var.clone(),
                Zeroizing::new(format!("https://api.{site}/api/intake/otlp/v1")),
            );
            env.insert(key_var.clone(), resolver(&api_key.account)?);
            let name = format!("otlphttp/datadog-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      DD-API-KEY: ${{env:{key_var}}}\n",
            );
            (name, block)
        }
        Backend::OtlpGeneric {
            endpoint,
            protocol,
            headers,
        } => {
            let endpoint_var = format!("TROVE_OTLP_ENDPOINT_{env_suffix_upper}");
            env.insert(endpoint_var.clone(), Zeroizing::new(endpoint.clone()));
            let mut header_lines = String::new();
            // BTreeMap iteration is sorted → byte-stable YAML.
            for (header_name, secret_ref) in headers {
                let env_key = format!(
                    "TROVE_OTLP_HEADER_{}_{env_suffix_upper}",
                    sanitize_for_env(header_name),
                );
                env.insert(env_key.clone(), resolver(&secret_ref.account)?);
                let _ = writeln!(
                    header_lines,
                    "      {header_name}: ${{env:{env_key}}}",
                );
            }
            let name = match protocol {
                OtlpProtocol::Grpc => format!("otlp/user-{suffix_lower}"),
                OtlpProtocol::Http => format!("otlphttp/user-{suffix_lower}"),
            };
            let headers_block = if header_lines.is_empty() {
                String::new()
            } else {
                format!("    headers:\n{header_lines}")
            };
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n{headers_block}",
            );
            (name, block)
        }
        Backend::NewRelic {
            region,
            license_key,
        } => {
            let endpoint_var = format!("TROVE_NEW_RELIC_ENDPOINT_{env_suffix_upper}");
            let key_var = format!("TROVE_NEW_RELIC_LICENSE_KEY_{env_suffix_upper}");
            // New Relic's OTLP intake is region-keyed. 'eu' lands at the
            // Frankfurt cluster; everything else maps to the US cluster.
            let host = match region.as_str() {
                "eu" => "https://otlp.eu01.nr-data.net:4318",
                _ => "https://otlp.nr-data.net:4318",
            };
            env.insert(endpoint_var.clone(), Zeroizing::new(host.to_string()));
            env.insert(key_var.clone(), resolver(&license_key.account)?);
            let name = format!("otlphttp/new-relic-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      api-key: ${{env:{key_var}}}\n",
            );
            (name, block)
        }
        Backend::SplunkObservability {
            realm,
            access_token,
        } => {
            let endpoint_var = format!("TROVE_SPLUNK_ENDPOINT_{env_suffix_upper}");
            let token_var = format!("TROVE_SPLUNK_ACCESS_TOKEN_{env_suffix_upper}");
            env.insert(
                endpoint_var.clone(),
                Zeroizing::new(format!("https://ingest.{realm}.signalfx.com")),
            );
            env.insert(token_var.clone(), resolver(&access_token.account)?);
            let name = format!("otlphttp/splunk-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      X-SF-Token: ${{env:{token_var}}}\n",
            );
            (name, block)
        }
        Backend::Dynatrace {
            environment_url,
            api_token,
        } => {
            let endpoint_var = format!("TROVE_DYNATRACE_ENDPOINT_{env_suffix_upper}");
            let token_var = format!("TROVE_DYNATRACE_API_TOKEN_{env_suffix_upper}");
            // Dynatrace OTLP intake lives under /api/v2/otlp on the
            // environment URL. Strip a trailing slash so we don't emit
            // a double-slash path the gateway rejects.
            let base = environment_url.trim_end_matches('/');
            env.insert(
                endpoint_var.clone(),
                Zeroizing::new(format!("{base}/api/v2/otlp")),
            );
            // Dynatrace expects `Authorization: Api-Token <token>` so we
            // build the full header value at render time. The supervisor
            // stores it as a single env var so the YAML never sees the
            // raw token.
            let token = resolver(&api_token.account)?;
            env.insert(
                token_var.clone(),
                Zeroizing::new(format!("Api-Token {}", token.as_str())),
            );
            let name = format!("otlphttp/dynatrace-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      Authorization: ${{env:{token_var}}}\n",
            );
            (name, block)
        }
        Backend::Elastic { endpoint, api_key } => {
            let endpoint_var = format!("TROVE_ELASTIC_ENDPOINT_{env_suffix_upper}");
            let key_var = format!("TROVE_ELASTIC_API_KEY_{env_suffix_upper}");
            env.insert(endpoint_var.clone(), Zeroizing::new(endpoint.clone()));
            // Elastic APM Server expects `Authorization: ApiKey <key>`.
            let key = resolver(&api_key.account)?;
            env.insert(
                key_var.clone(),
                Zeroizing::new(format!("ApiKey {}", key.as_str())),
            );
            let name = format!("otlphttp/elastic-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      Authorization: ${{env:{key_var}}}\n",
            );
            (name, block)
        }
        Backend::Opensearch {
            endpoint,
            authorization,
        } => {
            let endpoint_var = format!("TROVE_OPENSEARCH_ENDPOINT_{env_suffix_upper}");
            let auth_var = format!("TROVE_OPENSEARCH_AUTH_{env_suffix_upper}");
            env.insert(endpoint_var.clone(), Zeroizing::new(endpoint.clone()));
            env.insert(auth_var.clone(), resolver(&authorization.account)?);
            let name = format!("otlphttp/opensearch-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      Authorization: ${{env:{auth_var}}}\n",
            );
            (name, block)
        }
        Backend::Openobserve {
            endpoint,
            organization,
            authorization,
        } => {
            let endpoint_var = format!("TROVE_OPENOBSERVE_ENDPOINT_{env_suffix_upper}");
            let auth_var = format!("TROVE_OPENOBSERVE_AUTH_{env_suffix_upper}");
            // OpenObserve scopes OTLP intake under /api/<org>; the user
            // supplies the base URL and we append the org segment here.
            let base = endpoint.trim_end_matches('/');
            env.insert(
                endpoint_var.clone(),
                Zeroizing::new(format!("{base}/api/{organization}")),
            );
            env.insert(auth_var.clone(), resolver(&authorization.account)?);
            let name = format!("otlphttp/openobserve-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      Authorization: ${{env:{auth_var}}}\n",
            );
            (name, block)
        }
        Backend::Clickstack {
            endpoint,
            ingestion_key,
        } => {
            let endpoint_var = format!("TROVE_CLICKSTACK_ENDPOINT_{env_suffix_upper}");
            let key_var = format!("TROVE_CLICKSTACK_INGESTION_KEY_{env_suffix_upper}");
            env.insert(endpoint_var.clone(), Zeroizing::new(endpoint.clone()));
            env.insert(key_var.clone(), resolver(&ingestion_key.account)?);
            let name = format!("otlphttp/clickstack-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      authorization: ${{env:{key_var}}}\n",
            );
            (name, block)
        }
        Backend::Chronosphere { tenant, api_token } => {
            let endpoint_var = format!("TROVE_CHRONOSPHERE_ENDPOINT_{env_suffix_upper}");
            let token_var = format!("TROVE_CHRONOSPHERE_API_TOKEN_{env_suffix_upper}");
            env.insert(
                endpoint_var.clone(),
                Zeroizing::new(format!("https://{tenant}.chronosphere.io/data/v1/otlp")),
            );
            env.insert(token_var.clone(), resolver(&api_token.account)?);
            let name = format!("otlphttp/chronosphere-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      API-Token: ${{env:{token_var}}}\n",
            );
            (name, block)
        }
        Backend::Sentry {
            endpoint,
            auth_header,
        } => {
            let endpoint_var = format!("TROVE_SENTRY_ENDPOINT_{env_suffix_upper}");
            let auth_var = format!("TROVE_SENTRY_AUTH_{env_suffix_upper}");
            env.insert(endpoint_var.clone(), Zeroizing::new(endpoint.clone()));
            env.insert(auth_var.clone(), resolver(&auth_header.account)?);
            let name = format!("otlphttp/sentry-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n    headers:\n      X-Sentry-Auth: ${{env:{auth_var}}}\n",
            );
            (name, block)
        }
        Backend::OtelcolPassthrough { endpoint } => {
            let endpoint_var = format!("TROVE_PASSTHROUGH_ENDPOINT_{env_suffix_upper}");
            env.insert(endpoint_var.clone(), Zeroizing::new(endpoint.clone()));
            let name = format!("otlphttp/passthrough-{suffix_lower}");
            let block = format!(
                "  {name}:\n    endpoint: ${{env:{endpoint_var}}}\n",
            );
            (name, block)
        }
    };

    Ok((name, block, env))
}

/// True if `endpoint`'s host segment is a loopback address — used to
/// auto-disable TLS on presets like `SigNoz` whose cloud product is
/// TLS-on but whose self-hosted OSS deploys plaintext gRPC on 4317.
///
/// Recognises:
/// - `host:port` (gRPC convention, no scheme): everything before the
///   last colon is the host.
/// - `scheme://host:port` and `scheme://host`: the part between the
///   `://` and the next `:` or `/` is the host.
/// - IPv6 in `[::1]:4317` form: bracketed segment is the host.
///
/// Loopback hosts: `localhost`, `127.0.0.1`, `::1`, `[::1]`, `0.0.0.0`.
/// Anything else — including DNS names like `signoz.internal` that
/// happen to resolve to loopback at runtime — returns false. This is
/// deliberately a syntactic check so the rendered config is
/// reproducible without doing DNS at codegen time.
fn endpoint_is_loopback(endpoint: &str) -> bool {
    let mut s = endpoint.trim();
    // Strip the scheme if present.
    if let Some(idx) = s.find("://") {
        s = &s[idx + 3..];
    }
    // Strip everything after the path (slash) if present.
    if let Some(idx) = s.find('/') {
        s = &s[..idx];
    }
    // Pull out the host. Bracketed IPv6 → contents between brackets;
    // otherwise everything before the last `:` (port separator).
    let host = if let Some(rest) = s.strip_prefix('[') {
        match rest.find(']') {
            Some(end) => &rest[..end],
            None => return false,
        }
    } else if let Some(idx) = s.rfind(':') {
        &s[..idx]
    } else {
        s
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0")
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

/// Inject an opt-in `resource/identity` processor into `yaml` when
/// `resolved` carries non-empty values. Returns the input unchanged
/// when [`crate::identity::ResolvedSource::None`] or when both name
/// and email are empty — so disabled or "no signal" passes through
/// byte-identical to the unmodified template (preserves the existing
/// golden-file invariants).
///
/// The transform is two string substitutions:
///
/// 1. Add a `resource/identity:` block under the top-level
///    `processors:` map.
/// 2. Append `, resource/identity` to every pipeline's `processors:`
///    list (the canonical `[batch, attributes/redact, resource/source]`
///    line every template uses).
///
/// Both substitutions are anchored to verbatim strings that appear
/// once at the top level (1) and exactly three times across pipelines
/// (2). A unit test pins the invariant.
#[must_use]
pub fn apply_identity_overlay(yaml: String, resolved: &Resolved) -> String {
    if matches!(resolved.source, ResolvedSource::None)
        || (resolved.name.is_empty() && resolved.email.is_empty())
    {
        return yaml;
    }

    let mut attributes = String::new();
    if !resolved.name.is_empty() {
        let _ = writeln!(
            attributes,
            "      - {{ key: user.name, value: {}, action: upsert }}",
            yaml_quote(&resolved.name)
        );
    }
    if !resolved.email.is_empty() {
        let _ = writeln!(
            attributes,
            "      - {{ key: user.email, value: {}, action: upsert }}",
            yaml_quote(&resolved.email)
        );
    }

    let processor_block = format!(
        "  resource/identity:\n    attributes:\n{attributes}",
    );

    // (1) Inject the processor block after the top-level `processors:` key.
    //     The pipeline lines use `processors: [...]` (value on same line),
    //     so this verbatim match hits only the top-level occurrence.
    let with_block = yaml.replacen(
        "processors:\n",
        &format!("processors:\n{processor_block}"),
        1,
    );

    // (2) Append the new processor to every pipeline processors list.
    with_block.replace(
        "processors: [batch, attributes/redact, resource/source]",
        "processors: [batch, attributes/redact, resource/source, resource/identity]",
    )
}

/// Format `s` as a YAML double-quoted scalar. Escapes the two
/// characters that matter inside a `"..."` scalar: backslash and
/// double-quote. Everything else round-trips verbatim, including
/// spaces and Unicode.
fn yaml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// Tier A mapping overlay
// ---------------------------------------------------------------------------

/// Candidate `service.name` values each harness reports natively. The
/// `transform/harness-tag` processor emits one match statement per
/// candidate, back-filling `harness.id`/`harness.name` when one fires.
///
/// Listing multiple candidates per harness defends against minor
/// upstream rebranding (e.g. Gemini CLI shipping `service.name=gemini`
/// in older versions and `gemini-cli` in newer ones). The collector's
/// `set` is idempotent, so multiple matches for the same resource are
/// safe — they re-set the same value.
///
/// Adapter-injected `OTEL_RESOURCE_ATTRIBUTES` (Claude Code only) wins
/// when present; otherwise the upstream default lands here.
fn native_service_name_candidates(id: HarnessId) -> &'static [&'static str] {
    match id {
        // Claude Code sets `harness.id` directly via OTEL_RESOURCE_ATTRIBUTES,
        // but tagging is still safe (idempotent: same value either way).
        // Match on the wire-format id plus the unprefixed namespace.
        HarnessId::ClaudeCode => &["claude-code", "claude"],
        HarnessId::GeminiCli => &["gemini-cli", "gemini"],
        HarnessId::QwenCode => &["qwen-code", "qwen"],
        // Codex's Rust backend emits with `service.name=codex-app-server`
        // (the binary name of the `codex app-server` process — same
        // backend whether invoked by the CLI or by the desktop app).
        // Trove deliberately routes both the codex-cli and codex-desktop
        // rows through this single tag rather than minting separate
        // ones; the codex-desktop arm in `native_service_name_candidates`
        // returns an empty slice for exactly this reason. Users with
        // shell env overrides may also see `codex-cli` or `codex`, so
        // accept all three. Codex 0.130's `codex exec` subcommand emits
        // `service.name=codex_exec` (snake_case binary name) — added
        // 2026-05-22 after a matrix run found codex exec spans landing
        // untagged.
        HarnessId::CodexCli => &["codex-app-server", "codex-cli", "codex", "codex_exec"],
        HarnessId::Opencode => &["opencode"],
        // Claude Desktop (Cowork) sends OTLP logs whose resource carries
        // `service.name=claude-cowork` or `claude-desktop`. Tag both so
        // future renames don't silently drop `harness.id` assignment.
        HarnessId::ClaudeDesktop => &["claude-desktop", "claude-cowork"],
        // Wrapper-emitted OTLP already carries `harness.id` (the adapters
        // set it directly when crafting the payload), but listing the
        // service.name here lets `transform/harness-tag` re-stamp it
        // defensively — important when a child process inherits another
        // harness's `OTEL_RESOURCE_ATTRIBUTES` env (e.g. Claude Code
        // exports `harness.id=claude-code` and codex inherits it). The
        // transform's `set` is idempotent, so re-tagging from the
        // wrapper's own `service.name` is always safe.
        HarnessId::CursorCli => &["cursor-cli"],
        HarnessId::Aider => &["aider"],
        HarnessId::CopilotCli => &["copilot-cli"],
        // Detection-only harnesses and hook-only emitters have no native
        // OTEL emission of their own (cursor-ide hooks into Cursor's
        // process; cline is detection-only) — no service.name candidates.
        HarnessId::CursorIde
        | HarnessId::Cline
        // codex-desktop shares ~/.codex/config.toml with codex-cli and
        // drives the same Rust backend, so it has no distinct native
        // service.name to claim — the existing codex-cli arm above
        // catches both `codex-cli` and `codex` resource tags.
        | HarnessId::CodexDesktop
        | HarnessId::JunieCli
        | HarnessId::Droid
        | HarnessId::KimiCodeCli
        | HarnessId::Devin
        | HarnessId::Forgecode => &[],
    }
}

/// Sanitize a `HarnessId` into the suffix used by `transform/tierA-*`
/// and `metricstransform/tierA-*` processor names. The collector
/// accepts `/` as the segment separator; the suffix mirrors the
/// kebab-case wire format of [`HarnessId`].
#[must_use]
pub fn harness_id_suffix(id: HarnessId) -> &'static str {
    match id {
        HarnessId::ClaudeCode => "claude-code",
        HarnessId::GeminiCli => "gemini-cli",
        HarnessId::CodexCli => "codex-cli",
        HarnessId::CodexDesktop => "codex-desktop",
        HarnessId::QwenCode => "qwen-code",
        HarnessId::Opencode => "opencode",
        HarnessId::ClaudeDesktop => "claude-desktop",
        // Not used today (hook/watcher harnesses emit Tier A inline)
        // but provided for symmetry should a hybrid emerge.
        HarnessId::CursorIde => "cursor-ide",
        HarnessId::CursorCli => "cursor-cli",
        HarnessId::Cline => "cline",
        HarnessId::Aider => "aider",
        HarnessId::CopilotCli => "copilot-cli",
        // Detection-only — suffix is informational; no mapping overlay
        // emits processors keyed off these today.
        HarnessId::JunieCli => "junie-cli",
        HarnessId::Droid => "droid",
        HarnessId::KimiCodeCli => "kimi-code-cli",
        HarnessId::Devin => "devin",
        HarnessId::Forgecode => "forgecode",
    }
}

/// Inject Tier A mapping processors into `yaml` based on `mappings`.
/// Adds:
///
/// 1. `transform/harness-tag` — for each native-OTel harness with at
///    least one `synthesize-from-native` row, tag the resource's
///    `harness.id`/`harness.name` from its `service.name`. This is a
///    no-op when the adapter already injected the attributes (idempotent
///    `set`). The processor declares `metric_statements`,
///    `log_statements`, and `trace_statements` so the harness-tag is
///    safe to slot into all three pipelines.
///
/// 2. `metricstransform/tierA-<harness>` — one block per harness with
///    enabled synthesis rows. Uses `action: insert` so the native (Tier B)
///    metric stays on the wire — synthesis is additive, never destructive.
///    Per-attribute `update_label` operations rewrite raw attribute keys
///    onto the Tier A schema (e.g. `gen_ai.token.type` → `direction`).
///    Slotted into the **metrics pipeline only** — `metricstransform`
///    declares no log/trace support and the collector rejects pipelines
///    that include it for an unsupported signal type.
///
/// 3. Pipeline updates: rewrites each pipeline's `processors:` line
///    independently. Handles both the baseline template line and the
///    post-identity-overlay variant, so this overlay can run before or
///    after [`apply_identity_overlay`].
///
/// Returns the input unchanged when no harness has enabled
/// `synthesize-from-native` rows (the typical hook-only configuration).
#[must_use]
pub fn apply_mapping_overlay(yaml: String, mappings: &MappingState) -> String {
    // Anything with a native service.name (Tier-1 emitters) needs a
    // harness-tag entry, even if it has no SynthesizeFromNative rows.
    // Otherwise records from e.g. Claude Desktop arrive at the exporter
    // without `harness.id` set and don't filter on the dashboard.
    let tag_harnesses: Vec<&HarnessMapping> = mappings
        .all_harnesses()
        .iter()
        .copied()
        .filter(|h| h.enabled && !native_service_name_candidates(h.harness_id).is_empty())
        .collect();
    let synth_harnesses: Vec<&HarnessMapping> = mappings.native_synthesis_harnesses();
    if tag_harnesses.is_empty() && synth_harnesses.is_empty() {
        return yaml;
    }

    let tag_block = build_harness_tag_block(&tag_harnesses);
    let tier_a_blocks: Vec<(String, String)> = synth_harnesses
        .iter()
        .filter_map(|h| {
            let block = build_tier_a_block(h, mappings)?;
            let name = format!("metricstransform/tierA-{}", harness_id_suffix(h.harness_id));
            Some((name, block))
        })
        .collect();

    // Inject the processor definitions under the top-level `processors:`
    // map. The same verbatim anchor the identity overlay uses appears at
    // most once at the top level of every preset template.
    let mut definitions = String::new();
    definitions.push_str(&tag_block);
    for (_, block) in &tier_a_blocks {
        definitions.push_str(block);
    }
    let with_blocks = yaml.replacen(
        "processors:\n",
        &format!("processors:\n{definitions}"),
        1,
    );

    // Per-pipeline rewrite: metrics gets the full processor list
    // (harness-tag + every metricstransform/tierA-*); logs and traces
    // get only harness-tag (metricstransform doesn't support those
    // signal types — the collector exits 1 if we add it there).
    let tier_a_names: Vec<&str> = tier_a_blocks.iter().map(|(n, _)| n.as_str()).collect();
    let metrics_inserts = if tier_a_names.is_empty() {
        "transform/harness-tag".to_string()
    } else {
        format!("transform/harness-tag, {}", tier_a_names.join(", "))
    };
    let other_inserts = "transform/harness-tag".to_string();

    let metrics_updated = replace_pipeline_processors(&with_blocks, "metrics", &metrics_inserts);
    let logs_updated = replace_pipeline_processors(&metrics_updated, "logs", &other_inserts);
    let pipelines_updated = replace_pipeline_processors(&logs_updated, "traces", &other_inserts);

    // For each Tier-1 native emitter without a local adapter, inject a
    // diagnostics pipeline that filters logs down to records carrying
    // the harness's known `service.name` and sinks them to the debug
    // exporter. The pipeline's per-processor outgoing-items counter
    // gives the UI a clean per-harness "telemetry observed" signal that
    // the global receiver counter can't provide (it aggregates across
    // every harness, including Trove's own watchers).
    let with_diag = apply_diag_pipelines(pipelines_updated, &tag_harnesses);

    // If Claude Desktop is enabled, also wire the signaltometrics
    // connector so its log records produce Tier A metrics that hit the
    // same backend exporters every other harness uses.
    apply_cowork_logs_to_metrics(with_diag, &tag_harnesses)
}

/// Harnesses we want diagnostic pipelines for. Originally just
/// `ClaudeDesktop` (for the telemetry-pill overlay); now broadened to
/// every native-OTel emitter so the dashboard `FlowChart` can derive
/// per-harness rates for spans, metrics, and logs. Watcher-emitter
/// harnesses (Cline / Aider / Copilot / Cursor) are skipped — they
/// have no native `service.name` candidates so the filter has nothing
/// to match against.
fn diag_harnesses<'a>(tag_harnesses: &'a [&'a HarnessMapping]) -> Vec<&'a HarnessMapping> {
    tag_harnesses
        .iter()
        .copied()
        .filter(|h| !native_service_name_candidates(h.harness_id).is_empty())
        .collect()
}

/// When Claude Desktop is enabled, inject the `signaltometrics/cowork`
/// connector + the pipeline plumbing that bridges its Cowork log
/// records into the Tier A metric surface.
///
/// Pattern: signaltometrics is listed as an exporter on the logs
/// pipeline (so it consumes every log record) and as a receiver on
/// the metrics pipeline (so the synthesised counters/histograms flow
/// through every existing backend exporter, picking up `harness.id`
/// tagging and Trove's resource attributes along the way).
///
/// The OTTL conditions in the connector restrict the synthesis to
/// Cowork records only — every other harness's logs pass through
/// untouched and don't trigger the connector.
///
/// Field-name assumptions, per <https://claude.com/docs/cowork/monitoring>
/// (attribute list documented; event-name carrier is not — we match on
/// both `body` and `attributes["event.name"]` to be resilient):
///   `api_request`: `input_tokens`, `output_tokens`, `cost_usd`,
///                `duration_ms`, `model`
///   `api_error`:   `status_code`, `error`, `model`
///   `tool_result`: `tool_name`, `duration_ms`, `success`
/// YAML chunk inserted at top-level when Claude Desktop is enabled.
/// Defines the `signaltometrics/cowork` connector that translates the
/// five documented Cowork event types into Tier A metric specifications.
const COWORK_CONNECTOR_BLOCK: &str = r#"
connectors:
  signaltometrics/cowork:
    logs:
      # tool_result → trove.harness.events{event.kind=tool.call}
      - name: trove.harness.events
        description: Tier A event count (Cowork tool_result).
        conditions:
          - 'body == "tool_result" or attributes["event.name"] == "tool_result"'
        attributes:
          - key: event.kind
            default_value: tool.call
        sum:
          value: "1"

      # api_request → trove.harness.events{event.kind=chat.turn}
      - name: trove.harness.events
        description: Tier A event count (Cowork api_request).
        conditions:
          - 'body == "api_request" or attributes["event.name"] == "api_request"'
        attributes:
          - key: event.kind
            default_value: chat.turn
        sum:
          value: "1"

      # api_request → trove.harness.tokens{direction=input}
      - name: trove.harness.tokens
        description: Tier A token count, input direction.
        conditions:
          - 'body == "api_request" or attributes["event.name"] == "api_request"'
        attributes:
          - key: direction
            default_value: input
          - key: model
        sum:
          value: 'attributes["input_tokens"]'

      # api_request → trove.harness.tokens{direction=output}
      - name: trove.harness.tokens
        description: Tier A token count, output direction.
        conditions:
          - 'body == "api_request" or attributes["event.name"] == "api_request"'
        attributes:
          - key: direction
            default_value: output
          - key: model
        sum:
          value: 'attributes["output_tokens"]'

      # api_request → trove.harness.cost.usd
      - name: trove.harness.cost.usd
        description: Tier A model spend (USD), exact figure from Cowork.
        conditions:
          - 'body == "api_request" or attributes["event.name"] == "api_request"'
        attributes:
          - key: cost.method
            default_value: exact
          - key: model
        sum:
          value: 'attributes["cost_usd"]'

      # api_request → trove.harness.turn.duration (ms histogram)
      - name: trove.harness.turn.duration
        description: Tier A chat-turn duration histogram (milliseconds).
        unit: ms
        conditions:
          - 'body == "api_request" or attributes["event.name"] == "api_request"'
        attributes:
          - key: event.kind
            default_value: chat.turn
          - key: model
        histogram:
          value: 'attributes["duration_ms"]'

      # api_error → trove.harness.errors
      - name: trove.harness.errors
        description: Tier A error count (Cowork api_error).
        conditions:
          - 'body == "api_error" or attributes["event.name"] == "api_error"'
        attributes:
          - key: error.kind
            default_value: unknown
          - key: model
        sum:
          value: "1"
"#;

fn apply_cowork_logs_to_metrics(yaml: String, tag_harnesses: &[&HarnessMapping]) -> String {
    let cowork_enabled = tag_harnesses
        .iter()
        .any(|h| h.harness_id == HarnessId::ClaudeDesktop);
    if !cowork_enabled {
        return yaml;
    }

    // Insert the connector block at the top level — append it before
    // the `service:` block so it sits alongside extensions / receivers /
    // exporters / processors. Falls back to a no-op if `service:` isn't
    // a unique top-level key (shouldn't happen for any preset).
    let with_connectors = if let Some(idx) = yaml.find("\nservice:") {
        let mut out = String::with_capacity(yaml.len() + COWORK_CONNECTOR_BLOCK.len());
        out.push_str(&yaml[..idx]);
        out.push_str(COWORK_CONNECTOR_BLOCK);
        out.push_str(&yaml[idx..]);
        out
    } else {
        return yaml;
    };

    // Append `signaltometrics/cowork` as an exporter on the logs
    // pipeline and as a receiver on the metrics pipeline. Idempotent
    // string replaces against the anchors emitted by `render`.
    append_pipeline_exporter(
        &with_connectors,
        "logs",
        "signaltometrics/cowork",
    )
    .and_then(|s| append_pipeline_receiver(&s, "metrics", "signaltometrics/cowork"))
    .unwrap_or(with_connectors)
}

/// Append `entry` to the `exporters: [...]` array of the named
/// pipeline. Returns `None` if the anchor isn't found (so the caller
/// can fall back without panicking).
fn append_pipeline_exporter(yaml: &str, pipeline: &str, entry: &str) -> Option<String> {
    append_pipeline_list(yaml, pipeline, "exporters", entry)
}

fn append_pipeline_receiver(yaml: &str, pipeline: &str, entry: &str) -> Option<String> {
    append_pipeline_list(yaml, pipeline, "receivers", entry)
}

fn append_pipeline_list(
    yaml: &str,
    pipeline: &str,
    list_kind: &str,
    entry: &str,
) -> Option<String> {
    // Find the pipeline's named block inside `service.pipelines`. The
    // `pipelines:` key sits at 2-space indent; each pipeline name at
    // 4-space. Scope the search to that section so we don't mis-anchor
    // on a `logs:` key buried inside a processor config (e.g. the
    // filter/diag-claude-desktop block uses `logs:` for signal-typing).
    let pipelines_section_start = yaml.find("\n  pipelines:\n")?;
    let pipelines_offset = pipelines_section_start + "\n  pipelines:\n".len();
    let pipeline_anchor = format!("    {pipeline}:\n");
    let rel = yaml[pipelines_offset..].find(&pipeline_anchor)?;
    let pipeline_header_idx = pipelines_offset + rel;
    let pipeline_body_start = pipeline_header_idx + pipeline_anchor.len();
    let pipeline_body = &yaml[pipeline_body_start..];

    // Stop at the next top-level pipeline ("    <name>:") or block
    // ("  ", e.g. `  telemetry:` at 2-space indent) so we don't
    // accidentally edit the wrong section.
    let mut search_end = pipeline_body.len();
    for (i, line) in pipeline_body.split('\n').enumerate() {
        if i == 0 {
            continue;
        }
        if !line.starts_with("      ") && !line.is_empty() {
            search_end = pipeline_body
                .split('\n')
                .take(i)
                .map(|l| l.len() + 1)
                .sum::<usize>()
                .saturating_sub(1);
            break;
        }
    }
    let pipeline_section = &pipeline_body[..search_end];
    let list_prefix = format!("      {list_kind}: [");
    let list_start_rel = pipeline_section.find(&list_prefix)?;
    let list_open_abs = pipeline_body_start + list_start_rel + list_prefix.len() - 1;
    let close_rel = yaml[list_open_abs..].find(']')?;
    let close_abs = list_open_abs + close_rel;
    let inner = yaml[list_open_abs + 1..close_abs].trim();
    let new_inner = if inner.is_empty() {
        entry.to_string()
    } else if inner.split(',').any(|s| s.trim() == entry) {
        // Already present — idempotent.
        return Some(yaml.to_string());
    } else {
        format!("{inner}, {entry}")
    };
    let mut out = String::with_capacity(yaml.len() + entry.len() + 2);
    out.push_str(&yaml[..=list_open_abs]);
    out.push_str(&new_inner);
    out.push_str(&yaml[close_abs..]);
    Some(out)
}

fn apply_diag_pipelines(yaml: String, tag_harnesses: &[&HarnessMapping]) -> String {
    let diag = diag_harnesses(tag_harnesses);
    if diag.is_empty() {
        return yaml;
    }

    // Each filter processor drops records whose resource service.name
    // doesn't match the harness's known candidates — the `outgoing_items`
    // counter on the processor then equals the per-harness count for
    // that signal type. One processor block carries `traces.span`,
    // `metrics.datapoint`, and `logs.log_record` rules so the same
    // processor can be plugged into all three diagnostic pipelines.
    let mut processor_defs = String::new();
    let mut pipeline_entries = String::new();
    for h in &diag {
        let suffix = harness_id_suffix(h.harness_id);
        let names = native_service_name_candidates(h.harness_id);
        let drop_clause = names
            .iter()
            .map(|n| format!("resource.attributes[\"service.name\"] != \"{n}\""))
            .collect::<Vec<_>>()
            .join(" and ");

        let _ = writeln!(processor_defs, "  filter/diag-{suffix}:");
        let _ = writeln!(processor_defs, "    error_mode: ignore");
        let _ = writeln!(processor_defs, "    traces:");
        let _ = writeln!(processor_defs, "      span:");
        let _ = writeln!(processor_defs, "        - '{drop_clause}'");
        let _ = writeln!(processor_defs, "    metrics:");
        let _ = writeln!(processor_defs, "      datapoint:");
        let _ = writeln!(processor_defs, "        - '{drop_clause}'");
        let _ = writeln!(processor_defs, "    logs:");
        let _ = writeln!(processor_defs, "      log_record:");
        let _ = writeln!(processor_defs, "        - '{drop_clause}'");

        let _ = writeln!(pipeline_entries, "    traces/diag-{suffix}:");
        let _ = writeln!(pipeline_entries, "      receivers: [otlp]");
        let _ = writeln!(pipeline_entries, "      processors: [filter/diag-{suffix}]");
        let _ = writeln!(pipeline_entries, "      exporters: [debug/diag-noop]");
        let _ = writeln!(pipeline_entries, "    metrics/diag-{suffix}:");
        let _ = writeln!(pipeline_entries, "      receivers: [otlp]");
        let _ = writeln!(pipeline_entries, "      processors: [filter/diag-{suffix}]");
        let _ = writeln!(pipeline_entries, "      exporters: [debug/diag-noop]");
        let _ = writeln!(pipeline_entries, "    logs/diag-{suffix}:");
        let _ = writeln!(pipeline_entries, "      receivers: [otlp]");
        let _ = writeln!(pipeline_entries, "      processors: [filter/diag-{suffix}]");
        let _ = writeln!(pipeline_entries, "      exporters: [debug/diag-noop]");
    }

    let exporter_def = "  debug/diag-noop:\n    verbosity: basic\n";

    yaml.replacen(
        "processors:\n",
        &format!("processors:\n{processor_defs}"),
        1,
    )
    .replacen("exporters:\n", &format!("exporters:\n{exporter_def}"), 1)
    .replacen(
        "  pipelines:\n",
        &format!("  pipelines:\n{pipeline_entries}"),
        1,
    )
}

/// Rewrite the `processors:` line of a single named pipeline. The
/// per-pipeline anchor pins us to one of `metrics`/`logs`/`traces` so
/// we never accidentally insert a metric-only processor into another
/// pipeline. Handles both the baseline (no identity overlay) and the
/// identity-augmented form, so the mapping overlay is order-independent
/// with respect to [`apply_identity_overlay`].
///
/// Returns the input unchanged when the named pipeline doesn't have
/// the canonical processor list (the smoke/passthrough template, for
/// example, may diverge — in which case the overlay declines rather
/// than corrupts).
fn replace_pipeline_processors(yaml: &str, pipeline_name: &str, inserts: &str) -> String {
    let baseline_anchor = format!(
        "    {pipeline_name}:\n      receivers: [otlp]\n      processors: [batch, attributes/redact, resource/source]"
    );
    let baseline_repl = format!(
        "    {pipeline_name}:\n      receivers: [otlp]\n      processors: [batch, attributes/redact, resource/source, {inserts}]"
    );

    let identity_anchor = format!(
        "    {pipeline_name}:\n      receivers: [otlp]\n      processors: [batch, attributes/redact, resource/source, resource/identity]"
    );
    // Inserted before `resource/identity` so identity tagging applies to
    // the synthesized Tier A metrics too.
    let identity_repl = format!(
        "    {pipeline_name}:\n      receivers: [otlp]\n      processors: [batch, attributes/redact, resource/source, {inserts}, resource/identity]"
    );

    yaml.replace(&baseline_anchor, &baseline_repl)
        .replace(&identity_anchor, &identity_repl)
}

/// Build the `transform/harness-tag` processor stanza. For each
/// harness with synthesis rows, emit a `set` statement that maps the
/// harness's native `service.name` onto `harness.id` and `harness.name`.
///
/// The statements appear under all three signal-type lists
/// (`metric_statements`, `log_statements`, `trace_statements`) so logs
/// and traces emitted alongside the native metrics also pick up
/// `harness.id` — keeps the dashboard filter consistent across signal
/// types.
fn build_harness_tag_block(synth: &[&HarnessMapping]) -> String {
    let mut statements = String::new();
    for h in synth {
        let candidates = native_service_name_candidates(h.harness_id);
        if candidates.is_empty() {
            continue;
        }
        let harness_id_str = harness_id_suffix(h.harness_id);
        for service_name in candidates {
            let _ = writeln!(
                statements,
                "          - 'set(attributes[\"harness.id\"], \"{harness_id_str}\") where attributes[\"service.name\"] == \"{service_name}\"'"
            );
            let _ = writeln!(
                statements,
                "          - 'set(attributes[\"harness.name\"], \"{}\") where attributes[\"service.name\"] == \"{service_name}\"'",
                h.harness_id.label(),
            );
        }
    }

    if statements.is_empty() {
        return String::new();
    }

    let mut block = String::new();
    let _ = writeln!(block, "  transform/harness-tag:");
    for kind in ["metric_statements", "log_statements", "trace_statements"] {
        let _ = writeln!(block, "    {kind}:");
        let _ = writeln!(block, "      - context: resource");
        let _ = writeln!(block, "        statements:");
        block.push_str(&statements);
    }
    block
}

/// Build the `metricstransform/tierA-<harness>` stanza for one harness's
/// synthesis rows. Returns `None` when the harness has no enabled
/// `synthesize-from-native` rows (caller filters with this).
///
/// `action: insert` preserves the original Tier B metric — the new
/// Tier A row is an additional metric on the wire, not a rename. This
/// honors `MAPPING_PLAN.md` §"Defaults": "All Tier B passes through;
/// synthesis is additive."
///
/// The `state` parameter is needed to resolve `target_metric` (a catalog
/// id) to its full wire name. Rows whose `target_metric` doesn't resolve
/// are skipped — validation catches these before apply, so this is a
/// belt-and-suspenders guard against stale references slipping through.
fn build_tier_a_block(harness: &HarnessMapping, state: &MappingState) -> Option<String> {
    if !harness.enabled {
        return None;
    }
    let mut transforms = String::new();
    for source in &harness.sources {
        let MappingSource::SynthesizeFromNative {
            native_metric,
            target_metric,
            attribute_map,
            inject_attributes,
        } = source
        else {
            continue;
        };
        let Some(def) = state.metric(target_metric) else {
            // Stale catalog reference — skip so we don't emit invalid
            // YAML. Validation should have caught this; if it didn't,
            // a missing transform row is the least-bad failure mode.
            continue;
        };
        let _ = writeln!(transforms, "      - include: {native_metric}");
        let _ = writeln!(transforms, "        match_type: strict");
        let _ = writeln!(transforms, "        action: insert");
        let _ = writeln!(transforms, "        new_name: {}", def.name);
        if !attribute_map.is_empty() || !inject_attributes.is_empty() {
            let _ = writeln!(transforms, "        operations:");
            // BTreeMap iteration is sorted, so YAML order is stable
            // across re-renders (golden-file friendly).
            for (raw, tier_a) in attribute_map {
                let _ = writeln!(transforms, "          - action: update_label");
                let _ = writeln!(transforms, "            label: {raw}");
                let _ = writeln!(transforms, "            new_label: {tier_a}");
            }
            for (label, value) in inject_attributes {
                let _ = writeln!(transforms, "          - action: add_label");
                let _ = writeln!(transforms, "            new_label: {label}");
                let _ = writeln!(transforms, "            new_value: {value}");
            }
        }
    }
    if transforms.is_empty() {
        return None;
    }
    let suffix = harness_id_suffix(harness.harness_id);
    let mut block = String::new();
    let _ = writeln!(block, "  metricstransform/tierA-{suffix}:");
    let _ = writeln!(block, "    transforms:");
    block.push_str(&transforms);
    Some(block)
}


#[cfg(test)]
mod tests {
    use super::*;

    use crate::app_state::{Backend, BackendInstance, OtlpProtocol, SecretRef};

    /// Stub keychain resolver: returns the account name itself wrapped
    /// in `Zeroizing<String>`. Tests don't care about the actual secret
    /// bytes; they care that the right account was queried and the right
    /// env var ended up populated.
    // The Result wrap is mandated by `render_with`'s `&dyn Fn(...) ->
    // Result<Zeroizing<String>, RenderError>` resolver type, so we keep
    // it even though this stub can't fail.
    #[allow(clippy::unnecessary_wraps)]
    fn passthrough_resolver(account: &str) -> Result<Zeroizing<String>, RenderError> {
        Ok(Zeroizing::new(account.to_string()))
    }

    fn signoz_instance(id: &str) -> BackendInstance {
        BackendInstance {
            id: id.to_string(),
            label: None,
            enabled: true,
            backend: Backend::Signoz {
                endpoint: "ingest.us.signoz.cloud:443".into(),
                ingestion_key: SecretRef::for_account(format!(
                    "backend.signoz.ingestion-key.{id}",
                )),
            },
        }
    }

    fn datadog_instance(id: &str) -> BackendInstance {
        BackendInstance {
            id: id.to_string(),
            label: None,
            enabled: true,
            backend: Backend::Datadog {
                site: "datadoghq.com".into(),
                api_key: SecretRef::for_account(format!(
                    "backend.datadog.api-key.{id}",
                )),
            },
        }
    }

    fn honeycomb_instance(id: &str) -> BackendInstance {
        BackendInstance {
            id: id.to_string(),
            label: None,
            enabled: true,
            backend: Backend::Honeycomb {
                team: SecretRef::for_account(format!("backend.honeycomb.team.{id}")),
                dataset: "prod".into(),
            },
        }
    }

    fn rendered_yaml(backends: &[BackendInstance]) -> String {
        render_with(backends, &passthrough_resolver).unwrap().yaml
    }

    // -----------------------------------------------------------------
    // sanitize_for_env helpers
    // -----------------------------------------------------------------

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

    // -----------------------------------------------------------------
    // env_suffix_for
    // -----------------------------------------------------------------

    #[test]
    fn env_suffix_takes_first_eight_alnum_chars() {
        let uuid = "7a3f4b2c-1234-5678-9abc-def012345678";
        assert_eq!(env_suffix_for(uuid), "7a3f4b2c");
    }

    #[test]
    fn env_suffix_strips_non_alnum() {
        assert_eq!(env_suffix_for("ab-cd-ef-12345"), "abcdef12");
    }

    #[test]
    fn env_suffix_falls_back_to_default_for_empty() {
        assert_eq!(env_suffix_for(""), "default");
        assert_eq!(env_suffix_for("---"), "default");
    }

    // -----------------------------------------------------------------
    // render(&[BackendInstance])
    // -----------------------------------------------------------------

    #[test]
    fn render_single_signoz_emits_one_exporter_and_three_pipelines() {
        let instances = vec![signoz_instance("7a3f4b2c-1111-2222-3333-444455556666")];
        let out = render_with(&instances, &passthrough_resolver).unwrap();
        // Exporter name uses the first 8 hex chars of the uuid.
        assert!(out.yaml.contains("otlp/signoz-7a3f4b2c:"));
        // Three pipelines all reference the same exporter list.
        let pipeline_hits = out
            .yaml
            .matches("exporters: [otlp/signoz-7a3f4b2c]")
            .count();
        assert_eq!(pipeline_hits, 3, "metrics/logs/traces each list the exporter");
        // Env contains the endpoint + ingestion-key vars with the
        // uppercased suffix.
        assert!(out.env.contains_key("TROVE_SIGNOZ_ENDPOINT_7A3F4B2C"));
        assert!(out.env.contains_key("TROVE_SIGNOZ_INGESTION_KEY_7A3F4B2C"));
    }

    #[test]
    fn render_two_same_kind_emits_two_distinct_exporters_with_fan_out() {
        let instances = vec![
            signoz_instance("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            signoz_instance("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"),
        ];
        let out = render_with(&instances, &passthrough_resolver).unwrap();
        assert!(out.yaml.contains("otlp/signoz-aaaaaaaa:"));
        assert!(out.yaml.contains("otlp/signoz-bbbbbbbb:"));
        // Both exporters appear in every pipeline's `exporters: [...]` list.
        let pipeline_hits = out
            .yaml
            .matches("exporters: [otlp/signoz-aaaaaaaa, otlp/signoz-bbbbbbbb]")
            .count();
        assert_eq!(pipeline_hits, 3);
        // Per-instance env vars don't collide.
        assert!(out.env.contains_key("TROVE_SIGNOZ_INGESTION_KEY_AAAAAAAA"));
        assert!(out.env.contains_key("TROVE_SIGNOZ_INGESTION_KEY_BBBBBBBB"));
    }

    #[test]
    fn render_mixed_kinds_fan_out_into_every_pipeline() {
        let instances = vec![
            datadog_instance("11111111-1111-1111-1111-111111111111"),
            honeycomb_instance("22222222-2222-2222-2222-222222222222"),
        ];
        let out = render_with(&instances, &passthrough_resolver).unwrap();
        assert!(out.yaml.contains("otlphttp/datadog-11111111:"));
        assert!(out.yaml.contains("otlphttp/honeycomb-22222222:"));
        let pipeline_hits = out
            .yaml
            .matches("exporters: [otlphttp/datadog-11111111, otlphttp/honeycomb-22222222]")
            .count();
        assert_eq!(pipeline_hits, 3);
        // Datadog intake URL is per-site, built in Rust.
        let endpoint_value = out
            .env
            .get("TROVE_DATADOG_ENDPOINT_11111111")
            .expect("endpoint env var");
        assert_eq!(
            endpoint_value.as_str(),
            "https://api.datadoghq.com/api/intake/otlp/v1",
        );
    }

    #[test]
    fn render_otlp_generic_emits_each_header_with_scoped_env_var() {
        use std::collections::BTreeMap;
        let mut headers = BTreeMap::new();
        headers.insert(
            "X-Auth".to_string(),
            SecretRef::for_account("backend.otlp-generic.header.X-Auth.cc11"),
        );
        let instances = vec![BackendInstance {
            id: "cc11dd22-3333-4444-5555-666677778888".into(),
            label: None,
            enabled: true,
            backend: Backend::OtlpGeneric {
                endpoint: "https://example.test/v1/otlp".into(),
                protocol: OtlpProtocol::Http,
                headers,
            },
        }];
        let out = render_with(&instances, &passthrough_resolver).unwrap();
        assert!(out.yaml.contains("otlphttp/user-cc11dd22:"));
        assert!(out.yaml.contains("X-Auth: ${env:TROVE_OTLP_HEADER_X_AUTH_CC11DD22}"));
        assert!(out.env.contains_key("TROVE_OTLP_HEADER_X_AUTH_CC11DD22"));
    }

    #[test]
    fn render_resolver_is_called_per_secret_account() {
        use std::cell::RefCell;
        let calls: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let resolver = |account: &str| {
            calls.borrow_mut().push(account.to_string());
            Ok(Zeroizing::new("redacted".to_string()))
        };
        let instances = vec![signoz_instance("11112222-3333-4444-5555-666677778888")];
        let _ = render_with(&instances, &resolver).unwrap();
        let calls = calls.into_inner();
        assert_eq!(
            calls,
            vec!["backend.signoz.ingestion-key.11112222-3333-4444-5555-666677778888"],
        );
    }

    #[test]
    fn render_output_parses_as_valid_yaml_for_every_kind() {
        let cases: Vec<(&str, Vec<BackendInstance>)> = vec![
            ("signoz", vec![signoz_instance("aaaa1111-2222-3333-4444-555566667777")]),
            ("honeycomb", vec![honeycomb_instance("bbbb1111-2222-3333-4444-555566667777")]),
            ("datadog", vec![datadog_instance("cccc1111-2222-3333-4444-555566667777")]),
            (
                "grafana-cloud",
                vec![BackendInstance {
                    id: "dddd1111-2222-3333-4444-555566667777".into(),
                    label: None,
                    enabled: true,
                    backend: Backend::GrafanaCloud {
                        endpoint: "https://otlp-gateway.grafana.net/otlp".into(),
                        auth: SecretRef::for_account("backend.grafana-cloud.auth.dddd1111"),
                    },
                }],
            ),
            (
                "otelcol-passthrough",
                vec![BackendInstance {
                    id: "eeee1111-2222-3333-4444-555566667777".into(),
                    label: None,
                    enabled: true,
                    backend: Backend::OtelcolPassthrough {
                        endpoint: "http://127.0.0.1:4318".into(),
                    },
                }],
            ),
            (
                "multi",
                vec![
                    signoz_instance("11111111-2222-3333-4444-555566667777"),
                    datadog_instance("22221111-2222-3333-4444-555566667777"),
                ],
            ),
        ];
        for (label, instances) in cases {
            let yaml = rendered_yaml(&instances);
            let parsed: Result<serde_yml::Value, _> = serde_yml::from_str(&yaml);
            assert!(
                parsed.is_ok(),
                "render produced invalid YAML for {label}: {:?}\n--- yaml ---\n{yaml}",
                parsed.err(),
            );
        }
    }

    #[test]
    #[should_panic(expected = "at least one backend")]
    fn render_panics_on_empty_list() {
        let _ = render_with(&[], &passthrough_resolver);
    }

    // -----------------------------------------------------------------
    // apply_identity_overlay (operates on the rendered YAML)
    // -----------------------------------------------------------------

    fn resolved(name: &str, email: &str) -> Resolved {
        Resolved {
            name: name.to_string(),
            email: email.to_string(),
            source: ResolvedSource::GitConfig,
        }
    }

    #[test]
    fn identity_overlay_is_a_noop_when_source_is_none() {
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let r = Resolved {
            name: String::new(),
            email: String::new(),
            source: ResolvedSource::None,
        };
        assert_eq!(apply_identity_overlay(yaml.clone(), &r), yaml);
    }

    #[test]
    fn identity_overlay_is_a_noop_when_both_values_empty() {
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let r = resolved("", "");
        assert_eq!(apply_identity_overlay(yaml.clone(), &r), yaml);
    }

    #[test]
    fn identity_overlay_injects_processor_and_appends_to_pipelines() {
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let baseline_pipeline_hits = yaml
            .matches("processors: [batch, attributes/redact, resource/source]")
            .count();
        assert_eq!(baseline_pipeline_hits, 3, "rendered yaml invariant changed");

        let r = resolved("Ada Lovelace", "ada@example.com");
        let out = apply_identity_overlay(yaml, &r);
        assert_eq!(out.matches("resource/identity:").count(), 1);
        assert!(out.contains("- { key: user.name, value: \"Ada Lovelace\", action: upsert }"));
        assert!(out.contains("- { key: user.email, value: \"ada@example.com\", action: upsert }"));
        let original_pipeline_line =
            "processors: [batch, attributes/redact, resource/source]";
        let new_pipeline_line =
            "processors: [batch, attributes/redact, resource/source, resource/identity]";
        assert_eq!(out.matches(original_pipeline_line).count(), 0);
        assert_eq!(out.matches(new_pipeline_line).count(), 3);
    }

    #[test]
    fn identity_overlay_handles_name_only_or_email_only() {
        let yaml = rendered_yaml(&[honeycomb_instance("dddd1111-2222-3333-4444-555566667777")]);
        let with_name = apply_identity_overlay(yaml.clone(), &resolved("Ada", ""));
        assert!(with_name.contains("user.name"));
        assert!(!with_name.contains("user.email"));
        let with_email = apply_identity_overlay(yaml, &resolved("", "ada@example.com"));
        assert!(with_email.contains("user.email"));
        assert!(!with_email.contains("user.name"));
    }

    #[test]
    fn identity_overlay_yaml_escapes_quotes_and_backslashes_in_values() {
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let r = resolved("o\"\\dd", "ada@example.com");
        let out = apply_identity_overlay(yaml, &r);
        assert!(out.contains(r#"value: "o\"\\dd", action: upsert"#));
    }

    // -----------------------------------------------------------------
    // apply_mapping_overlay
    // -----------------------------------------------------------------

    #[test]
    fn mapping_overlay_is_a_noop_for_empty_mapping_state() {
        let empty = MappingState {
            schema_version: crate::mappings::MAPPING_SCHEMA_VERSION,
            metrics: vec![],
            harnesses: vec![],
        };
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        assert_eq!(apply_mapping_overlay(yaml.clone(), &empty), yaml);
    }

    #[test]
    fn mapping_overlay_emits_only_tag_block_for_hook_only_native_emitters() {
        // Tier-1 hook-only harnesses (e.g. Claude Desktop) ship a known
        // service.name but no SynthesizeFromNative rows. The overlay
        // must still emit the harness-tag block so the records arrive at
        // the exporter with `harness.id` set — but it must NOT emit any
        // `metricstransform/tierA-*` block, because there is nothing to
        // synthesize.
        let mut state = crate::mappings::default_state();
        state.harnesses.retain(|h| {
            !h.sources
                .iter()
                .any(|s| matches!(s, MappingSource::SynthesizeFromNative { .. }))
        });
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let out = apply_mapping_overlay(yaml, &state);
        assert_eq!(out.matches("transform/harness-tag:").count(), 1);
        assert!(out.contains("\"service.name\"] == \"claude-desktop\""));
        assert!(out.contains("\"service.name\"] == \"claude-cowork\""));
        assert!(!out.contains("metricstransform/tierA-"));
    }

    #[test]
    fn mapping_overlay_emits_diag_pipelines_for_every_native_emitter() {
        // FlowChart depends on per-harness diag counts to animate the
        // active harness's lane only. Diag is emitted for every enabled
        // harness with native `service.name` candidates — Claude Code,
        // Codex CLI, Gemini CLI, Qwen Code, OpenCode, and Claude Desktop,
        // plus the wrapper harnesses (cursor-cli, aider, copilot-cli)
        // whose adapters emit OTLP with a known `service.name` and which
        // were added to the candidate list 2026-05-22 for defensive
        // re-tagging (so an inherited `OTEL_RESOURCE_ATTRIBUTES` env var
        // can't mask the wrapper's true harness.id).
        // Each gets a `filter/diag-<suffix>` processor plus three
        // pipelines (traces, metrics, logs) that pipe into the shared
        // `debug/diag-noop` exporter.
        let state = crate::mappings::default_state();
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let out = apply_mapping_overlay(yaml, &state);
        assert!(out.contains("debug/diag-noop:"));
        for suffix in [
            "claude-code",
            "codex-cli",
            "gemini-cli",
            "qwen-code",
            "opencode",
            "claude-desktop",
            "cursor-cli",
            "aider",
            "copilot-cli",
        ] {
            assert!(
                out.contains(&format!("filter/diag-{suffix}:")),
                "missing filter for {suffix}"
            );
            assert!(
                out.contains(&format!("traces/diag-{suffix}:")),
                "missing traces pipeline for {suffix}"
            );
            assert!(
                out.contains(&format!("metrics/diag-{suffix}:")),
                "missing metrics pipeline for {suffix}"
            );
            assert!(
                out.contains(&format!("logs/diag-{suffix}:")),
                "missing logs pipeline for {suffix}"
            );
        }
        // Hook-only and detection-only harnesses have no native
        // `service.name` emission of their own, so they are excluded.
        for suffix in ["cursor-ide", "cline"] {
            assert!(
                !out.contains(&format!("filter/diag-{suffix}:")),
                "hook-only harness {suffix} should not get a diag filter"
            );
        }

        // Bug E regression lock — Codex 0.130's `codex exec` subcommand
        // emits `service.name=codex_exec` (snake_case). It must be in
        // codex-cli's candidate list so the transform/harness-tag rule
        // stamps `harness.id=codex-cli` and the filter/diag-codex-cli
        // pipeline retains the span. See the 2026-05-22 entry in
        // documentation/harness-platform-matrix.md.
        assert!(
            out.contains("\"service.name\"] == \"codex_exec\""),
            "codex_exec must appear in the harness-tag transform's matcher list"
        );
        assert!(
            out.contains("\"service.name\"] != \"codex_exec\""),
            "codex_exec must appear in the diag filter's drop clause for codex-cli"
        );

        // Bug G regression lock — wrapper harnesses (cursor-cli, aider,
        // copilot-cli) carry their own `service.name`, so the
        // harness-tag transform must include them as defensive
        // re-tagging in case an inherited `OTEL_RESOURCE_ATTRIBUTES` env
        // (e.g. Claude Code's) leaks through and mis-tags `harness.id`.
        for wrapper in ["cursor-cli", "aider", "copilot-cli"] {
            assert!(
                out.contains(&format!("\"service.name\"] == \"{wrapper}\"")),
                "{wrapper} must appear in the harness-tag transform's matcher list"
            );
        }
    }

    #[test]
    fn mapping_overlay_emits_cowork_signaltometrics_connector_when_claude_desktop_enabled() {
        // The signaltometrics connector translates Cowork's OTLP log
        // emission into Tier A metrics. Verify the connector block is
        // present, every Tier A metric name appears, the logs pipeline
        // exports to it, and the metrics pipeline receives from it.
        let state = crate::mappings::default_state();
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let out = apply_mapping_overlay(yaml, &state);

        // Connector definition emitted.
        assert!(out.contains("signaltometrics/cowork:"));
        for metric in [
            "trove.harness.events",
            "trove.harness.tokens",
            "trove.harness.cost.usd",
            "trove.harness.turn.duration",
            "trove.harness.errors",
        ] {
            assert!(out.contains(metric), "missing metric {metric}");
        }

        // Each of the 5 Cowork event-name conditions appears at least
        // once in the OTTL body.
        for event in ["api_request", "api_error", "tool_result"] {
            assert!(
                out.contains(&format!("\"{event}\"")),
                "missing condition reference to event {event}"
            );
        }

        // Pipeline plumbing: connector listed as exporter on logs,
        // receiver on metrics. The render baseline has
        // `exporters: [otlp/signoz-...]` — after wiring, the cowork
        // connector should be appended.
        assert!(
            out.contains("signaltometrics/cowork]")
                || out.contains("signaltometrics/cowork,"),
            "connector not appended to logs.exporters or metrics.receivers"
        );
    }

    #[test]
    fn mapping_overlay_omits_cowork_connector_when_claude_desktop_disabled() {
        let mut state = crate::mappings::default_state();
        for h in &mut state.harnesses {
            if h.harness_id == HarnessId::ClaudeDesktop {
                h.enabled = false;
            }
        }
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let out = apply_mapping_overlay(yaml, &state);
        assert!(!out.contains("signaltometrics/cowork"));
    }

    #[test]
    fn mapping_overlay_omits_diag_pipeline_for_disabled_harness_only() {
        // Disabling one harness drops only that harness's diag pipeline;
        // the others (still enabled native emitters) keep theirs and the
        // shared `debug/diag-noop` exporter remains.
        let mut state = crate::mappings::default_state();
        for h in &mut state.harnesses {
            if h.harness_id == HarnessId::ClaudeDesktop {
                h.enabled = false;
            }
        }
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let out = apply_mapping_overlay(yaml, &state);
        assert!(!out.contains("filter/diag-claude-desktop"));
        assert!(!out.contains("logs/diag-claude-desktop"));
        assert!(!out.contains("traces/diag-claude-desktop"));
        assert!(!out.contains("metrics/diag-claude-desktop"));
        // Other native emitters remain.
        assert!(out.contains("filter/diag-claude-code:"));
        assert!(out.contains("debug/diag-noop:"));
    }

    #[test]
    fn mapping_overlay_is_a_noop_when_no_harness_emits_natively_or_synthesizes() {
        // If every harness is disabled (or has no native service.name
        // and no synthesis rows), there is nothing to overlay.
        let mut state = crate::mappings::default_state();
        for h in &mut state.harnesses {
            h.enabled = false;
        }
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        assert_eq!(apply_mapping_overlay(yaml.clone(), &state), yaml);
    }

    #[test]
    fn mapping_overlay_injects_harness_tag_and_per_harness_tier_a_blocks() {
        let state = crate::mappings::default_state();
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let out = apply_mapping_overlay(yaml, &state);
        assert_eq!(out.matches("transform/harness-tag:").count(), 1);
        for expected in [
            "metricstransform/tierA-claude-code:",
            "metricstransform/tierA-gemini-cli:",
            "metricstransform/tierA-codex-cli:",
            "metricstransform/tierA-qwen-code:",
        ] {
            assert_eq!(out.matches(expected).count(), 1, "missing {expected}");
        }
        assert_eq!(out.matches("metricstransform/tierA-opencode:").count(), 0);
    }

    #[test]
    fn mapping_overlay_appends_to_every_pipeline_processors_list() {
        let state = crate::mappings::default_state();
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let out = apply_mapping_overlay(yaml, &state);
        let new_baseline = out
            .matches("[batch, attributes/redact, resource/source, transform/harness-tag")
            .count();
        assert_eq!(new_baseline, 3);
        assert_eq!(
            out.matches("processors: [batch, attributes/redact, resource/source]")
                .count(),
            0,
        );
    }

    #[test]
    fn mapping_overlay_composes_with_identity_overlay() {
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let with_identity = apply_identity_overlay(yaml, &resolved("Ada", "ada@example.com"));
        let final_yaml = apply_mapping_overlay(with_identity, &crate::mappings::default_state());
        assert!(final_yaml.contains("resource/identity:"));
        assert!(final_yaml.contains("transform/harness-tag:"));
        assert!(
            final_yaml.contains(
                "processors: [batch, attributes/redact, resource/source, transform/harness-tag"
            ),
        );
        assert_eq!(final_yaml.matches(", resource/identity]").count(), 3);
    }

    #[test]
    fn mapping_overlay_output_parses_as_valid_yaml() {
        let state = crate::mappings::default_state();
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let out = apply_mapping_overlay(yaml, &state);
        let parsed: Result<serde_yml::Value, _> = serde_yml::from_str(&out);
        assert!(parsed.is_ok(), "invalid YAML: {:?}\n{out}", parsed.err());
    }

    #[test]
    fn combined_overlays_parse_as_valid_yaml() {
        let yaml = rendered_yaml(&[
            signoz_instance("aaaaaaaa-1111-2222-3333-444455556666"),
            datadog_instance("bbbbbbbb-1111-2222-3333-444455556666"),
        ]);
        let with_identity = apply_identity_overlay(yaml, &resolved("Ada", "ada@example.com"));
        let final_yaml = apply_mapping_overlay(with_identity, &crate::mappings::default_state());
        let parsed: Result<serde_yml::Value, _> = serde_yml::from_str(&final_yaml);
        assert!(
            parsed.is_ok(),
            "combined overlays produced invalid YAML: {:?}\n{final_yaml}",
            parsed.err(),
        );
    }

    #[test]
    fn mapping_overlay_byte_stable_across_two_runs() {
        let state = crate::mappings::default_state();
        let yaml = rendered_yaml(&[signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")]);
        let a = apply_mapping_overlay(yaml.clone(), &state);
        let b = apply_mapping_overlay(yaml, &state);
        assert_eq!(a, b);
    }

    // -----------------------------------------------------------------
    // endpoint_is_loopback + Backend::Signoz TLS auto-detect
    // -----------------------------------------------------------------

    #[test]
    fn endpoint_is_loopback_matches_common_local_forms() {
        // gRPC convention — no scheme, just host:port
        assert!(endpoint_is_loopback("localhost:14317"));
        assert!(endpoint_is_loopback("127.0.0.1:14317"));
        assert!(endpoint_is_loopback("[::1]:14317"));
        assert!(endpoint_is_loopback("0.0.0.0:14317"));
        // Bare hostname (no port)
        assert!(endpoint_is_loopback("localhost"));
        assert!(endpoint_is_loopback("127.0.0.1"));
        // With scheme (HTTP variant of SigNoz, hypothetical)
        assert!(endpoint_is_loopback("http://localhost:14318"));
        assert!(endpoint_is_loopback("http://127.0.0.1:14318/"));
        // Surrounding whitespace from a UI paste
        assert!(endpoint_is_loopback("  localhost:14317  "));
    }

    #[test]
    fn endpoint_is_loopback_rejects_cloud_and_lan_hosts() {
        assert!(!endpoint_is_loopback("ingest.us.signoz.cloud:443"));
        assert!(!endpoint_is_loopback("signoz.internal:4317"));
        // Not a DNS-resolution check; loopback-by-name only.
        assert!(!endpoint_is_loopback("loopback.example.com:4317"));
        // IPv4 outside the 127/8 loopback range — we only match the
        // exact textual `127.0.0.1` form.
        assert!(!endpoint_is_loopback("127.0.0.2:4317"));
        assert!(!endpoint_is_loopback("10.0.0.1:4317"));
        // Unparseable bracketed IPv6
        assert!(!endpoint_is_loopback("[::1"));
        assert!(!endpoint_is_loopback(""));
    }

    #[test]
    fn signoz_cloud_endpoint_renders_tls_insecure_false() {
        let instances = vec![signoz_instance("aaaaaaaa-1111-2222-3333-444455556666")];
        let yaml = rendered_yaml(&instances);
        // Inline-rendered block must keep TLS on for the cloud preset.
        assert!(
            yaml.contains("tls:\n      insecure: false"),
            "expected `tls: insecure: false` for cloud endpoint, got:\n{yaml}",
        );
    }

    #[test]
    fn signoz_loopback_endpoint_renders_tls_insecure_true() {
        let instance = BackendInstance {
            id: "bbbbbbbb-2222-3333-4444-555566667777".into(),
            label: None,
            enabled: true,
            backend: Backend::Signoz {
                endpoint: "localhost:14317".into(),
                ingestion_key: SecretRef::for_account(
                    "backend.signoz.ingestion-key.bbbbbbbb-2222-3333-4444-555566667777",
                ),
            },
        };
        let yaml = rendered_yaml(&[instance]);
        assert!(
            yaml.contains("tls:\n      insecure: true"),
            "expected `tls: insecure: true` for loopback endpoint, got:\n{yaml}",
        );
        // Sanity: the header line is still present (auth still required
        // syntactically even if self-hosted OSS ignores it).
        assert!(yaml.contains("signoz-ingestion-key"));
    }

    #[test]
    fn signoz_127_endpoint_also_renders_insecure() {
        let instance = BackendInstance {
            id: "cccccccc-3333-4444-5555-666677778888".into(),
            label: None,
            enabled: true,
            backend: Backend::Signoz {
                endpoint: "127.0.0.1:14317".into(),
                ingestion_key: SecretRef::for_account(
                    "backend.signoz.ingestion-key.cccccccc-3333-4444-5555-666677778888",
                ),
            },
        };
        let yaml = rendered_yaml(&[instance]);
        assert!(yaml.contains("tls:\n      insecure: true"), "got:\n{yaml}");
    }

    /// Round-trip: a component id derived from a backend UUID resolves
    /// back to that backend via `backend_id_from_component_id`. Uses real
    /// component-id shapes captured during the 2026-05-18 release pairing
    /// pass.
    #[test]
    fn backend_id_resolves_from_real_component_ids() {
        let signoz = signoz_instance("31fb8e0a-0d50-4636-ab1a-868a4428a092");
        let dd = datadog_instance("42520ad9-2033-4b79-bb96-43dd6e469225");
        let backends = vec![signoz.clone(), dd.clone()];

        assert_eq!(
            backend_id_from_component_id("otlp/signoz-31fb8e0a", &backends),
            Some(signoz.id.as_str()),
        );
        assert_eq!(
            backend_id_from_component_id("otlphttp/openobserve-42520ad9", &backends),
            Some(dd.id.as_str()),
            "kind prefix is irrelevant — only the suffix matters",
        );
    }

    #[test]
    fn backend_id_returns_none_for_unknown_suffix() {
        let signoz = signoz_instance("31fb8e0a-0d50-4636-ab1a-868a4428a092");
        assert_eq!(
            backend_id_from_component_id("otlp/signoz-deadbeef", &[signoz]),
            None,
        );
    }

    #[test]
    fn backend_id_returns_none_for_malformed_component_id() {
        let signoz = signoz_instance("31fb8e0a-0d50-4636-ab1a-868a4428a092");
        let backends = vec![signoz];
        // No slash: not an otelcol component id.
        assert_eq!(backend_id_from_component_id("31fb8e0a", &backends), None);
        // Just "otlp" — no suffix after the slash.
        assert_eq!(backend_id_from_component_id("otlp/", &backends), None);
        // Empty string.
        assert_eq!(backend_id_from_component_id("", &backends), None);
    }

    /// Receivers and processors don't have backend suffixes, so they
    /// must not accidentally resolve to a backend. Defends against a
    /// future refactor that points the resolver at every component id
    /// in the collector config rather than only exporters.
    #[test]
    fn backend_id_does_not_match_processor_component_ids() {
        let signoz = signoz_instance("31fb8e0a-0d50-4636-ab1a-868a4428a092");
        assert_eq!(
            backend_id_from_component_id("transform/harness-tag", std::slice::from_ref(&signoz)),
            None,
        );
        assert_eq!(
            backend_id_from_component_id("batch", &[signoz]),
            None,
        );
    }
}
