//! Serialize the Trove mapping state into the JSON shape the bundled
//! Cursor hook script (`resources/hooks/cursor-otel-hook-impl.cjs`)
//! reads at run time.
//!
//! Cursor's hook runs out-of-process — every event spawns a fresh Node
//! subprocess — so the hook can't `app.try_state::<MappingStateStore>()`
//! the way an in-process watcher can. Instead, every `apply_mappings`
//! IPC call writes a fresh `~/.cursor/trove-hook-rules.json` sidecar
//! summarizing the resolved cursor harness's rules and catalog entries.
//! The Node script reads this file on each invocation and routes
//! emissions according to it.
//!
//! On hosts where the sidecar is missing (first-launch race, upgrade
//! from a v1 build before the sidecar existed), the script falls back
//! to its built-in defaults — identical Tier A behavior to today.

use serde::Serialize;

use crate::harness::HarnessId;
use crate::mappings::{MappingSource, MappingState};

/// Wire shape the JS hook script consumes. Versioned so a future
/// schema bump can move forward without breaking older scripts on the
/// host (e.g. user hasn't upgraded the bundled cjs yet).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookSidecar {
    pub schema_version: u32,
    pub metrics: Vec<HookMetricDef>,
    /// Rules keyed by Cursor's raw event names (`beforeSubmitPrompt`,
    /// `afterAgentResponse`, `beforeShellExecution`, `afterShellExecution`).
    /// One vec entry per rule; the JS script applies all matching
    /// entries when an event fires.
    pub rules: Vec<HookRule>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookMetricDef {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub required_attributes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookRule {
    pub when: String,
    /// `None` (serializes as `null`) means "suppress emission" — used by
    /// before-event rules to avoid double-counting.
    pub emit: Option<HookEmitWire>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookEmitWire {
    /// Catalog metric id; the JS script looks this up in `metrics` to
    /// get the wire name and kind.
    pub metric: String,
    pub attributes: std::collections::BTreeMap<String, String>,
}

/// Schema version of the sidecar wire format. Bump if the JS script's
/// expected fields change. The script must check `schemaVersion <= N`
/// and fall back to defaults on mismatch.
pub const SIDECAR_SCHEMA_VERSION: u32 = 1;

/// Project the [`MappingState`] down to the slice the Cursor hook
/// needs: the metric catalog (so the script can resolve target ids to
/// wire names + kinds) and the rules for both Cursor harness ids.
///
/// Both Cursor IDE and Cursor CLI share one hooks.json and one hook
/// script. We merge their rule lists; in practice the user has one or
/// the other enabled and they share the same `when` namespace, so
/// duplicates would only appear if the user has different rules for
/// IDE vs CLI. We dedupe by `(when, emit)` so duplicate rules don't
/// double-emit.
#[must_use]
pub fn serialize_for_hook(state: &MappingState) -> HookSidecar {
    // Cursor IDE + Cursor CLI share one hooks.json and one hook script.
    serialize_for_hook_ids(state, &[HarnessId::CursorIde, HarnessId::CursorCli])
}

/// Like [`serialize_for_hook`] but for an arbitrary set of hook-based
/// harness ids. The bundled hook sidecar shape is identical across
/// hook harnesses (Cursor, Antigravity), so the Antigravity adapter
/// reuses this with its single id; the Cursor pair uses the wrapper
/// above. Rules are deduped by `(when, emit)` across the given ids.
#[must_use]
pub fn serialize_for_hook_ids(state: &MappingState, harness_ids: &[HarnessId]) -> HookSidecar {
    let metrics: Vec<HookMetricDef> = state
        .metrics
        .iter()
        .map(|m| HookMetricDef {
            id: m.id.clone(),
            name: m.name.clone(),
            kind: match m.kind {
                crate::mappings::TroveMetricKind::Counter => "counter".to_string(),
                crate::mappings::TroveMetricKind::Gauge => "gauge".to_string(),
                crate::mappings::TroveMetricKind::Histogram => "histogram".to_string(),
            },
            required_attributes: m.required_attributes.clone(),
        })
        .collect();

    let mut rules: Vec<HookRule> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for &harness_id in harness_ids {
        let Some(harness) = state.for_harness(harness_id) else {
            continue;
        };
        if !harness.enabled {
            continue;
        }
        for source in &harness.sources {
            let MappingSource::HookRule { when, emit } = source else {
                continue;
            };
            // Dedup key: when + serialized emit. Same shape across IDE
            // and CLI should collapse to one rule for the script.
            let emit_serialized =
                serde_json::to_string(emit).unwrap_or_else(|_| "null".to_string());
            let key = format!("{when}\x00{emit_serialized}");
            if !seen.insert(key) {
                continue;
            }
            rules.push(HookRule {
                when: when.clone(),
                emit: emit.as_ref().map(|e| HookEmitWire {
                    metric: e.metric.clone(),
                    attributes: e.attributes.clone(),
                }),
            });
        }
    }

    HookSidecar {
        schema_version: SIDECAR_SCHEMA_VERSION,
        metrics,
        rules,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::mappings::{
        default_state, HarnessMapping, HookEmit, MappingSource, TroveMetricDefinition,
        TroveMetricKind,
    };

    use super::*;

    #[test]
    fn serialize_for_hook_includes_builtin_catalog() {
        let snap = serialize_for_hook(&default_state());
        assert_eq!(snap.schema_version, SIDECAR_SCHEMA_VERSION);
        assert!(snap.metrics.iter().any(|m| m.id == "events"));
        assert!(snap.metrics.iter().any(|m| m.id == "turn.duration"));
    }

    #[test]
    fn serialize_for_hook_includes_cursor_ide_rules() {
        let snap = serialize_for_hook(&default_state());
        let whens: Vec<&str> = snap.rules.iter().map(|r| r.when.as_str()).collect();
        assert!(whens.contains(&"afterAgentResponse"));
        assert!(whens.contains(&"beforeSubmitPrompt"));
    }

    #[test]
    fn serialize_for_hook_dedupes_ide_and_cli_identical_rules() {
        // Both cursor harnesses default to the same rule shape; the
        // serialized output should not double them.
        let snap = serialize_for_hook(&default_state());
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for r in &snap.rules {
            let key = format!(
                "{}|{}",
                r.when,
                r.emit.as_ref().map_or("null", |e| e.metric.as_str()),
            );
            *counts.entry(key).or_insert(0) += 1;
        }
        for (key, n) in &counts {
            assert_eq!(*n, 1, "duplicate rule {key}: count {n}");
        }
    }

    #[test]
    fn serialize_for_hook_skips_disabled_harnesses() {
        let mut state = default_state();
        for h in &mut state.harnesses {
            if h.harness_id == HarnessId::CursorIde || h.harness_id == HarnessId::CursorCli {
                h.enabled = false;
            }
        }
        let snap = serialize_for_hook(&state);
        assert!(snap.rules.is_empty());
    }

    #[test]
    fn serialize_for_hook_carries_custom_metric_in_catalog() {
        let mut state = default_state();
        state.metrics.push(TroveMetricDefinition {
            id: "shell_runs".to_string(),
            name: "my.team.shell_runs".to_string(),
            kind: TroveMetricKind::Counter,
            description: String::new(),
            required_attributes: vec!["shell".to_string()],
            builtin: false,
        });
        // Point Cursor IDE at the custom metric for after-shell events.
        state.harnesses.retain(|h| h.harness_id != HarnessId::CursorIde);
        state.harnesses.push(HarnessMapping {
            harness_id: HarnessId::CursorIde,
            enabled: true,
            sources: vec![MappingSource::HookRule {
                when: "afterShellExecution".to_string(),
                emit: Some(HookEmit {
                    metric: "shell_runs".to_string(),
                    attributes: BTreeMap::new(),
                }),
            }],
            cost_overrides: BTreeMap::new(),
        });
        let snap = serialize_for_hook(&state);
        assert!(snap.metrics.iter().any(|m| m.id == "shell_runs" && m.name == "my.team.shell_runs"));
        let custom_rule = snap
            .rules
            .iter()
            .find(|r| r.when == "afterShellExecution")
            .unwrap();
        assert_eq!(
            custom_rule
                .emit
                .as_ref()
                .map(|e| e.metric.as_str()),
            Some("shell_runs"),
        );
    }
}
