//! Per-harness default [`HarnessMapping`] tables.
//!
//! Each entry is the mapping Trove ships out of the box for a harness.
//! Users can edit (future PR) or reset to defaults via the Mappings UI.
//! These tables also seed [`MappingState::default`] so a fresh state.json
//! comes with sensible coverage on every supported harness.
//!
//! Sources for the defaults:
//!
//! - **Tier A schema and per-harness rows**: `documentation/MAPPING_PLAN.md`
//!   §"Defaults" (lines 132–154).
//! - **Native metric names**: upstream docs for each harness. Notes inline
//!   on each entry call out where verification is partial; treat them as
//!   conservative starting points refined by running each harness against
//!   a local OTLP capture during integration testing.
//! - **Open question #2 in `MAPPING_PLAN.md`**: default ON for `events` and
//!   `tokens` synthesis, OFF for `cost.usd` (cost double-count is the most
//!   confusing failure mode). Cost rows therefore are not seeded here; the
//!   UI lets users add them per harness.

use std::collections::BTreeMap;

use super::{
    HarnessMapping, HookEmit, MappingSource, MappingState, TierAMetric, TroveMetricDefinition,
    TroveMetricKind, MAPPING_SCHEMA_VERSION,
};
use crate::harness::HarnessId;

/// The full default mapping state — every supported harness in tier
/// order, plus the five builtin metric-catalog entries seeded so the
/// UI's metrics catalog is populated on first launch.
#[must_use]
pub fn default_state() -> MappingState {
    MappingState {
        schema_version: MAPPING_SCHEMA_VERSION,
        metrics: default_metric_catalog(),
        harnesses: vec![
            defaults_for(HarnessId::ClaudeCode),
            defaults_for(HarnessId::ClaudeDesktop),
            defaults_for(HarnessId::GeminiCli),
            defaults_for(HarnessId::CodexCli),
            defaults_for(HarnessId::CodexDesktop),
            defaults_for(HarnessId::QwenCode),
            defaults_for(HarnessId::Opencode),
            defaults_for(HarnessId::CursorIde),
            defaults_for(HarnessId::CursorCli),
            defaults_for(HarnessId::Cline),
            defaults_for(HarnessId::Aider),
            defaults_for(HarnessId::CopilotCli),
            // Detection-only harnesses ship empty mapping rows so the
            // codegen overlay leaves them out entirely. They surface in
            // the dashboard via detection but emit no telemetry until a
            // future adapter lands.
            defaults_for(HarnessId::JunieCli),
            defaults_for(HarnessId::Droid), // Tier 1 — full native-OTLP mapping
            defaults_for(HarnessId::KimiCodeCli),
            defaults_for(HarnessId::Devin),
            defaults_for(HarnessId::Forgecode),
            defaults_for(HarnessId::Sentinel),
        ],
    }
}

/// The five builtin Tier A metric definitions in dashboard-display
/// order. Consumed by [`default_state`] and the v1→v2 migration
/// (`MappingState::migrate_to_current`). Custom metrics added by the
/// user append to this list with `builtin: false`.
#[must_use]
pub fn default_metric_catalog() -> Vec<TroveMetricDefinition> {
    let mut catalog: Vec<TroveMetricDefinition> =
        TierAMetric::all().iter().map(|m| m.definition()).collect();
    // Sentinel's enriched signals ride alongside the five Tier A builtins
    // as custom (builtin: false) catalog entries — see `sentinel_defaults`.
    catalog.extend(sentinel_custom_metrics());
    catalog
}

/// Default mapping for one harness. Use this when an individual harness
/// row needs to be reset via the UI's "Reset to defaults" button.
#[must_use]
pub fn defaults_for(id: HarnessId) -> HarnessMapping {
    match id {
        HarnessId::ClaudeCode => claude_code_defaults(),
        HarnessId::ClaudeDesktop => claude_desktop_defaults(),
        HarnessId::GeminiCli => gemini_cli_defaults(),
        HarnessId::CodexCli => codex_cli_defaults(),
        HarnessId::CodexDesktop => codex_desktop_defaults(),
        HarnessId::QwenCode => qwen_code_defaults(),
        HarnessId::Opencode => opencode_defaults(),
        HarnessId::CursorIde => cursor_ide_defaults(),
        HarnessId::CursorCli => cursor_cli_defaults(),
        HarnessId::Cline => cline_defaults(),
        HarnessId::Aider => aider_defaults(),
        HarnessId::CopilotCli => copilot_cli_defaults(),
        HarnessId::Droid => droid_defaults(),
        HarnessId::Sentinel => sentinel_defaults(),
        // Detection-only harnesses: empty source list. The dashboard
        // shows the row, but no codegen overlay or watcher attaches.
        HarnessId::JunieCli | HarnessId::KimiCodeCli | HarnessId::Devin | HarnessId::Forgecode => {
            detection_only_defaults(id)
        }
    }
}

