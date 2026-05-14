//! Claude Desktop adapter. Like Cline, Claude Desktop has no host
//! config Trove patches — telemetry comes from reading the per-session
//! `audit.jsonl` files Cowork writes under
//! `~/Library/Application Support/Claude/local-agent-mode-sessions/`.
//! See [`crate::adapters::claude_desktop_watcher`] for the tailer.
//!
//! This module exists so the IPC layer can present a uniform
//! Enable/Disable toggle: `apply` records a `state.harnesses` entry
//! (which triggers `respawn_persisted_watchers` → the watcher) and
//! `revert` removes it (which aborts the watcher and stops the tap).
//! No files on disk are touched in either direction.

use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::ipc::IpcError;
use crate::safety::sentinels::Format;

use super::{ApplyOptions, PatchPreview, PreviewStatus, TrovePatch};

/// Cowork's sessions root on macOS. Mirrors the watcher's view of the
/// world so the Harnesses-tab "config path" tooltip points users to
/// the same directory the tap reads from. The path is informational —
/// it may not exist yet on a brand-new Mac that has never opened
/// Cowork.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("Claude")
        .join("local-agent-mode-sessions")
}

/// Synthetic preview. Always `Fresh` — Trove never writes a host file
/// for Claude Desktop, so the same body every call.
pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    let path = config_path(home);
    let after = format!(
        "Claude Desktop: enable in-stream telemetry from {}\n\
         Trove will tail each Cowork session's audit log and emit\n\
         Tier A metrics (events, tokens by model, exact USD cost,\n\
         turn duration, errors) as turns complete. Prompt and\n\
         response bodies are never read.\n\
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

/// Apply: synthetic patch carrying the user's options snapshot. The
/// IPC layer follows up by spawning the watcher (via
/// `spawn_tier3_watcher`) and upserting `state.json`.
///
/// Idempotent: re-applying with the same options yields the same
/// `managed_block_hash`. The `tier3_watchers` registry replaces any
/// prior watcher under the same `HarnessId`, so re-applying can't leak
/// a runaway task.
pub fn apply(_home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    let payload = build_payload(opts);
    Ok(TrovePatch {
        managed_block_hash: sha256_hex(payload.as_bytes()),
        file_hash_at_last_write: String::new(),
        format: Format::Json,
        last_written_region_payload: payload,
    })
}

/// Revert is a filesystem no-op. The IPC layer aborts the watcher and
/// removes the `state.json` row.
pub fn revert(_home: &Path) -> Result<(), IpcError> {
    Ok(())
}

fn build_payload(opts: &ApplyOptions) -> String {
    let value = json!({
        "harness": "claude-desktop",
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
    fn config_path_is_macos_cowork_sessions_root() {
        let home = PathBuf::from("/Users/dev");
        let p = config_path(&home);
        let s = p.to_string_lossy();
        assert!(s.contains("Library/Application Support/Claude"), "{s}");
        assert!(s.ends_with("local-agent-mode-sessions"), "{s}");
    }

    #[test]
    fn preview_is_always_fresh_with_empty_before() {
        let home = PathBuf::from("/Users/dev");
        let preview = preview(&home, &ApplyOptions::default()).unwrap();
        assert!(matches!(preview.status, PreviewStatus::Fresh));
        assert_eq!(preview.before, "");
        assert!(preview.after.contains("Claude Desktop"));
        assert!(preview.after.contains("audit log"));
    }

    #[test]
    fn preview_promises_never_to_read_message_bodies() {
        let home = PathBuf::from("/Users/dev");
        let preview = preview(&home, &ApplyOptions::default()).unwrap();
        assert!(preview.after.contains("never read"));
    }

    #[test]
    fn apply_payload_is_stable_for_identical_options() {
        let home = PathBuf::from("/Users/dev");
        let opts = ApplyOptions::default();
        let a = apply(&home, &opts).unwrap();
        let b = apply(&home, &opts).unwrap();
        assert_eq!(a.managed_block_hash, b.managed_block_hash);
        assert!(a.last_written_region_payload.contains("\"claude-desktop\""));
    }

    #[test]
    fn apply_hash_changes_when_options_change() {
        let home = PathBuf::from("/Users/dev");
        let mut a = ApplyOptions::default();
        a.custom_attributes.insert("team".into(), "platform".into());
        let mut b = ApplyOptions::default();
        b.custom_attributes.insert("team".into(), "data".into());
        let pa = apply(&home, &a).unwrap();
        let pb = apply(&home, &b).unwrap();
        assert_ne!(pa.managed_block_hash, pb.managed_block_hash);
    }

    #[test]
    fn revert_is_filesystem_noop() {
        revert(&PathBuf::from("/Users/dev")).unwrap();
    }
}
