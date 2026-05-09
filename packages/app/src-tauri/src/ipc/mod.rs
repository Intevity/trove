//! Tauri command surface and the wire-format error type.
//!
//! Every command returns `Result<T, IpcError>` where `IpcError` is
//! serialized as a tagged JSON discriminated union. The React side has
//! a Zod twin in `packages/shared/src/schemas.ts` so `kind`-based
//! branching stays type-safe across the wire.

pub mod collector_status;
pub mod commands;
pub mod test_export;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::adapters::{ApplyOptions, TrovePatch};
use crate::harness::HarnessId;
use crate::safety::sentinels::Format;

/// All errors the IPC layer surfaces to the UI. Discriminated by `kind`
/// in JSON; the TS side has a matching Zod union.
#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum IpcError {
    /// Host config file exists but cannot be parsed in its expected
    /// format. The UI shows the `reason` and disables apply.
    #[error("config at {path} could not be parsed: {reason}")]
    ConfigUnparseable { path: String, reason: String },

    /// A managed Trove region exists in the host file but its hash
    /// differs from the region we'd write, AND no `StoredPatchMetadata`
    /// is available to populate a full 3-way payload. The orphan-block
    /// path (state.json wiped or never written) lands here; Sprint 8's
    /// resolver UI degrades to the 2-pane mode based on this variant.
    #[error("existing managed region in {path} differs from the new patch; refusing to overwrite")]
    RegionConflict { path: String },

    /// Sprint 8: a managed Trove region exists in the host file with a
    /// different hash, and `state.json` carries the metadata we need to
    /// surface a full 3-way merge to the user. The React resolver UI
    /// renders `conflict.original_region_payload` /
    /// `conflict.current_region_payload` /
    /// `conflict.theirs_region_payload` as three panes and offers
    /// `Keep mine` / `Take Trove's` / `Merge manually` actions, each of
    /// which round-trips through the `resolve_conflict` IPC command.
    ///
    /// The payload is `Box`ed so adding this variant doesn't bloat
    /// every `Result<T, IpcError>` in the codebase — `ConflictPayload`
    /// holds two whole-file strings, easily over 100 bytes; clippy's
    /// `result_large_err` triggers without the indirection.
    #[error("3-way conflict detected in {}; surface resolver UI", conflict.config_path)]
    RegionConflictDetected { conflict: Box<ConflictPayload> },

    /// Caller asked for a harness that wasn't detected on this machine.
    #[error("harness {id:?} was not detected on this machine")]
    HarnessNotDetected { id: HarnessId },

    /// Caller asked for a harness whose adapter is not yet implemented
    /// (codex-cli / qwen-code in Sprint 3 — those land in Sprint 4).
    #[error("harness {id:?} does not yet have an adapter implementation")]
    HarnessNotImplemented { id: HarnessId },

    /// Generic filesystem failure. `path` may be empty when no specific
    /// file is involved.
    #[error("io error at {path}: {reason}")]
    Io { path: String, reason: String },

    /// Sprint 10 — `check_for_updates` failed. Causes range from network
    /// errors to a malformed `latest.json` to a signature mismatch
    /// against the embedded pubkey. The UI surfaces `reason` verbatim.
    #[error("updater check failed: {reason}")]
    UpdaterCheckFailed { reason: String },

    /// Catch-all for unexpected failures the UI should treat as a bug.
    #[error("internal error: {reason}")]
    Internal { reason: String },
}

/// Sprint 8 conflict payload. Returned to the React resolver inside an
/// [`IpcError::RegionConflictDetected`] when a re-apply against a
/// hand-edited host config can be resolved with a 3-way merge.
///
/// `original_region_payload` is `None` for the orphan-block path —
/// state.json had no record of this harness, so Trove has no
/// "what we last wrote" baseline. The resolver renders 2 panes
/// (Yours / Trove's) in that mode.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictPayload {
    pub config_path: String,
    pub format: Format,
    pub original_region_payload: Option<String>,
    pub current_region_payload: String,
    pub theirs_region_payload: String,
    /// The host config's full text right now. Drives the resolver's
    /// "Yours" pane diff overlay.
    pub file_before: String,
    /// The full host config Trove would write if the user clicks
    /// "Take Trove's". Pre-rendered server-side so the UI never has to
    /// re-run `upsert_region`.
    pub file_after_if_taking_theirs: String,
}