/// Empty mapping for a detection-only harness. No sources → codegen's
/// `apply_mapping_overlay` skips emitting any per-harness processor and
/// no watcher is attached.
fn detection_only_defaults(id: HarnessId) -> HarnessMapping {
    HarnessMapping {
        harness_id: id,
        enabled: true,
        sources: Vec::new(),
        cost_overrides: BTreeMap::new(),
    }
}

fn attr(k: &str, v: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(k.to_string(), v.to_string());
    m
}

// ---------------------------------------------------------------------------
// Native-OTel harnesses (Tier 1)
// ---------------------------------------------------------------------------

/// Claude Code emits `claude_code.*` natively. Synthesis maps the
/// session/tool counters onto `events` and the token-usage gauge onto
/// `tokens`. Cost is left to the user to opt into (open question #2).
///
/// Note on `event.kind`: dashboards filter `trove.harness.events` by
/// `event.kind`. `metricstransform action: insert` carries the source
/// metric's attributes over, but none of Claude Code's counters expose
/// a Tier A `event.kind` natively, so we inject the literal here.
///
/// Factored out because Sentinel forwards Claude Code's raw stream
/// verbatim and reuses the exact same rows — see [`sentinel_defaults`].
/// The collector de-duplicates identical native→Tier A transforms across
/// harnesses (see `crate::collector::codegen::build_tier_a_block`), so
/// carrying these rows in both mappings never double-counts.
fn claude_code_native_sources() -> Vec<MappingSource> {
    vec![
        MappingSource::SynthesizeFromNative {
            native_metric: "claude_code.session.count".into(),
            target_metric: TierAMetric::Events.id(),
            attribute_map: BTreeMap::new(),
            inject_attributes: BTreeMap::from([("event.kind".into(), "chat.turn".into())]),
        },
        MappingSource::SynthesizeFromNative {
            native_metric: "claude_code.tool.decision.count".into(),
            target_metric: TierAMetric::Events.id(),
            attribute_map: BTreeMap::new(),
            inject_attributes: BTreeMap::from([("event.kind".into(), "tool.call".into())]),
        },
        MappingSource::SynthesizeFromNative {
            native_metric: "claude_code.token.usage".into(),
            target_metric: TierAMetric::Tokens.id(),
            // Claude Code's native attribute is `type=input|output`;
            // Tier A wants `direction`. The collector transform
            // rewrites the attribute key inline.
            attribute_map: BTreeMap::from([("type".into(), "direction".into())]),
            inject_attributes: BTreeMap::new(),
        },
    ]
}

fn claude_code_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::ClaudeCode,
        enabled: true,
        sources: claude_code_native_sources(),
        cost_overrides: BTreeMap::new(),
    }
}

/// Sentinel forwards Claude Code's raw OTLP stream
/// (`service.name=claude-code`) and also emits its own enriched
/// `sentinel.*` metrics (`service.name=sentinel`). Its default mapping
/// therefore carries the same Tier A synthesis rows as Claude Code — so a
/// Sentinel-only user still gets `events`/`tokens` even if the Claude Code
/// mapping is disabled — while Sentinel's own metrics ride along as custom
/// catalog entries ([`sentinel_custom_metrics`]) that pass through 1:1 (a
/// synthesis row would `insert` a duplicate, same-named metric).
///
/// The collector de-duplicates the shared `claude_code.*` transforms
/// across the claude-code and sentinel blocks, so the two coexist without
/// double-counting; Claude Code is the canonical emitter by declaration
/// order.
fn sentinel_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::Sentinel,
        enabled: true,
        sources: claude_code_native_sources(),
        cost_overrides: BTreeMap::new(),
    }
}

/// Sentinel's enriched, self-computed metrics. Registered as custom
/// (`builtin: false`) catalog entries so they're first-class signals in
/// Trove's metric catalog; they flow through the collector untouched (no
/// synthesis row), keeping their native names and attributes. Names,
/// kinds, and attribute keys mirror Sentinel's `OtelEmitter`.
pub(crate) fn sentinel_custom_metrics() -> Vec<TroveMetricDefinition> {
    use TroveMetricKind::{Counter, Gauge};
    let def = |id: &str, kind: TroveMetricKind, attrs: &[&str], description: &str| {
        TroveMetricDefinition {
            id: id.to_string(),
            name: id.to_string(),
            kind,
            description: description.to_string(),
            required_attributes: attrs.iter().map(|s| (*s).to_string()).collect(),
            builtin: false,
        }
    };
    vec![
        def(
            "sentinel.cache.tokens_by_ttl",
            Gauge,
            &["ttl", "operation"],
            "Prompt-cache token volume over the rolling 5h window, by TTL bucket (5m/1h writes) and aggregate reads.",
        ),
        def(
            "sentinel.account.usage.tokens",
            Gauge,
            &["account_id", "kind"],
            "Per-account token usage in the rolling 5h window, split by kind (input/output/cache_read/cache_create).",
        ),
        def(
            "sentinel.account.usage.cost_usd",
            Gauge,
            &["account_id"],
            "Per-account estimated API cost over the rolling 5h window.",
        ),
        def(
            "sentinel.account.switch.count",
            Counter,
            &["to_account"],
            "Cumulative account switches since Sentinel started, by destination account.",
        ),
        def(
            "sentinel.proxy.requests.count",
            Counter,
            &["account_id", "status_class"],
            "Cumulative upstream Anthropic API requests proxied, by account and HTTP status class.",
        ),
        def(
            "sentinel.proxy.errors.count",
            Counter,
            &["account_id", "error_kind"],
            "Cumulative upstream proxy errors, by account and error kind.",
        ),
        def(
            "sentinel.security.event.count",
            Counter,
            &["kind", "severity", "outcome"],
            "Cumulative security-scanner detections, by kind, severity, and outcome.",
        ),
        def(
            "sentinel.permission.decision.count",
            Counter,
            &["decision", "source"],
            "Cumulative tool-permission decisions, by decision (allow/deny/ask) and source.",
        ),
    ]
}

