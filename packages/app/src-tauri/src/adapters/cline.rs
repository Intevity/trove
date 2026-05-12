//! Cline adapter — Tier 3 best-effort. Cline (the `VSCode` extension at
//! `saoudrizwan.claude-dev`) doesn't emit OpenTelemetry natively, and
//! its `package.json` declares an empty `contributes.configuration`
//! object — there's no setting we can flip that meaningfully changes
//! its output. So Trove watches Cline's per-task records under the
//! `VSCode` `globalStorage/saoudrizwan.claude-dev/tasks/` tree and emits
//! OTLP logs derived from them.
//!
//! ## Diverges from `HarnessSpec`
//!
//! Tier 1 and Tier 2 adapters declare a `HarnessSpec` and merge a
//! managed region into a host config file. Cline doesn't have one to
//! patch — every observable signal lives in files Cline writes (and
//! Trove must not write to). So this module exposes the same
//! `preview` / `apply` / `revert` / `config_path` shape for the IPC
//! dispatcher, but the implementations are bespoke:
//!
//! - `config_path` returns Cline's globalStorage root (the path the
//!   watcher tails).
//! - `preview` always returns `PreviewStatus::Fresh` with a synthetic
//!   `before` / `after` block describing what enabling will do.
//! - `apply` returns a synthetic `TrovePatch` whose
//!   `last_written_region_payload` records the user's `ApplyOptions`
//!   snapshot; the IPC layer also spawns the log watcher and upserts
//!   `state.json`.
//! - `revert` is a no-op on the filesystem (no host file was patched);
//!   the IPC layer aborts the watcher and removes the `state.json`
//!   row.

use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::ipc::IpcError;
use crate::safety::sentinels::Format;

use super::{ApplyOptions, PatchPreview, PreviewStatus, TrovePatch};

/// `VSCode` publisher.extension id Cline ships under. Pinned because the
/// tasks dir under `globalStorage/<id>/` only exists for this exact
/// string; renames to a fork (e.g. `RooVeterinaryInc.roo-cline`) need
/// a separate adapter.
pub(crate) const EXTENSION_ID: &str = "saoudrizwan.claude-dev";

/// Resolve the absolute path of Cline's `globalStorage` directory under
/// `home`. The directory may not exist yet on a fresh machine — the
/// watcher tolerates that.
///
/// Per-OS layout (mirrors `VSCode`'s user-data-dir layout):
/// - macOS: `~/Library/Application Support/Code/User/globalStorage/<id>`
/// - Linux: `~/.config/Code/User/globalStorage/<id>`
/// - Windows: `%APPDATA%\Code\User\globalStorage\<id>`
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    cline_global_storage_root(home)
}

/// `globalStorage/<id>` — used by `config_path` and `tasks_dir`.
#[must_use]
pub fn cline_global_storage_root(home: &Path) -> PathBuf {
    let user_data = vscode_user_data_dir(home);
    user_data.join("globalStorage").join(EXTENSION_ID)
}

/// `globalStorage/<id>/tasks` — the watcher polls this. Absent on a
/// fresh Cline install until the user starts their first task.
#[must_use]
pub fn tasks_dir(home: &Path) -> PathBuf {
    cline_global_storage_root(home).join("tasks")
}

/// Best-guess `VSCode` user-data dir. Cline's globalStorage lives under
/// `<user_data>/User/globalStorage/<id>`. `home` is the test seam.
fn vscode_user_data_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
    } else if cfg!(target_os = "windows") {
        home.join("AppData")
            .join("Roaming")
            .join("Code")
            .join("User")
    } else {
        home.join(".config").join("Code").join("User")
    }
}

/// Compute the diff Trove would write for cline. Always `Fresh` — Cline
/// has no host file we touch, so the same output every call. The IPC
/// layer treats `Fresh` as "OK to apply" which is the right semantic
/// for repeat clicks: the second apply re-spawns the watcher
/// (replacing the prior handle) and re-stamps `state.json`, neither of
/// which is destructive.
pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    let path = config_path(home);
    let after = format!(
        "Cline: enable best-effort log watching at {}\n\
         Token counts and turn metadata will be emitted as OTLP logs.\n\
         Prompt bodies are never captured (Trove is metrics-only).\n\
         Custom attributes: {} entries.\n",
        path.display(),
        opts.custom_attributes.len(),
    );
    Ok(PatchPreview {
        config_path: path,
        format: Format::Json,
        before: String::new(),
        after,
        status: PreviewStatus::Fresh,
    })
}

/// Apply: returns a synthetic `TrovePatch` carrying the user's options
/// snapshot. The IPC layer is responsible for:
/// 1. Spawning the watcher via `tier3_watchers`.
/// 2. Calling `app_state::upsert_harness` with the returned patch.
///
/// Idempotency: re-applying with identical options yields the same
/// `managed_block_hash` and the same payload. The `tier3_watchers`
/// registry aborts any prior watcher when a new one is inserted under
/// the same `HarnessId`, so re-applying can never leak a runaway task.
pub fn apply(_home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    let payload = build_payload(opts);
    Ok(TrovePatch {
        managed_block_hash: sha256_hex(payload.as_bytes()),
        // Cline doesn't write a host file, so there is no "file at
        // last write" to hash. Empty by convention; the conflict UI
        // uses `last_written_region_payload` for tier 3 records.
        file_hash_at_last_write: String::new(),
        format: Format::Json,
        last_written_region_payload: payload,
    })
}