/// What the resolver does when the user clicks one of the three
/// buttons. Tagged union over the IPC; each variant maps to a distinct
/// server-side handler in `resolve_conflict`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ConflictAction {
    /// Accept the user's hand-edits as the new baseline. Re-stamp
    /// `state.json` with the current region's hash + payload; do not
    /// touch the host file.
    KeepMine,
    /// Overwrite the host file with Trove's intended region. A backup
    /// fires before the write; the resolution returns the new
    /// `TrovePatch` for `state.json`.
    TakeTheirs { options: ApplyOptions },
    /// Drop sibling files (`<host>.trove.original`, `<host>.trove.theirs`)
    /// next to the host config and signal the UI to open the host file
    /// in the OS default editor. The user re-applies after merging.
    MergeManually { options: ApplyOptions },
}

/// What `resolve_conflict` returns to the UI on success. Discriminated
/// by `status` on the wire so the resolver branches cleanly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ConflictResolutionOutcome {
    /// `TakeTheirs` succeeded; the host file now matches Trove's
    /// intended content and `patch` is the new `state.json` record.
    Applied { patch: TrovePatch },
    /// `KeepMine` succeeded; the host file is unchanged and the new
    /// `patch` re-baselines `state.json` against the user's content.
    MarkedMine { patch: TrovePatch },
    /// `MergeManually` wrote sibling files and asked the OS to open
    /// the host config. The UI surfaces a "I've finished merging,
    /// re-apply" CTA driven by these paths.
    #[serde(rename_all = "camelCase")]
    MergeDeferred { sibling_paths: SiblingPaths },
}

/// Absolute paths of the sibling files dropped by `MergeManually`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiblingPaths {
    /// `<host>.trove.original` — what Trove last wrote (or empty for
    /// the orphan-block path). The user merges from this baseline.
    pub original: String,
    /// `<host>.trove.theirs` — the whole-file Trove would write right
    /// now. The user copies/pastes from here when manually merging.
    pub theirs: String,
    /// `<host>` — the host config the user is asked to edit. Returned
    /// so the UI can open it via `tauri-plugin-shell`.
    pub host: String,
}

impl From<crate::secrets::SecretsError> for IpcError {
    fn from(value: crate::secrets::SecretsError) -> Self {
        // Keychain operations don't carry a meaningful filesystem path —
        // map to Internal with the underlying message preserved. The
        // wizard surfaces these distinctly via the `kind` discriminator
        // on the TS side.
        IpcError::Internal {
            reason: format!("keychain: {value}"),
        }
    }
}

impl From<crate::app_state::AppStateError> for IpcError {
    fn from(value: crate::app_state::AppStateError) -> Self {
        match value {
            crate::app_state::AppStateError::Io { path, source } => IpcError::Io {
                path: path.display().to_string(),
                reason: source.to_string(),
            },
            other => IpcError::Internal {
                reason: other.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_unparseable_serializes_with_kind_and_camel_paths() {
        let err = IpcError::ConfigUnparseable {
            path: "/tmp/x".into(),
            reason: "expected `:`".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"config-unparseable\""));
        assert!(json.contains("\"path\":\"/tmp/x\""));
        assert!(json.contains("\"reason\":\"expected `:`\""));
    }

    #[test]
    fn region_conflict_carries_path_for_sprint_8() {
        let err = IpcError::RegionConflict {
            path: "/home/me/.claude/settings.json".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"region-conflict\""));
        assert!(json.contains("/home/me/.claude/settings.json"));
    }

    #[test]
    fn harness_not_implemented_includes_id() {
        let err = IpcError::HarnessNotImplemented {
            id: HarnessId::CodexCli,
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"harness-not-implemented\""));
        assert!(json.contains("\"id\":\"codex-cli\""));
    }