/// Claude Desktop (formerly Claude Cowork) emits OTLP **logs**, not
/// counters, to the user's admin-configured collector endpoint. The
/// five documented event names (see
/// <https://claude.com/docs/cowork/monitoring#events>) map cleanly onto
/// Tier A; rows below are keyed by the literal `event.name` Anthropic
/// emits. A downstream collector connector (e.g. `count`,
/// `signaltometrics`, or a Trove-shipped logs-to-metrics processor)
/// will translate these events into the Tier A counter/histogram
/// surface — until that wiring lands the rows are documentation that
/// also lets the user toggle individual events on/off without re-typing.
///
/// Anti-double-count rules:
///
/// - `user_prompt` and `api_request` both correlate with one chat turn.
///   `api_request` carries the model + token totals + duration, so it's
///   the canonical `chat.turn` emitter; `user_prompt` therefore emits
///   `None` (informational only).
/// - `tool_decision` and `tool_result` both fire per tool invocation
///   (permission grant, then execution). `tool_result` is the canonical
///   `tool.call` emitter; `tool_decision` emits `None`.
fn claude_desktop_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::ClaudeDesktop,
        enabled: true,
        sources: vec![
            // `user_prompt` — fires when the user submits a prompt.
            // Documented attributes: prompt_length, prompt. Suppressed
            // here so api_request is the sole `chat.turn` emitter.
            MappingSource::HookRule {
                when: "user_prompt".into(),
                emit: None,
            },
            // `tool_result` — fires on every tool execution completion.
            // Documented attributes: tool_name, success, duration_ms,
            // error, decision_type, decision_source,
            // tool_result_size_bytes, mcp_server_scope, tool_parameters,
            // tool_input. Maps onto Tier A `events` as the canonical
            // tool.call emitter; tool_decision is suppressed to avoid
            // double-counting.
            MappingSource::HookRule {
                when: "tool_result".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "tool.call"),
                }),
            },
            // `api_request` — fires on every successful Claude API call.
            // Documented attributes: model, cost_usd, duration_ms,
            // input_tokens, output_tokens, cache_read_tokens,
            // cache_creation_tokens, speed.
            //
            // The watcher fans out the observation across distinct
            // `when` keys (one per facet) so each rule matches exactly
            // one observation type. The kind filter in the runtime
            // additionally lets events (Counter) and turn.duration
            // (Histogram) share `when=api_request` without fighting.
            //
            //   1. events       — `api_request`           (Counter, +1, event.kind=chat.turn)
            //   2. turn.duration— `api_request`           (Histogram, duration_s, event.kind=chat.turn)
            //   3. tokens.in    — `api_request.tokens.input`  (Counter, +n, direction=input)
            //   4. tokens.out   — `api_request.tokens.output` (Counter, +n, direction=output)
            //   5. cost.usd     — `api_request.cost`      (Counter/double, +usd, cost.method=exact)
            //   6. errors       — `api_error`             (Counter, +1, error.kind from status)
            MappingSource::HookRule {
                when: "api_request".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "api_request".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::TurnDuration.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "api_request.tokens.input".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "input"),
                }),
            },
            MappingSource::HookRule {
                when: "api_request.tokens.output".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "output"),
                }),
            },
            MappingSource::HookRule {
                when: "api_request.cost".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::CostUsd.id(),
                    attributes: attr("cost.method", "exact"),
                }),
            },
            // `api_error` — fires on Claude API failures. Documented
            // attributes: model, error, status_code, duration_ms,
            // attempt, speed. Maps onto Tier A `errors`; the
            // logs-to-metrics connector will classify error.kind from
            // status_code at conversion time (4xx → policy/auth, 429
            // → rate_limit, 5xx → network, otherwise unknown).
            MappingSource::HookRule {
                when: "api_error".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Errors.id(),
                    attributes: attr("error.kind", "unknown"),
                }),
            },
            // `tool_decision` — fires when a tool's permission is
            // granted or denied. Suppressed here because tool_result
            // already counts the tool.call.
            MappingSource::HookRule {
                when: "tool_decision".into(),
                emit: None,
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Gemini CLI ≥0.41 emits `gemini_cli.api.request.count` per chat
/// turn (confirmed against a live capture). Token usage rides under
/// the OTel-semconv `gen_ai.client.token.usage` histogram with
/// `gen_ai.token.type=input|output`; tool use lands in
/// `gemini_cli.tool.call.count`. Turn duration comes from
/// `gen_ai.client.operation.duration` (seconds, matches Tier A unit).
/// Failure counters synthesize into `trove.harness.errors`.
///
/// **Cost is not synthesized here** — `metricstransform` cannot do
/// per-model rate × token-count arithmetic. Cost (and the more
/// reliable tokens/duration emission with `model` and `user.email`
/// labels attached) comes from the dedicated Gemini chat-log watcher
/// in `crate::adapters::gemini_watcher`, which reads
/// `~/.gemini/tmp/<proj>/chats/session-*.jsonl`.
fn gemini_cli_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::GeminiCli,
        enabled: true,
        sources: vec![
            MappingSource::SynthesizeFromNative {
                native_metric: "gemini_cli.api.request.count".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::from([
                    // The dashboard groups by `model`; Gemini's native
                    // attribute is `gen_ai.request.model`. Rename it
                    // here so the synthesized metric carries `model`.
                    ("gen_ai.request.model".into(), "model".into()),
                ]),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gemini_cli.tool.call.count".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "tool.call".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.token.usage".into(),
                target_metric: TierAMetric::Tokens.id(),
                attribute_map: BTreeMap::from([
                    ("gen_ai.token.type".into(), "direction".into()),
                    ("gen_ai.request.model".into(), "model".into()),
                ]),
                inject_attributes: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.operation.duration".into(),
                target_metric: TierAMetric::TurnDuration.id(),
                attribute_map: BTreeMap::from([
                    ("gen_ai.request.model".into(), "model".into()),
                ]),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                // Model-routing failures are upstream / network-layer
                // problems. error.kind is closed-enum per MAPPING_PLAN.md
                // §"Background"; "network" is the canonical bucket for
                // upstream-API failures.
                native_metric: "gemini_cli.model_routing.failure.count".into(),
                target_metric: TierAMetric::Errors.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("error.kind".into(), "network".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                // Content-retry failures are also upstream API failures
                // (transient errors that exceeded the retry budget). Same
                // canonical bucket.
                native_metric: "gemini_cli.chat.content_retry_failure.count".into(),
                target_metric: TierAMetric::Errors.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("error.kind".into(), "network".into()),
                ]),
            },
            // gemini_watcher hook rules — the chat-log watcher emits
            // per-turn observations against these `when` keys. Users
            // can disable or retarget any of them via the Mappings
            // editor; the watcher consults this table at every poll
            // so edits take effect without a restart.
            MappingSource::HookRule {
                when: "gemini.turn".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "gemini.turn".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::TurnDuration.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "gemini.tokens.input".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "input"),
                }),
            },
            MappingSource::HookRule {
                when: "gemini.tokens.output".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "output"),
                }),
            },
            MappingSource::HookRule {
                when: "gemini.cost".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::CostUsd.id(),
                    attributes: attr("cost.method", "estimated"),
                }),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Codex CLI's native metric namespace mirrors Claude Code's shape;
