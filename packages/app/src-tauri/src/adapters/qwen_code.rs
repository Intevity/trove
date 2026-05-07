//! Qwen Code adapter — patches `~/.qwen/settings.json`'s top-level
//! `telemetry` object. Qwen Code is a Gemini CLI fork; the settings
//! schema for telemetry is byte-for-byte the same shape, so this
//! adapter mirrors `gemini_cli` with the namespace swapped.
//!
//! Verify the field set against the upstream qwen-code docs at every
//! adapter rev — the schema may diverge from Gemini's over time.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::ipc::IpcError;
use crate::safety::atomic::write_atomic;
use crate::safety::backup::{backup_file, prune_backups};
use crate::safety::sentinels::{
    Format, ManagedRegion, SentinelError, extract_region, remove_region, upsert_region,
};

use super::{ApplyOptions, BACKUPS_TO_KEEP, PatchPreview, PreviewStatus, TrovePatch};

const CONFIG_DIR: &str = ".qwen";
const CONFIG_FILE: &str = "settings.json";

/// Resolve the absolute path of the Qwen Code settings file under
/// `home`. Pure helper so tests can scope to a `tempdir`.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    home.join(CONFIG_DIR).join(CONFIG_FILE)
}

/// Compute the diff between the current file and what an apply with
/// `opts` would write.
pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    let path = config_path(home);
    let (current, _existed) = read_or_empty(&path)?;
    let working = if current.is_empty() {
        "{}".to_string()
    } else {
        current.clone()
    };

    let region = build_region(opts).map_err(|e| IpcError::Internal {
        reason: format!("could not build managed region: {e}"),
    })?;

    let status = classify(&working, &region, &path)?;

    let after = upsert_region(Format::Json, &working, &region).map_err(|e| match e {
        SentinelError::Malformed { .. } | SentinelError::RegionMalformed(_) => {
            IpcError::ConfigUnparseable {
                path: path.display().to_string(),
                reason: e.to_string(),
            }
        }
        other => IpcError::Internal {
            reason: other.to_string(),
        },
    })?;

    Ok(PatchPreview {
        config_path: path,
        format: Format::Json,
        before: current,
        after,
        status,
    })
}

/// Apply the patch. Same safety contract as the other JSON adapters:
/// backup, atomic write, prune, refuse to overwrite a managed region
/// whose hash differs from the new patch.
pub fn apply(home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    let path = config_path(home);
    let (current, existed) = read_or_empty(&path)?;
    let working = if current.is_empty() {
        "{}".to_string()
    } else {
        current.clone()
    };

    let region = build_region(opts).map_err(|e| IpcError::Internal {
        reason: format!("could not build managed region: {e}"),
    })?;

    match classify(&working, &region, &path)? {
        PreviewStatus::Idempotent => {
            return Ok(TrovePatch {
                managed_block_hash: region.hash.clone(),
                file_hash_at_last_write: hash_hex(current.as_bytes()),
                format: Format::Json,
            });
        }
        PreviewStatus::Conflict => {
            return Err(IpcError::RegionConflict {
                path: path.display().to_string(),
            });
        }
        PreviewStatus::Fresh => {}
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| IpcError::Io {
            path: parent.display().to_string(),
            reason: e.to_string(),
        })?;
    }

    if existed {
        backup_file(&path).map_err(|e| IpcError::Io {
            path: path.display().to_string(),
            reason: format!("backup failed: {e}"),
        })?;
    }

    let after = upsert_region(Format::Json, &working, &region).map_err(|e| match e {
        SentinelError::Malformed { .. } | SentinelError::RegionMalformed(_) => {
            IpcError::ConfigUnparseable {
                path: path.display().to_string(),
                reason: e.to_string(),
            }
        }
        other => IpcError::Internal {
            reason: other.to_string(),
        },
    })?;

    write_atomic(&path, after.as_bytes()).map_err(|e| IpcError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let _ = prune_backups(&path, BACKUPS_TO_KEEP);

    Ok(TrovePatch {
        managed_block_hash: region.hash.clone(),
        file_hash_at_last_write: hash_hex(after.as_bytes()),
        format: Format::Json,
    })
}