    #[test]
    fn io_error_carries_both_fields() {
        let err = IpcError::Io {
            path: "/tmp/x".into(),
            reason: "permission denied".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"io\""));
        assert!(json.contains("\"path\":\"/tmp/x\""));
    }

    #[test]
    fn internal_error_serializes() {
        let err = IpcError::Internal {
            reason: "unexpected".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"internal\""));
    }

    #[test]
    fn updater_check_failed_serializes_with_kebab_kind() {
        let err = IpcError::UpdaterCheckFailed {
            reason: "network timeout".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"updater-check-failed\""));
        assert!(json.contains("\"reason\":\"network timeout\""));
    }

    #[test]
    fn display_messages_are_useful() {
        let err = IpcError::HarnessNotDetected {
            id: HarnessId::ClaudeCode,
        };
        let msg = format!("{err}");
        assert!(msg.contains("ClaudeCode"));
    }

    fn sample_payload() -> ConflictPayload {
        ConflictPayload {
            config_path: "/tmp/.claude/settings.json".into(),
            format: crate::safety::sentinels::Format::Json,
            original_region_payload: Some(r#"{"a":1}"#.into()),
            current_region_payload: r#"{"a":2}"#.into(),
            theirs_region_payload: r#"{"a":3}"#.into(),
            file_before: r#"{"a":2}"#.into(),
            file_after_if_taking_theirs: r#"{"a":3}"#.into(),
        }
    }

    #[test]
    fn region_conflict_detected_serializes_with_kebab_kind_and_camel_payload_fields() {
        let err = IpcError::RegionConflictDetected {
            conflict: Box::new(sample_payload()),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"region-conflict-detected\""));
        assert!(json.contains("\"configPath\":\"/tmp/.claude/settings.json\""));
        assert!(json.contains("\"originalRegionPayload\":\"{\\\"a\\\":1}\""));
        assert!(json.contains("\"currentRegionPayload\""));
        assert!(json.contains("\"theirsRegionPayload\""));
        assert!(json.contains("\"fileBefore\""));
        assert!(json.contains("\"fileAfterIfTakingTheirs\""));
    }

    #[test]
    fn region_conflict_detected_orphan_path_serializes_original_as_null() {
        let mut payload = sample_payload();
        payload.original_region_payload = None;
        let err = IpcError::RegionConflictDetected {
            conflict: Box::new(payload),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"originalRegionPayload\":null"));
    }

    #[test]
    fn conflict_action_kebab_kind_with_options_options() {
        let action = ConflictAction::TakeTheirs {
            options: crate::adapters::ApplyOptions::default(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"kind\":\"take-theirs\""));
        assert!(json.contains("\"options\""));

        let json = serde_json::to_string(&ConflictAction::KeepMine).unwrap();
        assert_eq!(json, r#"{"kind":"keep-mine"}"#);

        let json = serde_json::to_string(&ConflictAction::MergeManually {
            options: crate::adapters::ApplyOptions::default(),
        })
        .unwrap();
        assert!(json.contains("\"kind\":\"merge-manually\""));
    }

    #[test]
    fn conflict_resolution_outcome_serializes_camel_case_fields_per_variant() {
        // Applied: single-word `patch` field — coincidentally fine, but
        // pin it anyway so a future refactor doesn't drift.
        let outcome = ConflictResolutionOutcome::Applied {
            patch: crate::adapters::TrovePatch {
                managed_block_hash: "h".into(),
                file_hash_at_last_write: "f".into(),
                format: crate::safety::sentinels::Format::Json,
                last_written_region_payload: r#"{"a":1}"#.into(),
            },
        };
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(json.contains("\"status\":\"applied\""));
        assert!(json.contains("\"patch\""));
        assert!(json.contains("\"lastWrittenRegionPayload\""));

        // MergeDeferred: needs the variant-level rename_all so the
        // wire emits `siblingPaths`, not `sibling_paths`.
        let outcome = ConflictResolutionOutcome::MergeDeferred {
            sibling_paths: SiblingPaths {
                original: "/tmp/x.trove.original".into(),
                theirs: "/tmp/x.trove.theirs".into(),
                host: "/tmp/x".into(),
            },
        };
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(json.contains("\"status\":\"merge-deferred\""));
        assert!(json.contains("\"siblingPaths\""));
        assert!(!json.contains("\"sibling_paths\""));
    }
}
