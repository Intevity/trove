//! Per-harness adapters that translate `ApplyOptions` into a managed
//! region for the host config file. Tier 1 covers four harnesses:
//! claude-code, codex-cli, gemini-cli, qwen-code.
//!
//! The Tauri `#[command]` layer in `crate::ipc` dispatches by
//! [`crate::harness::HarnessId`] into the per-adapter free functions.
//! Each adapter is a thin wrapper around [`common`]: it declares a
//! `const SPEC: HarnessSpec` with its config path / format / region
//! builder and delegates `preview` / `apply` / `revert` to the shared
//! helpers. See `documentation/adding-a-harness.md` for the contract
//! contributors must satisfy.

mod common;

pub mod claude_code;
pub mod cline;
pub mod cline_watcher;
pub mod codex_cli;
pub mod cursor_cli;
pub mod cursor_common;
pub mod cursor_ide;
pub mod gemini_cli;
pub mod opencode;
pub mod qwen_code;

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

/// What [`apply`] returns on success. Carries the hashes
/// `safety::conflict` consumes and the canonical region payload the
/// 3-way merge UI's "original" pane reads back at conflict time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrovePatch {
    pub managed_block_hash: String,
    pub file_hash_at_last_write: String,
    pub format: Format,
    /// The canonical payload string Trove wrote at last apply. Empty for
    /// records migrated from schema v2 — those harnesses fall back to a
    /// 2-pane orphan-block view in the resolver until the next apply
    /// re-populates the field.
    #[serde(default)]
    pub last_written_region_payload: String,
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
            last_written_region_payload: r#"{"env":{"OTEL_FOO":"bar"}}"#.into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        let revived: TrovePatch = serde_json::from_str(&json).unwrap();
        assert_eq!(p, revived);
        assert!(json.contains("\"lastWrittenRegionPayload\""));
    }

    #[test]
    fn trove_patch_accepts_v2_records_missing_last_written_region_payload() {
        // schemaVersion 2 records persisted before Sprint 8 don't have
        // the lastWrittenRegionPayload key; serde must default it to ""
        // so the v2 -> v3 migration is structurally a no-op for serde.
        let v2_json = r#"{
            "managedBlockHash": "aaaa",
            "fileHashAtLastWrite": "bbbb",
            "format": "json"
        }"#;
        let revived: TrovePatch = serde_json::from_str(v2_json).unwrap();
        assert_eq!(revived.last_written_region_payload, "");
    }
}
