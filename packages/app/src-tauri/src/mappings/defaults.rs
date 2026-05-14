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

use super::{HarnessMapping, HookEmit, MappingSource, MappingState, TierAMetric, MAPPING_SCHEMA_VERSION};
use crate::harness::HarnessId;

/// The full default mapping state — every supported harness, in tier
/// order to match the UI sort.
#[must_use]
pub fn default_state() -> MappingState {
    MappingState {
        schema_version: MAPPING_SCHEMA_VERSION,
        harnesses: vec![
            defaults_for(HarnessId::ClaudeCode),
            defaults_for(HarnessId::ClaudeDesktop),
            defaults_for(HarnessId::GeminiCli),
            defaults_for(HarnessId::CodexCli),
            defaults_for(HarnessId::QwenCode),
            defaults_for(HarnessId::Opencode),
            defaults_for(HarnessId::CursorIde),
            defaults_for(HarnessId::CursorCli),
            defaults_for(HarnessId::Cline),
            defaults_for(HarnessId::Aider),
            defaults_for(HarnessId::CopilotCli),
        ],
    }
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
        HarnessId::QwenCode => qwen_code_defaults(),
        HarnessId::Opencode => opencode_defaults(),
        HarnessId::CursorIde => cursor_ide_defaults(),
        HarnessId::CursorCli => cursor_cli_defaults(),
        HarnessId::Cline => cline_defaults(),
        HarnessId::Aider => aider_defaults(),
        HarnessId::CopilotCli => copilot_cli_defaults(),
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
fn claude_code_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::ClaudeCode,
        enabled: true,
        sources: vec![
            MappingSource::SynthesizeFromNative {
                native_metric: "claude_code.session.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "claude_code.tool.decision.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "tool.call".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "claude_code.token.usage".into(),
                target_metric: TierAMetric::Tokens,
                // Claude Code's native attribute is `type=input|output`;
                // Tier A wants `direction`. The collector transform
                // rewrites the attribute key inline.
                attribute_map: BTreeMap::from([("type".into(), "direction".into())]),
                inject_attributes: BTreeMap::new(),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
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
                    metric: TierAMetric::Events,
                    attributes: attr("event.kind", "tool.call"),
                }),
            },
            // `api_request` — fires on every successful Claude API call.
            // Documented attributes: model, cost_usd, duration_ms,
            // input_tokens, output_tokens, cache_read_tokens,
            // cache_creation_tokens, speed. Fans out into four Tier A
            // metrics:
            //
            //   1. events (event.kind=chat.turn) — one chat turn per
            //      successful api_request.
            //   2. tokens — the connector inspects input_tokens /
            //      output_tokens / cache_* and emits one Tier A row
            //      per direction (direction=input/output). The
            //      validator allows multiple HookRule rows for the
            //      same `when` as long as each targets a distinct
            //      TierAMetric, so all four lines below coexist.
            //   3. cost.usd (cost.method=exact) — Anthropic computes
            //      the per-call cost server-side, so the Tier A
            //      cost.method is `exact` (not the estimated rate-card
            //      math Trove does for other harnesses).
            //   4. turn.duration — duration_ms / 1000 → seconds.
            MappingSource::HookRule {
                when: "api_request".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events,
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "api_request".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens,
                    attributes: BTreeMap::new(),
                }),
            },
            MappingSource::HookRule {
                when: "api_request".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::CostUsd,
                    attributes: attr("cost.method", "exact"),
                }),
            },
            MappingSource::HookRule {
                when: "api_request".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::TurnDuration,
                    attributes: attr("event.kind", "chat.turn"),
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
                    metric: TierAMetric::Errors,
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
                target_metric: TierAMetric::Events,
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
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "tool.call".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.token.usage".into(),
                target_metric: TierAMetric::Tokens,
                attribute_map: BTreeMap::from([
                    ("gen_ai.token.type".into(), "direction".into()),
                    ("gen_ai.request.model".into(), "model".into()),
                ]),
                inject_attributes: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.operation.duration".into(),
                target_metric: TierAMetric::TurnDuration,
                attribute_map: BTreeMap::from([
                    ("gen_ai.request.model".into(), "model".into()),
                ]),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gemini_cli.model_routing.failure.count".into(),
                target_metric: TierAMetric::Errors,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("error.kind".into(), "routing".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gemini_cli.chat.content_retry_failure.count".into(),
                target_metric: TierAMetric::Errors,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("error.kind".into(), "retry".into()),
                ]),
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
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "codex.tool.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "tool.call".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.token.usage".into(),
                target_metric: TierAMetric::Tokens,
                attribute_map: BTreeMap::from([
                    ("gen_ai.token.type".into(), "direction".into()),
                    ("gen_ai.request.model".into(), "model".into()),
                ]),
                inject_attributes: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.operation.duration".into(),
                target_metric: TierAMetric::TurnDuration,
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
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "chat.turn".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "qwen_code.tool.call.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
                inject_attributes: BTreeMap::from([
                    ("event.kind".into(), "tool.call".into()),
                ]),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "qwen_code.token.usage".into(),
                target_metric: TierAMetric::Tokens,
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
            MappingSource::HookRule {
                when: "beforeSubmitPrompt".into(),
                emit: None,
            },
            MappingSource::HookRule {
                when: "afterAgentResponse".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events,
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "beforeShellExecution".into(),
                emit: None,
            },
            MappingSource::HookRule {
                when: "afterShellExecution".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events,
                    attributes: attr("event.kind", "shell.exec"),
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
    HarnessMapping {
        harness_id: HarnessId::Cline,
        enabled: true,
        sources: vec![
            MappingSource::HookRule {
                when: "say.text".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events,
                    attributes: attr("event.kind", "chat.turn"),
                }),
            },
            MappingSource::HookRule {
                when: "say.tool".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events,
                    attributes: attr("event.kind", "tool.call"),
                }),
            },
            MappingSource::HookRule {
                when: "say.command".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Events,
                    attributes: attr("event.kind", "shell.exec"),
                }),
            },
            MappingSource::HookRule {
                when: "say.error".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Errors,
                    attributes: attr("error.kind", "unknown"),
                }),
            },
            MappingSource::HookRule {
                when: "api_req_finished".into(),
                emit: Some(HookEmit {
                    metric: TierAMetric::Tokens,
                    attributes: BTreeMap::new(),
                }),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Aider's wrapper emits one event per invocation; the only timing
/// data is the wrapper's start/end timestamps. Tokens/cost aren't
/// available without scraping Aider's chat-history files (future PR).
fn aider_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::Aider,
        enabled: true,
        sources: vec![MappingSource::HookRule {
            when: "wrapper.invocation".into(),
            emit: Some(HookEmit {
                metric: TierAMetric::Events,
                attributes: attr("event.kind", "chat.turn"),
            }),
        }],
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
                HarnessId::QwenCode,
                HarnessId::Opencode,
                HarnessId::CursorIde,
                HarnessId::CursorCli,
                HarnessId::Cline,
                HarnessId::Aider,
                HarnessId::CopilotCli,
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
        // api_request carries model + token totals + cost_usd + duration
        // — verify the four-row fan-out one-by-one. The validator
        // enforces (when, metric) uniqueness, so collision-by-mistake
        // is caught at apply time.
        let m = claude_desktop_defaults();
        let emitted_metrics: Vec<TierAMetric> = m
            .sources
            .iter()
            .filter_map(|s| match s {
                MappingSource::HookRule {
                    when,
                    emit: Some(e),
                } if when == "api_request" => Some(e.metric),
                _ => None,
            })
            .collect();
        for expected in [
            TierAMetric::Events,
            TierAMetric::Tokens,
            TierAMetric::CostUsd,
            TierAMetric::TurnDuration,
        ] {
            assert!(
                emitted_metrics.contains(&expected),
                "api_request should fan into {expected:?}",
            );
        }
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
        let mut state = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![claude_desktop_defaults()],
        };
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
                        TierAMetric::CostUsd,
                        "{id:?} defaults included a cost.usd row"
                    );
                }
            }
        }
    }
}
