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
fn claude_code_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::ClaudeCode,
        enabled: true,
        sources: vec![
            MappingSource::SynthesizeFromNative {
                native_metric: "claude_code.session.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "claude_code.tool.decision.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "claude_code.token.usage".into(),
                target_metric: TierAMetric::Tokens,
                // Claude Code's native attribute is `type=input|output`;
                // Tier A wants `direction`. The collector transform
                // rewrites the attribute key inline.
                attribute_map: BTreeMap::from([("type".into(), "direction".into())]),
            },
        ],
        cost_overrides: BTreeMap::new(),
    }
}

/// Gemini CLI ≥0.41 emits `gemini_cli.api.request.count` per chat
/// turn (confirmed against a live capture). Token usage rides under
/// the OTel-semconv `gen_ai.client.token.usage` gauge with
/// `gen_ai.token.type=input|output`; tool use lands in
/// `gemini_cli.tool.call.count`. The earlier defaults' `session.count`
/// name doesn't match what upstream actually emits — leave it out so
/// no rule shadow-matches and silently emits nothing.
fn gemini_cli_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::GeminiCli,
        enabled: true,
        sources: vec![
            MappingSource::SynthesizeFromNative {
                native_metric: "gemini_cli.api.request.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gemini_cli.tool.call.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.token.usage".into(),
                target_metric: TierAMetric::Tokens,
                attribute_map: BTreeMap::from([(
                    "gen_ai.token.type".into(),
                    "direction".into(),
                )]),
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
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "codex.tool.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "gen_ai.client.token.usage".into(),
                target_metric: TierAMetric::Tokens,
                attribute_map: BTreeMap::from([(
                    "gen_ai.token.type".into(),
                    "direction".into(),
                )]),
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
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "qwen_code.tool.call.count".into(),
                target_metric: TierAMetric::Events,
                attribute_map: BTreeMap::new(),
            },
            MappingSource::SynthesizeFromNative {
                native_metric: "qwen_code.token.usage".into(),
                target_metric: TierAMetric::Tokens,
                attribute_map: BTreeMap::from([(
                    "type".into(),
                    "direction".into(),
                )]),
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
