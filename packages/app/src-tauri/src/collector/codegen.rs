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
use crate::harness::HarnessId;
use crate::identity::{Resolved, ResolvedSource};
use crate::mappings::{HarnessMapping, MappingSource, MappingState};
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
        // Codex CLI's default service.name is the unprefixed `codex`
        // per upstream; users with shell env overrides may also see
        // `codex-cli`.
        HarnessId::CodexCli => &["codex-cli", "codex"],
        HarnessId::Opencode => &["opencode"],
        // Hook/watcher harnesses tag `harness.id` themselves in their
        // OTLP payload — no inference needed.
        HarnessId::CursorIde
        | HarnessId::CursorCli
        | HarnessId::Cline
        | HarnessId::Aider
        | HarnessId::CopilotCli => &[],
    }
}

/// Sanitize a `HarnessId` into the suffix used by `transform/tierA-*`
/// and `metricstransform/tierA-*` processor names. The collector
/// accepts `/` as the segment separator; the suffix mirrors the
/// kebab-case wire format of [`HarnessId`].
fn harness_id_suffix(id: HarnessId) -> &'static str {
    match id {
        HarnessId::ClaudeCode => "claude-code",
        HarnessId::GeminiCli => "gemini-cli",
        HarnessId::CodexCli => "codex-cli",
        HarnessId::QwenCode => "qwen-code",
        HarnessId::Opencode => "opencode",
        // Not used today (hook/watcher harnesses emit Tier A inline)
        // but provided for symmetry should a hybrid emerge.
        HarnessId::CursorIde => "cursor-ide",
        HarnessId::CursorCli => "cursor-cli",
        HarnessId::Cline => "cline",
        HarnessId::Aider => "aider",
        HarnessId::CopilotCli => "copilot-cli",
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
    let synth_harnesses: Vec<&HarnessMapping> = mappings.native_synthesis_harnesses();
    if synth_harnesses.is_empty() {
        return yaml;
    }

    let tag_block = build_harness_tag_block(&synth_harnesses);
    let tier_a_blocks: Vec<(String, String)> = synth_harnesses
        .iter()
        .filter_map(|h| {
            let block = build_tier_a_block(h)?;
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
    replace_pipeline_processors(&logs_updated, "traces", &other_inserts)
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
fn build_tier_a_block(harness: &HarnessMapping) -> Option<String> {
    if !harness.enabled {
        return None;
    }
    let mut transforms = String::new();
    for source in &harness.sources {
        let MappingSource::SynthesizeFromNative {
            native_metric,
            target_metric,
            attribute_map,
        } = source
        else {
            continue;
        };
        let _ = writeln!(transforms, "      - include: {native_metric}");
        let _ = writeln!(transforms, "        match_type: strict");
        let _ = writeln!(transforms, "        action: insert");
        let _ = writeln!(
            transforms,
            "        new_name: {}",
            target_metric.full_name()
        );
        if !attribute_map.is_empty() {
            let _ = writeln!(transforms, "        operations:");
            // BTreeMap iteration is sorted, so YAML order is stable
            // across re-renders (golden-file friendly).
            for (raw, tier_a) in attribute_map {
                let _ = writeln!(transforms, "          - action: update_label");
                let _ = writeln!(transforms, "            label: {raw}");
                let _ = writeln!(transforms, "            new_label: {tier_a}");
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

    fn resolved(name: &str, email: &str) -> Resolved {
        Resolved {
            name: name.to_string(),
            email: email.to_string(),
            source: ResolvedSource::GitConfig,
        }
    }

    #[test]
    fn identity_overlay_is_a_noop_when_source_is_none() {
        let original = SIGNOZ_TEMPLATE.to_string();
        let r = Resolved {
            name: String::new(),
            email: String::new(),
            source: ResolvedSource::None,
        };
        assert_eq!(apply_identity_overlay(original.clone(), &r), original);
    }

    #[test]
    fn identity_overlay_is_a_noop_when_both_values_empty() {
        let original = SIGNOZ_TEMPLATE.to_string();
        let r = resolved("", "");
        assert_eq!(apply_identity_overlay(original.clone(), &r), original);
    }

    #[test]
    fn identity_overlay_injects_processor_and_appends_to_pipelines() {
        let yaml = SIGNOZ_TEMPLATE.to_string();
        let baseline_pipeline_hits = yaml
            .matches("processors: [batch, attributes/redact, resource/source]")
            .count();
        // Three pipeline lines (metrics/logs/traces) on every template.
        assert_eq!(baseline_pipeline_hits, 3, "preset template invariant changed");

        let r = resolved("Ada Lovelace", "ada@example.com");
        let out = apply_identity_overlay(yaml, &r);

        // Block injected once at the top-level processors map.
        assert_eq!(out.matches("resource/identity:").count(), 1);
        assert!(out.contains("- { key: user.name, value: \"Ada Lovelace\", action: upsert }"));
        assert!(out.contains("- { key: user.email, value: \"ada@example.com\", action: upsert }"));

        // Every pipeline line picks up the new processor.
        let original_pipeline_line =
            "processors: [batch, attributes/redact, resource/source]";
        let new_pipeline_line =
            "processors: [batch, attributes/redact, resource/source, resource/identity]";
        assert_eq!(out.matches(original_pipeline_line).count(), 0);
        assert_eq!(out.matches(new_pipeline_line).count(), 3);
    }

    #[test]
    fn identity_overlay_handles_name_only_or_email_only() {
        let yaml = HONEYCOMB_TEMPLATE.to_string();

        let with_name = apply_identity_overlay(yaml.clone(), &resolved("Ada", ""));
        assert!(with_name.contains("user.name"));
        assert!(!with_name.contains("user.email"));

        let with_email = apply_identity_overlay(yaml, &resolved("", "ada@example.com"));
        assert!(with_email.contains("user.email"));
        assert!(!with_email.contains("user.name"));
    }

    #[test]
    fn identity_overlay_yaml_escapes_quotes_and_backslashes_in_values() {
        let yaml = SIGNOZ_TEMPLATE.to_string();
        let r = resolved("o\"\\dd", "ada@example.com");
        let out = apply_identity_overlay(yaml, &r);
        // Backslash and quote are escaped inside the double-quoted YAML scalar.
        assert!(out.contains(r#"value: "o\"\\dd", action: upsert"#));
    }

    #[test]
    fn identity_overlay_does_not_touch_unrelated_template_bytes() {
        // Smoke-test: the smoke-test/passthrough template lacks the
        // canonical pipeline line, so the overlay must be a no-op
        // beyond injecting the top-level block if pipelines are
        // missing. Confirms we don't accidentally double-apply.
        let custom = "extensions:\nprocessors:\n  noop:\nservice:\n  pipelines:\n    traces:\n      processors: [noop]\n".to_string();
        let r = resolved("Ada", "ada@x");
        let out = apply_identity_overlay(custom, &r);
        assert!(out.contains("resource/identity:"));
        // Pipeline line didn't match our verbatim anchor — left alone.
        assert!(out.contains("processors: [noop]"));
    }

    // -----------------------------------------------------------------
    // apply_mapping_overlay
    // -----------------------------------------------------------------

    #[test]
    fn mapping_overlay_is_a_noop_for_empty_mapping_state() {
        let empty = MappingState {
            schema_version: 1,
            harnesses: vec![],
        };
        let yaml = SIGNOZ_TEMPLATE.to_string();
        assert_eq!(apply_mapping_overlay(yaml.clone(), &empty), yaml);
    }

    #[test]
    fn mapping_overlay_is_a_noop_when_no_harness_has_synthesis_rows() {
        // A state populated only with hook-rule mappings doesn't need
        // any collector transforms — those harnesses emit Tier A inline.
        let mut state = crate::mappings::default_state();
        state.harnesses.retain(|h| {
            !h.sources
                .iter()
                .any(|s| matches!(s, MappingSource::SynthesizeFromNative { .. }))
        });
        let yaml = SIGNOZ_TEMPLATE.to_string();
        assert_eq!(apply_mapping_overlay(yaml.clone(), &state), yaml);
    }

    #[test]
    fn mapping_overlay_injects_harness_tag_and_per_harness_tier_a_blocks() {
        let state = crate::mappings::default_state();
        let yaml = SIGNOZ_TEMPLATE.to_string();
        let out = apply_mapping_overlay(yaml, &state);

        // Single harness-tag block at the top.
        assert_eq!(out.matches("transform/harness-tag:").count(), 1);
        // One metricstransform per native-OTel harness with default
        // synthesis rows. Claude Code, Gemini, Codex, Qwen all qualify
        // — Opencode's default mapping is empty pending verification.
        for expected in [
            "metricstransform/tierA-claude-code:",
            "metricstransform/tierA-gemini-cli:",
            "metricstransform/tierA-codex-cli:",
            "metricstransform/tierA-qwen-code:",
        ] {
            assert_eq!(
                out.matches(expected).count(),
                1,
                "missing transform block for {expected}"
            );
        }
        assert_eq!(out.matches("metricstransform/tierA-opencode:").count(), 0);
    }

    #[test]
    fn mapping_overlay_appends_to_every_pipeline_processors_list() {
        let state = crate::mappings::default_state();
        let yaml = SIGNOZ_TEMPLATE.to_string();
        let out = apply_mapping_overlay(yaml, &state);

        // Baseline pipeline lines (3, one per signal type) all picked
        // up the new processors and now include `transform/harness-tag`.
        let new_baseline = out
            .matches("[batch, attributes/redact, resource/source, transform/harness-tag")
            .count();
        assert_eq!(new_baseline, 3, "expected 3 pipeline lines to be rewritten");
        // The unmodified baseline line should no longer be present.
        assert_eq!(
            out.matches("processors: [batch, attributes/redact, resource/source]")
                .count(),
            0
        );
    }

    #[test]
    fn mapping_overlay_uses_insert_action_to_preserve_tier_b_passthrough() {
        // MAPPING_PLAN.md §Defaults says Tier B passes through unchanged.
        // metricstransform's `insert` action means the native metric stays
        // intact while a renamed copy lands on Tier A. A regression to
        // `update` (rename) would silently break Tier B dashboards.
        let state = crate::mappings::default_state();
        let out = apply_mapping_overlay(SIGNOZ_TEMPLATE.to_string(), &state);
        assert!(out.contains("action: insert"));
        // Reject the rename-style action being accidentally generated.
        assert_eq!(out.matches("action: update\n").count(), 0);
    }

    #[test]
    fn mapping_overlay_emits_attribute_relabel_for_gen_ai_token_type() {
        // Gemini/Qwen/Codex defaults map `gen_ai.token.type` → `direction`.
        // The metricstransform `update_label` operation renames the
        // attribute on the synthesized metric without touching the
        // native one (insert keeps both).
        let state = crate::mappings::default_state();
        let out = apply_mapping_overlay(SIGNOZ_TEMPLATE.to_string(), &state);
        assert!(out.contains("- action: update_label"));
        assert!(out.contains("label: gen_ai.token.type"));
        assert!(out.contains("new_label: direction"));
    }

    #[test]
    fn mapping_overlay_composes_with_identity_overlay() {
        // Both overlays in either order must produce a pipeline list that
        // contains every expected processor name in order. Identity ran
        // first here; the mapping overlay sees the post-identity pipeline
        // line and threads its processors in *before* `resource/identity`
        // so identity tags the synthesized Tier A metrics too.
        let yaml = SIGNOZ_TEMPLATE.to_string();
        let r = resolved("Ada", "ada@example.com");
        let with_identity = apply_identity_overlay(yaml, &r);
        let final_yaml =
            apply_mapping_overlay(with_identity, &crate::mappings::default_state());

        // Confirm both overlays' definitions are present at the top.
        assert!(final_yaml.contains("resource/identity:"));
        assert!(final_yaml.contains("transform/harness-tag:"));

        // Pipeline list: harness-tag and tierA-* come before
        // resource/identity (so identity sees the synthesized metrics).
        // Match the full canonical line shape.
        assert!(
            final_yaml.contains(
                "processors: [batch, attributes/redact, resource/source, transform/harness-tag"
            ),
            "harness-tag should appear after resource/source"
        );
        assert_eq!(
            final_yaml.matches(", resource/identity]").count(),
            3,
            "identity must remain at the tail of every pipeline"
        );
    }

    #[test]
    fn mapping_overlay_byte_stable_across_two_runs_with_same_input() {
        // Golden-file tests rely on the YAML being deterministic across
        // re-renders — BTreeMap iteration order and vec iteration order
        // are both stable by construction, but the test pins the
        // invariant so a future map swap doesn't silently break it.
        let state = crate::mappings::default_state();
        let a = apply_mapping_overlay(SIGNOZ_TEMPLATE.to_string(), &state);
        let b = apply_mapping_overlay(SIGNOZ_TEMPLATE.to_string(), &state);
        assert_eq!(a, b);
    }

    #[test]
    fn metricstransform_tier_a_is_only_added_to_the_metrics_pipeline() {
        // metricstransform doesn't support log/trace signal types; the
        // collector exits 1 at startup if we wire it into the logs or
        // traces pipelines. Pin the invariant so a future refactor
        // can't accidentally regress.
        let state = crate::mappings::default_state();
        let out = apply_mapping_overlay(SIGNOZ_TEMPLATE.to_string(), &state);

        let metrics_line = out
            .lines()
            .find(|l| l.contains("processors: ["))
            .expect("at least one pipeline processors line");
        assert!(
            metrics_line.contains("metricstransform/tierA-claude-code"),
            "metrics pipeline should include tierA blocks: {metrics_line}"
        );

        // The lines come after `metrics:`, `logs:`, `traces:` in template
        // order. Lines 1 and 2 are logs and traces.
        let pipeline_lines: Vec<&str> = out
            .lines()
            .filter(|l| l.contains("processors: ["))
            .collect();
        assert_eq!(pipeline_lines.len(), 3, "three pipelines expected");

        for (i, line) in pipeline_lines.iter().enumerate().skip(1) {
            assert!(
                !line.contains("metricstransform/"),
                "non-metrics pipeline {i} must not include metricstransform: {line}"
            );
            assert!(
                line.contains("transform/harness-tag"),
                "non-metrics pipeline {i} should still carry harness-tag: {line}"
            );
        }
    }

    #[test]
    fn harness_tag_emits_a_statement_per_candidate_service_name() {
        // Gemini's harness-tag block should fire for both "gemini-cli"
        // and the unprefixed "gemini" service.name. Guards against
        // upstream rebranding silently breaking detection.
        let state = crate::mappings::default_state();
        let out = apply_mapping_overlay(SIGNOZ_TEMPLATE.to_string(), &state);
        assert!(out.contains(
            r#"set(attributes["harness.id"], "gemini-cli") where attributes["service.name"] == "gemini-cli""#
        ));
        assert!(out.contains(
            r#"set(attributes["harness.id"], "gemini-cli") where attributes["service.name"] == "gemini""#
        ));
        assert!(out.contains(
            r#"set(attributes["harness.id"], "codex-cli") where attributes["service.name"] == "codex""#
        ));
    }

    #[test]
    fn mapping_overlay_output_parses_as_valid_yaml_against_every_preset() {
        // The bytes the supervisor hands to the otelcol child are the
        // overlay's output. If we ever emit a syntax error (mismatched
        // indentation, unquoted special character, etc.), the
        // collector exits 1 at startup with a cryptic message. Catch
        // that at codegen time instead.
        let state = crate::mappings::default_state();
        for (name, tpl) in [
            ("signoz", SIGNOZ_TEMPLATE),
            ("honeycomb", HONEYCOMB_TEMPLATE),
            ("grafana-cloud", GRAFANA_CLOUD_TEMPLATE),
            ("datadog", DATADOG_TEMPLATE),
            ("otelcol-passthrough", OTELCOL_PASSTHROUGH_TEMPLATE),
        ] {
            let out = apply_mapping_overlay(tpl.to_string(), &state);
            let parsed: Result<serde_yml::Value, _> = serde_yml::from_str(&out);
            assert!(
                parsed.is_ok(),
                "mapping overlay produced invalid YAML for preset {name}: {:?}\n--- yaml ---\n{out}",
                parsed.err()
            );
        }
    }

    #[test]
    fn mapping_overlay_combined_with_identity_overlay_parses_as_valid_yaml() {
        // Stack both overlays — the order the supervisor uses in
        // `prepare_collector_runtime` — and parse the result. Identity
        // is OK to render unconditionally; with empty values it's a
        // no-op.
        let state = crate::mappings::default_state();
        let r = resolved("Ada Lovelace", "ada@example.com");
        let with_identity = apply_identity_overlay(SIGNOZ_TEMPLATE.to_string(), &r);
        let final_yaml = apply_mapping_overlay(with_identity, &state);
        let parsed: Result<serde_yml::Value, _> = serde_yml::from_str(&final_yaml);
        assert!(
            parsed.is_ok(),
            "combined overlays produced invalid YAML: {:?}\n--- yaml ---\n{final_yaml}",
            parsed.err()
        );
    }

    #[test]
    fn mapping_overlay_handles_every_preset_template() {
        // Every preset uses the same baseline pipeline anchor. Sanity
        // check that the overlay produces a non-empty diff for each.
        let state = crate::mappings::default_state();
        for tpl in [
            SIGNOZ_TEMPLATE,
            HONEYCOMB_TEMPLATE,
            GRAFANA_CLOUD_TEMPLATE,
            DATADOG_TEMPLATE,
            OTELCOL_PASSTHROUGH_TEMPLATE,
        ] {
            let out = apply_mapping_overlay(tpl.to_string(), &state);
            assert_ne!(out, tpl, "preset template did not pick up overlay");
            assert!(out.contains("transform/harness-tag:"));
        }
    }
}
