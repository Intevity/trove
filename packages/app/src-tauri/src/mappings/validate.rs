//! Validate a [`MappingState`] before persisting or regenerating the
//! collector config.
//!
//! Catches:
//!
//! - Attribute values outside the Tier A enum domain (e.g.
//!   `event.kind = "frobulate"` would silently break the dashboard
//!   panels that filter on the enum).
//! - Two enabled `HookRule` rows for the same harness emitting on the
//!   same `when` event (would double-count the metric).
//! - Mapping schema-version mismatches (forward-compat trap; users on
//!   older Trove builds shouldn't silently accept newer state.json).
//!
//! Per `documentation/MAPPING_PLAN.md` §"Persistence and migration"
//! (lines 252–261).

use std::collections::HashSet;

use thiserror::Error;

use super::{MappingSource, MappingState, TierAMetric, MAPPING_SCHEMA_VERSION};
use crate::harness::HarnessId;

/// The set of allowed values for each well-known attribute key. Lookup
/// returns `None` for keys whose value is free-form (e.g. `model`,
/// `direction` has a domain but `model` does not).
fn allowed_values(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "event.kind" => Some(&[
            "chat.turn",
            "tool.call",
            "shell.exec",
            "file.edit",
            "session.start",
            "session.end",
        ]),
        "direction" => Some(&["input", "output"]),
        "cost.method" => Some(&["exact", "estimated"]),
        "error.kind" => Some(&[
            "rate_limit",
            "auth",
            "tool_failure",
            "network",
            "policy",
            "unknown",
        ]),
        _ => None,
    }
}

/// Validation errors. Each variant carries enough context to point a
/// user at the offending row in the UI.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("unknown mapping schema version {0}; expected {MAPPING_SCHEMA_VERSION}")]
    UnknownSchemaVersion(u32),
    #[error("harness {harness:?}: attribute {key}={value:?} is not in the allowed set {allowed:?}")]
    AttributeOutOfDomain {
        harness: HarnessId,
        key: String,
        value: String,
        allowed: Vec<String>,
    },
    #[error(
        "harness {harness:?}: two enabled hook-rule rows fire on the same event {when:?} \
         — would double-count {metric:?}. Disable one or merge them."
    )]
    DoubleEmit {
        harness: HarnessId,
        when: String,
        metric: TierAMetric,
    },
    #[error("harness {harness:?}: duplicate entries — only one HarnessMapping allowed per id")]
    DuplicateHarness { harness: HarnessId },
}

