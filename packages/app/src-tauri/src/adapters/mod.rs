//! Per-harness adapters that translate `ApplyOptions` into a managed
//! region for the host config file. Sprint 3 ships claude-code (PR 2)
//! and gemini-cli (PR 3); Sprint 4 will extract a shared trait once
//! four adapters exist to inform the design.
//!
//! The Tauri `#[command]` layer in `crate::ipc` dispatches by
//! [`HarnessId`] into the per-adapter free functions. The pattern is
//! intentionally simple — each adapter has the same shape (`preview`,
//! `apply`, `revert`) so Sprint 4's trait extraction is mechanical.

pub mod claude_code;
pub mod codex_cli;
pub mod gemini_cli;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::safety::sentinels::Format;

/// Per-apply options chosen by the user in the UI. Mirrors the Zod
/// `ApplyOptions` schema in `@trove/shared`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOptions {
    /// When true, the adapter installs the harness's prompt-logging
    /// toggle in the patched config. Default false; the wizard requires
    /// an explicit acknowledgement before the user can flip it on.
    #[serde(default)]
    pub log_user_prompts: bool,
    /// Free-form `OTel` resource attributes the user wants attached to
    /// every emitted signal (e.g. `team=platform`).
    #[serde(default)]
    pub custom_attributes: BTreeMap<String, String>,
}

/// What [`preview`] tells the UI about the proposed write. Drives the
/// diff modal's CTA text and whether `apply` would actually write.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreviewStatus {
    /// No managed region exists yet. `apply` will install one fresh.
    Fresh,
    /// A managed region exists with the same hash we'd write. `apply`
    /// is a no-op (returns success without writing).
    Idempotent,
    /// A managed region exists with a different hash. `apply` will
    /// refuse with [`crate::ipc::IpcError::RegionConflict`]. Sprint 8
    /// replaces the refusal with a 3-way merge UI.
    Conflict,
}

/// What [`preview`] returns to the UI. The diff modal renders the
/// `before`/`after` diff client-side via the `diff` npm package.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPreview {
    pub config_path: PathBuf,
    pub format: Format,
    pub before: String,
    pub after: String,
    pub status: PreviewStatus,
}

/// What [`apply`] returns on success. Carries the same hash pair the
/// `safety::conflict` module needs (Sprint 5+ will persist it into
/// `state.json`; Sprint 3 returns it but does not yet store it).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrovePatch {
    pub managed_block_hash: String,
    pub file_hash_at_last_write: String,
    pub format: Format,
}

/// How many timestamped backups each adapter keeps for the harness's
/// config file. Older ones are pruned after every successful apply.
pub const BACKUPS_TO_KEEP: usize = 10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_options_default_is_off_with_empty_attrs() {
        let opts = ApplyOptions::default();
        assert!(!opts.log_user_prompts);
        assert!(opts.custom_attributes.is_empty());
    }

    #[test]
    fn apply_options_serializes_camel_case() {
        let mut opts = ApplyOptions::default();
        opts.custom_attributes.insert("team".into(), "platform".into());
        let json = serde_json::to_string(&opts).unwrap();
        assert!(json.contains("\"logUserPrompts\""));
        assert!(json.contains("\"customAttributes\""));
        assert!(!json.contains("\"log_user_prompts\""));
    }

    #[test]
    fn preview_status_serializes_kebab_case() {
        assert_eq!(serde_json::to_string(&PreviewStatus::Fresh).unwrap(), "\"fresh\"");
        assert_eq!(
            serde_json::to_string(&PreviewStatus::Idempotent).unwrap(),
            "\"idempotent\""
        );
        assert_eq!(
            serde_json::to_string(&PreviewStatus::Conflict).unwrap(),
            "\"conflict\""
        );
    }

    #[test]
    fn trove_patch_round_trips() {
        let p = TrovePatch {
            managed_block_hash: "a".repeat(64),
            file_hash_at_last_write: "b".repeat(64),
            format: Format::Json,
        };
        let json = serde_json::to_string(&p).unwrap();
        let revived: TrovePatch = serde_json::from_str(&json).unwrap();
        assert_eq!(p, revived);
    }
}