/// Permissive revert — removes any Trove-managed region present.
pub fn revert(home: &Path) -> Result<(), IpcError> {
    let path = config_path(home);
    let (current, existed) = read_or_empty(&path)?;
    if !existed {
        return Ok(());
    }

    match extract_region(Format::Json, &current) {
        Ok(Some(_)) => {}
        Ok(None) => return Ok(()),
        Err(e) => {
            return Err(IpcError::ConfigUnparseable {
                path: path.display().to_string(),
                reason: e.to_string(),
            });
        }
    }

    backup_file(&path).map_err(|e| IpcError::Io {
        path: path.display().to_string(),
        reason: format!("backup failed: {e}"),
    })?;

    let after = remove_region(Format::Json, &current).map_err(|e| IpcError::ConfigUnparseable {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    write_atomic(&path, after.as_bytes()).map_err(|e| IpcError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let _ = prune_backups(&path, BACKUPS_TO_KEEP);

    Ok(())
}

fn read_or_empty(path: &Path) -> Result<(String, bool), IpcError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok((text, true)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((String::new(), false)),
        Err(e) => Err(IpcError::Io {
            path: path.display().to_string(),
            reason: e.to_string(),
        }),
    }
}

fn classify(
    current: &str,
    region: &ManagedRegion,
    path: &Path,
) -> Result<PreviewStatus, IpcError> {
    match extract_region(Format::Json, current) {
        Ok(Some(existing)) if existing.hash == region.hash => Ok(PreviewStatus::Idempotent),
        Ok(Some(_)) => Ok(PreviewStatus::Conflict),
        Ok(None) => Ok(PreviewStatus::Fresh),
        Err(e) => Err(IpcError::ConfigUnparseable {
            path: path.display().to_string(),
            reason: e.to_string(),
        }),
    }
}

/// Build the [`ManagedRegion`] for a JSON merge of the `telemetry`
/// block. Mirrors `gemini_cli::build_region` because Qwen Code is a
/// Gemini CLI fork and the upstream schema is byte-identical for this
/// block. `customAttributes` is a no-op for Qwen for the same reason
/// it's a no-op for Gemini — the schema doesn't yet expose a clear
/// path for resource attributes; we'll add the field once Qwen's docs
/// settle (or the schema diverges from Gemini's).
fn build_region(opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    let mut telemetry = serde_json::Map::new();
    telemetry.insert("enabled".to_string(), Value::Bool(true));
    telemetry.insert("target".to_string(), Value::String("local".to_string()));
    telemetry.insert(
        "otlpEndpoint".to_string(),
        Value::String("http://127.0.0.1:4318".to_string()),
    );
    telemetry.insert("logPrompts".to_string(), Value::Bool(opts.log_user_prompts));

    let mut top = serde_json::Map::new();
    top.insert("telemetry".to_string(), Value::Object(telemetry));
    ManagedRegion::for_json_patches(&top)
}

fn hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn read_config(home: &Path) -> String {
        fs::read_to_string(config_path(home)).unwrap()
    }

    // --- 1. Fresh install ----------------------------------------------------

    #[test]
    fn fresh_install_creates_a_valid_settings_file() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        let written = read_config(home.path());
        let parsed: Value = serde_json::from_str(&written).unwrap();
        let telemetry = parsed.get("telemetry").and_then(Value::as_object).unwrap();
        assert_eq!(telemetry["enabled"], true);
        assert_eq!(telemetry["target"], "local");
        assert_eq!(telemetry["otlpEndpoint"], "http://127.0.0.1:4318");
        assert_eq!(telemetry["logPrompts"], false);
        assert!(parsed.get("_trove").is_some());
    }

    // --- 2. Idempotent re-apply ---------------------------------------------

    #[test]
    fn idempotent_reapply_does_not_change_the_file() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_first = read_config(home.path());
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_second = read_config(home.path());
        assert_eq!(after_first, after_second);
    }

    // --- 3. User-edited outside the managed block --------------------------

    #[test]
    fn user_keys_outside_block_survive_apply() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Pre-populate Qwen-flavoured user keys.
        fs::write(
            &path,
            r#"{"theme":"dark","model":{"name":"qwen3-coder"}}"#,
        )
        .unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(parsed["theme"], "dark");
        assert_eq!(parsed["model"]["name"], "qwen3-coder");
        assert_eq!(parsed["telemetry"]["enabled"], true);
    }

    // --- 4. User-edited inside the managed block (conflict) ---------------

    #[test]
    fn editing_inside_the_managed_block_yields_conflict() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        let path = config_path(home.path());
        let written = read_config(home.path());
        let edited = written.replace("http://127.0.0.1:4318", "http://x.example.com");
        assert_ne!(edited, written);
        fs::write(&path, &edited).unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
        assert!(matches!(err, IpcError::RegionConflict { .. }));
        assert_eq!(read_config(home.path()), edited);
    }

    // --- 5. Malformed file --------------------------------------------------

    #[test]
    fn malformed_file_is_unparseable_error() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not valid json").unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
        assert!(matches!(err, IpcError::ConfigUnparseable { .. }));
        assert_eq!(read_config(home.path()), "{not valid json");
    }

    // --- 6. Missing parent dir ---------------------------------------------

    #[test]
    fn missing_parent_dir_is_created_automatically() {
        let home = tempdir().unwrap();
        assert!(!home.path().join(".qwen").exists());
        apply(home.path(), &ApplyOptions::default()).unwrap();
        assert!(config_path(home.path()).exists());
    }

    // --- 7. Read-only parent dir → IO error --------------------------------

    #[cfg(unix)]
    #[test]
    fn readonly_parent_dir_yields_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempdir().unwrap();
        let parent = home.path().join(".qwen");
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(err, IpcError::Io { .. }));
    }

    // --- Revert round-trip --------------------------------------------------

    #[test]
    fn revert_restores_byte_identical_pre_apply_file() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "{\n  \"theme\": \"dark\",\n  \"model\": {\n    \"name\": \"qwen3-coder\"\n  }\n}\n";
        fs::write(&path, original).unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();
        revert(home.path()).unwrap();

        assert_eq!(read_config(home.path()), original);
    }

    #[test]
    fn revert_on_missing_file_is_noop() {
        let home = tempdir().unwrap();
        revert(home.path()).unwrap();
        assert!(!config_path(home.path()).exists());
    }

    #[test]
    fn revert_when_no_trove_block_is_noop() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let user_only = "{\"theme\":\"light\"}";
        fs::write(&path, user_only).unwrap();
        revert(home.path()).unwrap();
        assert_eq!(read_config(home.path()), user_only);
    }

    // --- log_user_prompts toggle -------------------------------------------

    #[test]
    fn log_user_prompts_propagates_to_telemetry_log_prompts() {
        let home = tempdir().unwrap();
        let opts = ApplyOptions {
            log_user_prompts: true,
            ..Default::default()
        };
        apply(home.path(), &opts).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(parsed["telemetry"]["logPrompts"], true);
    }

    #[test]
    fn preview_after_apply_returns_idempotent_status() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Idempotent);
    }
}