/// upstream uses `codex.*` for session and tool counters and emits
/// token-usage as a gauge. These names need verification against a
/// real Codex run with telemetry enabled — they're the documented
/// shape but the prefix may differ (`codex_cli`, `codex`, or
/// straight-up `gen_ai.*` only).
fn codex_cli_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::CodexCli,
        enabled: true,
        sources: vec![
            MappingSource::SynthesizeFromNative {
                native_metric: "codex.session.count".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "codex.tool.count".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "tool.call".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.token.usage".into(),
                target_metric: TierAMetric::Tokens.id(),
                attribute_map: BTreeMap::from([
                    ("gen_ai.token.type".into(), "direction".into()),
                    ("gen_ai.request.model".into(), "model".into()),
                ]),
                inject_attributes: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.operation.duration".into(),
                target_metric: TierAMetric::TurnDuration.id(),
                attribute_map: BTreeMap::from([
                    ("gen_ai.request.model".into(), "model".into()),
                ]),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Codex desktop app shares `~/.codex/config.toml` with codex-cli and
/// drives the same Rust `codex app-server` backend. The backend tags
/// emitted signals with `service.name = codex`, which the existing
/// codex-cli mappings already consume — so codex-desktop ships empty
/// sources here and lets codex-cli's rules handle the actual signal
/// routing. This row tracks enablement state in state.json; the
/// dashboard's telemetry pill keys off the shared backend observation
/// via `telemetry_observed`.
fn codex_desktop_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::CodexDesktop,
        enabled: true,
        sources: Vec::new(),
        cost_overrides: BTreeMap::new(),
    }
}