/// Revert is a filesystem no-op for Cline — the IPC layer aborts the
/// watcher and removes the `state.json` row. Returning `Ok` lets
/// `revert_patch` proceed without erroring on missing host files.
pub fn revert(_home: &Path) -> Result<(), IpcError> {
    Ok(())
}

/// JSON payload Trove records for the cline `state.json` row. Snapshot
/// of the user's `ApplyOptions` so a future re-apply can detect drift.
fn build_payload(opts: &ApplyOptions) -> String {
    let value = json!({
        "harness": "cline",
        "customAttributes": opts.custom_attributes,
    });
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extension_id_is_pinned_to_upstream_publisher() {
        assert_eq!(EXTENSION_ID, "saoudrizwan.claude-dev");
    }

    #[test]
    fn config_path_is_global_storage_root_under_user_data() {
        let home = PathBuf::from("/home/dev");
        let resolved = config_path(&home);
        let s = resolved.to_string_lossy();
        assert!(s.contains("globalStorage"), "{s}");
        assert!(s.ends_with(EXTENSION_ID), "{s}");
    }

    #[test]
    fn tasks_dir_lives_under_global_storage_root() {
        let home = PathBuf::from("/home/dev");
        let tasks = tasks_dir(&home);
        let root = cline_global_storage_root(&home);
        assert!(tasks.starts_with(&root), "{tasks:?} not under {root:?}");
        assert_eq!(tasks.file_name().unwrap(), "tasks");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_global_storage_path_uses_application_support() {
        let home = PathBuf::from("/Users/dev");
        let p = config_path(&home);
        let s = p.to_string_lossy();
        assert!(s.contains("Library/Application Support/Code/User"), "{s}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_global_storage_path_uses_dot_config_code() {
        let home = PathBuf::from("/home/dev");
        let p = config_path(&home);
        let s = p.to_string_lossy();
        assert!(s.contains(".config/Code/User"), "{s}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_global_storage_path_uses_appdata_roaming() {
        let home = PathBuf::from("C:\\Users\\dev");
        let p = config_path(&home);
        let s = p.to_string_lossy();
        assert!(s.contains("AppData") && s.contains("Roaming"), "{s}");
    }

    #[test]
    fn preview_always_returns_fresh_status() {
        let home = PathBuf::from("/home/dev");
        let preview = preview(&home, &ApplyOptions::default()).unwrap();
        assert!(matches!(preview.status, PreviewStatus::Fresh));
        assert_eq!(preview.before, "");
        assert!(preview.after.contains("Cline"));
    }

    #[test]
    fn preview_after_text_says_prompt_bodies_are_never_captured() {
        let home = PathBuf::from("/home/dev");
        let preview = preview(&home, &ApplyOptions::default()).unwrap();
        assert!(preview.after.contains("never captured"));
    }

    #[test]
    fn apply_returns_synthetic_patch_with_consistent_hash() {
        let home = PathBuf::from("/home/dev");
        let opts = ApplyOptions::default();
        let p1 = apply(&home, &opts).unwrap();
        let p2 = apply(&home, &opts).unwrap();
        assert_eq!(p1.managed_block_hash, p2.managed_block_hash);
        assert!(p1.last_written_region_payload.contains("\"cline\""));
    }

    #[test]
    fn apply_hash_changes_when_options_change() {
        let home = PathBuf::from("/home/dev");
        let mut opts_a = ApplyOptions::default();
        opts_a.custom_attributes.insert("team".into(), "platform".into());
        let mut opts_b = ApplyOptions::default();
        opts_b.custom_attributes.insert("team".into(), "data".into());

        let pa = apply(&home, &opts_a).unwrap();
        let pb = apply(&home, &opts_b).unwrap();
        assert_ne!(pa.managed_block_hash, pb.managed_block_hash);
    }

    #[test]
    fn apply_payload_records_custom_attributes_verbatim() {
        let home = PathBuf::from("/home/dev");
        let mut opts = ApplyOptions::default();
        opts.custom_attributes.insert("team".into(), "platform".into());
        opts.custom_attributes.insert("env".into(), "prod".into());
        let patch = apply(&home, &opts).unwrap();
        assert!(patch
            .last_written_region_payload
            .contains("\"team\":\"platform\""));
        assert!(patch
            .last_written_region_payload
            .contains("\"env\":\"prod\""));
    }

    #[test]
    fn revert_is_a_filesystem_noop() {
        let home = PathBuf::from("/home/dev");
        revert(&home).unwrap();
    }

    #[test]
    fn apply_payload_omits_log_user_prompts_field() {
        // The toggle was removed; the persisted snapshot must not
        // contain any logUserPrompts key so v3 records stay clean.
        let home = PathBuf::from("/home/dev");
        let patch = apply(&home, &ApplyOptions::default()).unwrap();
        assert!(!patch.last_written_region_payload.contains("logUserPrompts"));
    }
}