/// Validate `state`. Returns `Ok(())` when the state is internally
/// consistent and safe to persist. Errors describe the first problem
/// found; fix-and-revalidate is the expected UX.
pub fn validate(state: &MappingState) -> Result<(), ValidationError> {
    if state.schema_version != MAPPING_SCHEMA_VERSION {
        return Err(ValidationError::UnknownSchemaVersion(state.schema_version));
    }

    let mut seen_harnesses: HashSet<HarnessId> = HashSet::new();
    for mapping in &state.harnesses {
        if !seen_harnesses.insert(mapping.harness_id) {
            return Err(ValidationError::DuplicateHarness {
                harness: mapping.harness_id,
            });
        }

        // Track enabled hook-rule `when` events per metric to catch
        // double-emit. We key on (when, metric) so a hook event firing
        // on multiple distinct metrics is allowed — a single event
        // legitimately can contribute to `events` *and* `tokens` (one
        // chat turn = one event row + several token rows).
        let mut seen_when_metric: HashSet<(String, TierAMetric)> = HashSet::new();

        for source in &mapping.sources {
            match source {
                MappingSource::HookRule { when, emit } => {
                    if let Some(e) = emit {
                        // Attribute-domain check
                        for (k, v) in &e.attributes {
                            if let Some(allowed) = allowed_values(k) {
                                if !allowed.contains(&v.as_str()) {
                                    return Err(ValidationError::AttributeOutOfDomain {
                                        harness: mapping.harness_id,
                                        key: k.clone(),
                                        value: v.clone(),
                                        allowed: allowed
                                            .iter()
                                            .map(|s| (*s).to_string())
                                            .collect(),
                                    });
                                }
                            }
                        }
                        // Double-emit only matters when the harness is
                        // enabled; a disabled harness's rows are
                        // dormant. Skip the check entirely when off.
                        if mapping.enabled
                            && !seen_when_metric.insert((when.clone(), e.metric))
                        {
                            return Err(ValidationError::DoubleEmit {
                                harness: mapping.harness_id,
                                when: when.clone(),
                                metric: e.metric,
                            });
                        }
                    }
                }
                MappingSource::SynthesizeFromNative { attribute_map, .. } => {
                    // The attribute_map's *values* are Tier A keys; we
                    // don't have a value to domain-check here (the
                    // collector transform copies the native value
                    // through). Reserved for a future check that the
                    // target key matches the target metric's required
                    // attributes — out of scope for v1.
                    let _ = attribute_map;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::{HarnessMapping, HookEmit};
    use super::*;

    fn empty_state() -> MappingState {
        MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![],
        }
    }

    #[test]
    fn empty_state_is_valid() {
        assert!(validate(&empty_state()).is_ok());
    }

    #[test]
    fn default_state_is_valid() {
        assert!(validate(&super::super::default_state()).is_ok());
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let mut s = empty_state();
        s.schema_version = 99;
        assert!(matches!(
            validate(&s),
            Err(ValidationError::UnknownSchemaVersion(99))
        ));
    }

    #[test]
    fn duplicate_harness_id_is_rejected() {
        let s = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![
                HarnessMapping {
                    harness_id: HarnessId::ClaudeCode,
                    enabled: true,
                    sources: vec![],
                    cost_overrides: BTreeMap::new(),
                },
                HarnessMapping {
                    harness_id: HarnessId::ClaudeCode,
                    enabled: true,
                    sources: vec![],
                    cost_overrides: BTreeMap::new(),
                },
            ],
        };
        assert!(matches!(
            validate(&s),
            Err(ValidationError::DuplicateHarness {
                harness: HarnessId::ClaudeCode
            })
        ));
    }

    #[test]
    fn out_of_domain_event_kind_is_rejected() {
        let s = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::Aider,
                enabled: true,
                sources: vec![MappingSource::HookRule {
                    when: "wrapper.invocation".into(),
                    emit: Some(HookEmit {
                        metric: TierAMetric::Events,
                        attributes: BTreeMap::from([(
                            "event.kind".into(),
                            "frobulate".into(),
                        )]),
                    }),
                }],
                cost_overrides: BTreeMap::new(),
            }],
        };
        match validate(&s).unwrap_err() {
            ValidationError::AttributeOutOfDomain { key, value, .. } => {
                assert_eq!(key, "event.kind");
                assert_eq!(value, "frobulate");
            }
            other => panic!("expected AttributeOutOfDomain, got {other:?}"),
        }
    }

    #[test]
    fn out_of_domain_error_kind_is_rejected() {
        let s = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::Cline,
                enabled: true,
                sources: vec![MappingSource::HookRule {
                    when: "say.error".into(),
                    emit: Some(HookEmit {
                        metric: TierAMetric::Errors,
                        attributes: BTreeMap::from([(
                            "error.kind".into(),
                            "kaboom".into(),
                        )]),
                    }),
                }],
                cost_overrides: BTreeMap::new(),
            }],
        };
        assert!(matches!(
            validate(&s),
            Err(ValidationError::AttributeOutOfDomain { .. })
        ));
    }

    #[test]
    fn double_emit_on_same_when_and_metric_is_rejected() {
        let s = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::CursorIde,
                enabled: true,
                sources: vec![
                    MappingSource::HookRule {
                        when: "afterAgentResponse".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::Events,
                            attributes: BTreeMap::from([(
                                "event.kind".into(),
                                "chat.turn".into(),
                            )]),
                        }),
                    },
                    MappingSource::HookRule {
                        when: "afterAgentResponse".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::Events,
                            attributes: BTreeMap::from([(
                                "event.kind".into(),
                                "tool.call".into(),
                            )]),
                        }),
                    },
                ],
                cost_overrides: BTreeMap::new(),
            }],
        };
        assert!(matches!(validate(&s), Err(ValidationError::DoubleEmit { .. })));
    }

    #[test]
    fn double_emit_check_is_skipped_when_harness_disabled() {
        // The same conflicting rows pass validation when the harness
        // is off — disabled rows are dormant configuration data.
        let s = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::CursorIde,
                enabled: false,
                sources: vec![
                    MappingSource::HookRule {
                        when: "afterAgentResponse".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::Events,
                            attributes: BTreeMap::from([(
                                "event.kind".into(),
                                "chat.turn".into(),
                            )]),
                        }),
                    },
                    MappingSource::HookRule {
                        when: "afterAgentResponse".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::Events,
                            attributes: BTreeMap::from([(
                                "event.kind".into(),
                                "tool.call".into(),
                            )]),
                        }),
                    },
                ],
                cost_overrides: BTreeMap::new(),
            }],
        };
        assert!(validate(&s).is_ok());
    }

    #[test]
    fn same_when_different_metric_is_allowed() {
        // One Cline `api_req_finished` legitimately fires both a tokens
        // row and (in future) a cost.usd row. Validation must not
        // reject this.
        let s = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::Cline,
                enabled: true,
                sources: vec![
                    MappingSource::HookRule {
                        when: "api_req_finished".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::Tokens,
                            attributes: BTreeMap::new(),
                        }),
                    },
                    MappingSource::HookRule {
                        when: "api_req_finished".into(),
                        emit: Some(HookEmit {
                            metric: TierAMetric::CostUsd,
                            attributes: BTreeMap::from([(
                                "cost.method".into(),
                                "exact".into(),
                            )]),
                        }),
                    },
                ],
                cost_overrides: BTreeMap::new(),
            }],
        };
        assert!(validate(&s).is_ok());
    }

    #[test]
    fn unknown_attribute_keys_are_silently_allowed() {
        // `model` and other free-form keys aren't part of the enum
        // domain check — only the closed enums are validated.
        let s = MappingState {
            schema_version: MAPPING_SCHEMA_VERSION,
            harnesses: vec![HarnessMapping {
                harness_id: HarnessId::ClaudeCode,
                enabled: true,
                sources: vec![MappingSource::HookRule {
                    when: "wrapper.invocation".into(),
                    emit: Some(HookEmit {
                        metric: TierAMetric::Tokens,
                        attributes: BTreeMap::from([(
                            "model".into(),
                            "claude-anything-the-user-likes".into(),
                        )]),
                    }),
                }],
                cost_overrides: BTreeMap::new(),
            }],
        };
        assert!(validate(&s).is_ok());
    }
}