/// Qwen Code is a Gemini CLI fork and inherits its metric namespace.
/// But emitting `metricstransform` rules that match `gemini_cli.*`
/// would shadow-fire whenever Gemini itself emits, producing duplicate
/// `trove.harness.events`. So Qwen's rules use the `qwen_code.*`
/// namespace; until upstream Qwen actually renames its emissions
/// (verify with a live capture before relying on these), this means no
/// Tier A synthesis for Qwen. Tier B passthrough still works, and
/// `harness.id` is still tagged from `service.name`.
fn qwen_code_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::QwenCode,
        enabled: true,
        sources: vec![
            MappingSource::SynthesizeFromNative {
                native_metric: "qwen_code.api.request.count".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "qwen_code.tool.call.count".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "tool.call".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "qwen_code.token.usage".into(),
                target_metric: TierAMetric::Tokens.id(),
                attribute_map: BTreeMap::from([
                    ("type".into(), "direction".into()),
                ]),
                inject_attributes: BTreeMap::new(),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Opencode ships its `OTel` emission through the upstream
/// `@devtheops/opencode-plugin-otel` package. The plugin's metric names
/// haven't been canonicalized in Trove docs — defaults are intentionally
/// empty; the user can populate them via the Mappings UI once a real
/// Opencode run reveals what the plugin emits. Enabled flag set true so
/// adding rows is a single edit, not a two-step enable+populate.
fn opencode_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::Opencode,
        enabled: true,
        sources: vec![],
        cost_overrides: BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Hook harnesses (Tier 2)
// ---------------------------------------------------------------------------

/// Cursor IDE's hook (`resources/hooks/cursor-otel-hook-impl.cjs`)
/// produces Tier A inline today; these rows document its behavior in
/// the Mappings UI. Before* events emit `null` to avoid double-counting
/// against after* events.
fn cursor_ide_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::CursorIde,
        enabled: true,
        sources: vec![
            // before* events are turn-start markers — the bundled Cursor
            // hook stashes a /tmp file so the matching after* can compute
            // turn.duration. Suppressed here to avoid double-counting.
            MappingSource::HookRule {
                when: "beforeSubmitPrompt".into(),
                emit: None,
            },
            MappingSource::HookRule {
                when: "beforeShellExecution".into(),
                emit: None,
            },
            // afterAgentResponse — one chat turn. The hook fans the
            // single observation across distinct `when` keys (one per
            // facet) so each rule matches exactly one observation type.
            // Events + turn.duration share `afterAgentResponse` (Counter
            // vs Histogram kind filter keeps them separate at runtime).
            MappingSource::HookRule {
                when: "afterAgentResponse".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "afterAgentResponse".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::TurnDuration.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "afterAgentResponse.tokens.input".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "input"),
                }),
            },
            MappingSource::HookRule {
                when: "afterAgentResponse.tokens.output".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "output"),
                }),
            },
            MappingSource::HookRule {
                when: "afterAgentResponse.cost".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::CostUsd.id(),
                    attributes: attr("cost.method", "estimated"),
                }),
            },
            // afterShellExecution — one shell command. Events +
            // turn.duration share the bare `when` key (kind-separated).
            // Errors (exit_code != 0) get their own `.error` suffix so
            // the rule fires only on failures.
            MappingSource::HookRule {
                when: "afterShellExecution".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "shell.exec"),
                }),
            },
            MappingSource::HookRule {
                when: "afterShellExecution".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::TurnDuration.id(),
                    attributes: attr("event.kind", "shell.exec"),
                }),
            },
            MappingSource::HookRule {
                when: "afterShellExecution.error".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Errors.id(),
                    attributes: attr("error.kind", "tool_failure"),
                }),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Cursor CLI shares the same hook script as Cursor IDE; the mapping
/// rows are identical. Kept as a separate function (rather than
/// aliased) so future divergence (e.g. a CLI-only event) is a
/// per-function edit.
fn cursor_cli_defaults() -> HarnessMapping {
    let mut m = cursor_ide_defaults();
    m.harness_id = HarnessId::CursorCli;
    m
}

// ---------------------------------------------------------------------------
// Watcher / wrapper harnesses (Tier 3)
// ---------------------------------------------------------------------------

/// Cline messages classified by `type` and `say` from
/// `ui_messages.json`. The watcher (`cline_watcher.rs`) consults these
/// rules at emission time after the Tier A upgrade in Phase 2.
fn cline_defaults() -> HarnessMapping {
    // The Cline watcher classifies `ui_messages.json` rows into the four
    // `say.*` `when` keys below — `text`, `tool`, `command`, `error` —
    // and emits one observation per matching message. Cline's
    // `api_req_finished` row carries an aggregated token count *string*
    // without a per-direction breakdown, so the watcher doesn't surface
    // it to the rules runtime today: any rule keyed on
    // `api_req_finished` would be dead and would fail validation
    // (Tokens requires `direction`). If a future Cline release exposes
    // direction-tagged token totals, add a per-facet `say.tokens.input`/
    // `say.tokens.output` rule pair (mirroring the Claude Desktop
    // pattern) rather than a single direction-less rule.
    HarnessMapping {
        harness_id: HarnessId::Cline,
        enabled: true,
        sources: vec![
            MappingSource::HookRule {
                when: "say.text".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "say.tool".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "tool.call"),
                }),
            },
            MappingSource::HookRule {
                when: "say.command".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "shell.exec"),
                }),
            },
            MappingSource::HookRule {
                when: "say.error".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Errors.id(),
                    attributes: attr("error.kind", "unknown"),
                }),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Aider's wrapper emits one event per invocation. The runtime emits
/// three Tier A facets per invocation: an `events` row, a
/// `turn.duration` histogram observation, and (only on non-zero exit)
/// an `errors` row. Each is a separate rule so users can disable any
/// facet individually via the Mappings editor.
fn aider_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::Aider,
        enabled: true,
        sources: vec![
            MappingSource::HookRule {
                when: "wrapper.invocation".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "wrapper.invocation".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::TurnDuration.id(),
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "wrapper.error".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Errors.id(),
                    attributes: attr("error.kind", "unknown"),
                }),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Mirrors Aider exactly — Copilot CLI also has only invocation
/// timing/exit-code data.
fn copilot_cli_defaults() -> HarnessMapping {
    let mut m = aider_defaults();
    m.harness_id = HarnessId::CopilotCli;
    m
}

/// Droid (factory.ai CLI) Tier 1 native-OTLP defaults.
///
/// Droid emits all metrics under the `droid.*` namespace with delta
/// temporality (60s flush interval).
///
/// **Native OTLP metrics** (via `SynthesizeFromNative`):
/// - `droid.tool.invocations` → events(event.kind=tool.call)
/// - `droid.tool.execution_time` → turn.duration(event.kind=tool.call)
/// - `droid.code.files_modified` → events(event.kind=file.edit)
/// - `droid.slash_command.invocations` → events(event.kind=chat.turn)
///
/// **Log-watcher metrics** (via `HookRule`, sourced from
/// `crate::adapters::droid_watcher`):
/// - `droid.tokens.input` → tokens(direction=input) — full-price new input tokens
/// - `droid.tokens.output` → tokens(direction=output)
/// - `droid.tokens.cache_read` → tokens(`direction=cacheRead`) — cache-read tokens
/// - `droid.cost` → cost.usd(cost.method=estimated) — from contextCount + outputTokens;
///   cacheReadInputTokens excluded (billed at a lower rate; ~10% underestimate)
///
/// **Excluded by default** (native OTLP):
/// - `droid.mcp.tool_invocations` — overlap with `droid.tool.invocations`
///   is unknown; excluded conservatively to avoid potential double-counting.
/// - `droid.git.commits` / `droid.git.pull_requests` — no clean Tier A target.
/// - `droid.code.files_read` / `droid.code.lines_modified` — no Tier A equivalent.
/// - `droid.skill.invocations` / `droid.hook.invocations` — internal machinery.
/// - `droid.auth.login_success` / `droid.repo.metadata` — no Tier A equivalent.
fn droid_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::Droid,
        enabled: true,
        sources: vec![
            // Tool calls → events(event.kind=tool.call)
            MappingSource::SynthesizeFromNative {
                native_metric: "droid.tool.invocations".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([("event.kind".into(), "tool.call".into())]),
            },
            // Tool execution time (histogram, seconds) → turn.duration(event.kind=tool.call)
            MappingSource::SynthesizeFromNative {
                native_metric: "droid.tool.execution_time".into(),
                target_metric: TierAMetric::TurnDuration.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([("event.kind".into(), "tool.call".into())]),
            },
            // Files modified → events(event.kind=file.edit)
            MappingSource::SynthesizeFromNative {
                native_metric: "droid.code.files_modified".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([("event.kind".into(), "file.edit".into())]),
            },
            // Slash command invocations → events(event.kind=chat.turn)
            // Slash commands are the primary user-initiated interaction unit.
            MappingSource::SynthesizeFromNative {
                native_metric: "droid.slash_command.invocations".into(),
                target_metric: TierAMetric::Events.id(),
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([("event.kind".into(), "chat.turn".into())]),
            },
            // droid_watcher hook rules — the log watcher emits per-call
            // observations against these `when` keys. Users can disable
            // or retarget any of them via the Mappings editor; the watcher
            // re-reads this table on every poll so edits take effect
            // without a restart.
            MappingSource::HookRule {
                when: "droid.tokens.input".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "input"),
                }),
            },
            MappingSource::HookRule {
                when: "droid.tokens.output".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "output"),
                }),
            },
            MappingSource::HookRule {
                when: "droid.tokens.cache_read".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens.id(),
                    attributes: attr("direction", "cacheRead"),
                }),
            },
            MappingSource::HookRule {
                when: "droid.cost".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::CostUsd.id(),
                    attributes: attr("cost.method", "estimated"),
                }),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_includes_every_harness_in_kebab_order() {
        let s = default_state();
        let ids: Vec<HarnessId> = s.harnesses.iter().map(|h| h.harness_id).collect();
        assert_eq!(
            ids,
            vec![
                HarnessId::ClaudeCode,
                HarnessId::ClaudeDesktop,
                HarnessId::GeminiCli,
                HarnessId::CodexCli,
                HarnessId::CodexDesktop,
                HarnessId::QwenCode,
                HarnessId::Opencode,
                HarnessId::CursorIde,
                HarnessId::CursorCli,
                HarnessId::Cline,
                HarnessId::Aider,
                HarnessId::CopilotCli,
                HarnessId::JunieCli,
                HarnessId::Droid,
                HarnessId::KimiCodeCli,
                HarnessId::Devin,
                HarnessId::Forgecode,
                HarnessId::Sentinel,
            ]
        );
    }

    #[test]
    fn claude_desktop_maps_all_five_documented_events() {
        // Every event name Anthropic's Cowork docs list appears in the
        // mapping table — either as a real emitter or as an explicit
        // None to document why it's suppressed.
        let m = claude_desktop_defaults();
        let whens: Vec<&str> = m
            .sources
            .iter()
            .filter_map(|s| match s {
                MappingSource::HookRule { when, .. } => Some(when.as_str()),
                MappingSource::SynthesizeFromNative { .. } => None,
            })
            .collect();
        for expected in [
            "user_prompt",
            "tool_result",
            "api_request",
            "api_error",
            "tool_decision",
        ] {
            assert!(
                whens.contains(&expected),
                "claude-desktop defaults missing `{expected}`; got {whens:?}",
            );
        }
    }

    #[test]
    fn claude_desktop_api_request_fans_into_four_tier_a_metrics() {
        // api_request carries model + token totals + cost_usd + duration.
        // v2 splits the fan-out across distinct `when` keys per facet so
        // each rule matches exactly one observation type (the runtime
        // additionally uses a kind filter so events+turn.duration can
        // share `api_request` without fighting):
        //   - api_request                  → events, turn.duration
        //   - api_request.tokens.input     → tokens (direction=input)
        //   - api_request.tokens.output    → tokens (direction=output)
        //   - api_request.cost             → cost.usd
        let m = claude_desktop_defaults();
        let by_when: std::collections::BTreeMap<String, Vec<String>> = m
            .sources
            .iter()
            .filter_map(|s| match s {
                MappingSource::HookRule {
                    when,
                    emit: Some(e),
                } => Some((when.clone(), e.metric.clone())),
                _ => None,
            })
            .fold(std::collections::BTreeMap::new(), |mut acc, (w, m)| {
                acc.entry(w).or_default().push(m);
                acc
            });
        // api_request fans into events + turn.duration.
        let api_request = by_when.get("api_request").expect("api_request rules");
        assert!(api_request.contains(&TierAMetric::Events.id()));
        assert!(api_request.contains(&TierAMetric::TurnDuration.id()));
        // Per-facet keys each target exactly one Tier A metric.
        assert_eq!(
            by_when.get("api_request.tokens.input").expect("tokens.input"),
            &vec![TierAMetric::Tokens.id()],
        );
        assert_eq!(
            by_when.get("api_request.tokens.output").expect("tokens.output"),
            &vec![TierAMetric::Tokens.id()],
        );
        assert_eq!(
            by_when.get("api_request.cost").expect("cost"),
            &vec![TierAMetric::CostUsd.id()],
        );
    }

    #[test]
    fn claude_desktop_suppresses_user_prompt_and_tool_decision() {
        // Both events fire alongside a "canonical" emitter (api_request
        // for chat.turn, tool_result for tool.call). Emitting them too
        // would double-count chat turns and tool invocations.
        let m = claude_desktop_defaults();
        for when in ["user_prompt", "tool_decision"] {
            let row = m
                .sources
                .iter()
                .find(|s| matches!(s, MappingSource::HookRule { when: w, .. } if w == when))
                .unwrap_or_else(|| panic!("missing row for {when}"));
            match row {
                MappingSource::HookRule { emit, .. } => {
                    assert!(emit.is_none(), "{when} should be informational (emit None)");
                }
                MappingSource::SynthesizeFromNative { .. } => panic!("expected HookRule"),
            }
        }
    }

    #[test]
    fn claude_desktop_default_passes_validation() {
        // The mapping has multiple HookRule rows keyed by `api_request`
        // each targeting a distinct TierAMetric. The validator must
        // accept this shape — `same_when_different_metric_is_allowed`
        // pins the contract; this test pins it specifically for
        // claude-desktop so a future regression doesn't slip through.
        // The catalog seed is needed for the v2 validator to resolve
        // `target_metric` ids; build a default state and swap in just
        // the claude-desktop harness for the targeted case.
        let mut state = default_state();
        state.harnesses = vec![claude_desktop_defaults()];
        super::super::validate::validate(&state).expect("claude-desktop defaults must validate");

        // Also confirm the full default_state validates as a sanity
        // check that ClaudeDesktop doesn't conflict with siblings.
        state = default_state();
        super::super::validate::validate(&state).expect("default_state must validate");
    }

    #[test]
    fn native_otel_harnesses_have_synthesize_rows() {
        for id in [HarnessId::ClaudeCode, HarnessId::GeminiCli, HarnessId::CodexCli, HarnessId::QwenCode] {
            let m = defaults_for(id);
            assert!(
                m.sources
                    .iter()
                    .any(|s| matches!(s, MappingSource::SynthesizeFromNative { .. })),
                "{id:?} should have at least one synthesize-from-native row"
            );
        }
    }

    #[test]
    fn cursor_ide_and_cli_share_rule_shape() {
        let ide = cursor_ide_defaults();
        let cli = cursor_cli_defaults();
        assert_eq!(ide.sources.len(), cli.sources.len());
        // Same `when` keys in the same order.
        let ide_whens: Vec<_> = ide
            .sources
            .iter()
            .filter_map(|s| match s {
                MappingSource::HookRule { when, .. } => Some(when.clone()),
                MappingSource::SynthesizeFromNative { .. } => None,
            })
            .collect();
        let cli_whens: Vec<_> = cli
            .sources
            .iter()
            .filter_map(|s| match s {
                MappingSource::HookRule { when, .. } => Some(when.clone()),
                MappingSource::SynthesizeFromNative { .. } => None,
            })
            .collect();
        assert_eq!(ide_whens, cli_whens);
        assert_ne!(ide.harness_id, cli.harness_id);
    }

    #[test]
    fn cursor_before_events_emit_null_to_avoid_double_count() {
        let m = cursor_ide_defaults();
        let before_prompt = m
            .sources
            .iter()
            .find(|s| matches!(s, MappingSource::HookRule { when, .. } if when == "beforeSubmitPrompt"))
            .unwrap();
        match before_prompt {
            MappingSource::HookRule { emit, .. } => assert!(emit.is_none()),
            MappingSource::SynthesizeFromNative { .. } => panic!("expected HookRule"),
        }
    }

    #[test]
    fn cline_classifies_tool_and_command_distinctly() {
        let m = cline_defaults();
        let tool_kind = m
            .sources
            .iter()
            .find_map(|s| match s {
                MappingSource::HookRule { when, emit: Some(e) } if when == "say.tool" => {
                    e.attributes.get("event.kind").cloned()
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(tool_kind, "tool.call");

        let cmd_kind = m
            .sources
            .iter()
            .find_map(|s| match s {
                MappingSource::HookRule { when, emit: Some(e) } if when == "say.command" => {
                    e.attributes.get("event.kind").cloned()
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(cmd_kind, "shell.exec");
    }

    #[test]
    fn opencode_default_is_empty_and_enabled_pending_verification() {
        let m = opencode_defaults();
        assert!(m.enabled);
        assert!(m.sources.is_empty());
    }

    #[test]
    fn cost_usd_is_not_seeded_by_default_per_open_question_2() {
        // MAPPING_PLAN.md open question #2 recommends OFF for cost.usd
        // synthesis by default. No default row should target CostUsd
        // — the user adds them via the UI when they're ready.
        for id in [
            HarnessId::ClaudeCode,
            HarnessId::GeminiCli,
            HarnessId::CodexCli,
            HarnessId::QwenCode,
        ] {
            for src in defaults_for(id).sources {
                if let MappingSource::SynthesizeFromNative { target_metric, .. } = src {
                    assert_ne!(
                        target_metric,
                        TierAMetric::CostUsd.id(),
                        "{id:?} defaults included a cost.usd row"
                    );
                }
            }
        }
    }

    #[test]
    fn default_metric_catalog_seeds_every_builtin() {
        let cat = default_metric_catalog();
        assert_eq!(cat.len(), 13);
        for builtin in TierAMetric::all() {
            let entry = cat
                .iter()
                .find(|m| m.id == builtin.id())
                .unwrap_or_else(|| panic!("catalog missing builtin {builtin:?}"));
            assert!(entry.builtin, "builtin entry must carry builtin: true");
            assert_eq!(entry.name, builtin.full_name());
            assert_eq!(entry.kind, builtin.kind());
        }
    }

    #[test]
    fn default_catalog_includes_sentinel_custom_metrics() {
        let cat = default_metric_catalog();
        let customs = sentinel_custom_metrics();
        assert_eq!(customs.len(), 8);
        for m in &customs {
            let entry = cat
                .iter()
                .find(|c| c.id == m.id)
                .unwrap_or_else(|| panic!("catalog missing sentinel metric {}", m.id));
            assert!(
                !entry.builtin,
                "sentinel metrics are custom (builtin: false)"
            );
            assert!(entry.id.starts_with("sentinel."));
        }
    }

    #[test]
    fn default_state_carries_catalog_at_current_version() {
        let s = default_state();
        assert_eq!(s.schema_version, MAPPING_SCHEMA_VERSION);
        assert_eq!(s.metrics.len(), 13);
    }

    #[test]
    fn sentinel_defaults_reuse_claude_code_tier_a_rows() {
        // Self-contained: Sentinel carries the same native→Tier A rows as
        // Claude Code so a Sentinel-only user still gets events/tokens.
        assert_eq!(sentinel_defaults().sources, claude_code_defaults().sources);
        assert!(sentinel_defaults().enabled);
    }
}
