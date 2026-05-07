//! Claude Code adapter — patches `~/.claude/settings.json`'s top-level
//! `env` object with the `OTel` env vars Anthropic's harness reads at
//! launch. The patch is a JSON merge: leaf paths under `env` are
//! installed at their natural locations and recorded in the file's
//! `_trove` sentinel block so [`revert`] can remove exactly the keys
//! Trove owns without touching anything the user added.
//!
//! Verify the env-var list against
//! <https://docs.anthropic.com/en/docs/claude-code/monitoring-usage>
//! at every adapter rev — Anthropic occasionally adds variables.

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

const CONFIG_DIR: &str = ".claude";
const CONFIG_FILE: &str = "settings.json";

/// The base set of `OTel` env vars Trove writes into `env`. Custom
/// attributes and the prompt-logging toggle are appended at apply time
/// based on [`ApplyOptions`].
const MANAGED_ENV_KEYS: &[(&str, &str)] = &[
    ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
    ("OTEL_METRICS_EXPORTER", "otlp"),
    ("OTEL_LOGS_EXPORTER", "otlp"),
    ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
    ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:4318"),
    ("OTEL_METRIC_EXPORT_INTERVAL", "60000"),
    ("OTEL_LOGS_EXPORT_INTERVAL", "5000"),
];

/// Resolve the absolute path of the Claude Code settings file under
/// `home`. Pure helper so tests can scope to a `tempdir`.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    home.join(CONFIG_DIR).join(CONFIG_FILE)
}

/// Compute the diff between the current file and what an apply with
/// `opts` would write. Always succeeds when the file is parseable;
/// returns `IpcError::ConfigUnparseable` otherwise.
pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    let path = config_path(home);
    let (current, _existed) = read_or_empty(&path)?;
    let working = if current.is_empty() { "{}".to_string() } else { current.clone() };

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

/// Apply the patch. Backs the existing file up, atomically writes the
/// new content, and prunes old backups. Idempotent when the existing
/// managed region matches what we'd write; refuses with
/// [`IpcError::RegionConflict`] when it doesn't (Sprint 8 will replace
/// the refusal with a 3-way merge UI).
pub fn apply(home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    let path = config_path(home);
    let (current, existed) = read_or_empty(&path)?;
    let working = if current.is_empty() { "{}".to_string() } else { current.clone() };

    let region = build_region(opts).map_err(|e| IpcError::Internal {
        reason: format!("could not build managed region: {e}"),
    })?;

    match classify(&working, &region, &path)? {
        PreviewStatus::Idempotent => {
            // Existing block already matches; return the current
            // metadata without re-writing. The file hash reflects what's
            // on disk now.
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

    // Best-effort prune; a failure here doesn't poison the apply (the
    // user's config has already been written successfully).
    let _ = prune_backups(&path, BACKUPS_TO_KEEP);

    Ok(TrovePatch {
        managed_block_hash: region.hash.clone(),
        file_hash_at_last_write: hash_hex(after.as_bytes()),
        format: Format::Json,
    })
}

/// Remove any Trove-managed region from the host file. Permissive: any
/// region present is removed even without stored metadata, so a user
/// who reinstalls Trove on a fresh machine can still unwire the
/// previous machine's patch. No-op when the file is missing or contains
/// no managed region.
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

/// Read the host file or return an empty string if absent. The boolean
/// distinguishes "missing" from "present and empty" — apply skips the
/// backup step when the file didn't exist before.
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

/// Decide whether `region` would be a fresh write, an idempotent
/// no-op, or a refused conflict against `current`.
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

/// Build the [`ManagedRegion`] for a JSON merge of the env block.
fn build_region(opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    let mut env = serde_json::Map::new();
    for (k, v) in MANAGED_ENV_KEYS {
        env.insert((*k).to_string(), Value::String((*v).to_string()));
    }
    if opts.log_user_prompts {
        env.insert(
            "OTEL_LOG_USER_PROMPTS".to_string(),
            Value::String("true".to_string()),
        );
    }
    if !opts.custom_attributes.is_empty() {
        // Use a stable comma-separated form so the hash is deterministic
        // (BTreeMap iteration is already sorted).
        let attrs = opts
            .custom_attributes
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");
        env.insert(
            "OTEL_RESOURCE_ATTRIBUTES".to_string(),
            Value::String(attrs),
        );
    }

    let mut top = serde_json::Map::new();
    top.insert("env".to_string(), Value::Object(env));
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

    // --- 1. Fresh install -----------------------------------------------------

    #[test]
    fn fresh_install_creates_a_valid_settings_file() {
        let home = tempdir().unwrap();
        let patch = apply(home.path(), &ApplyOptions::default()).unwrap();

        let written = read_config(home.path());
        // Plan agent flag: assert the file actually parses as JSON.
        let parsed: Value = serde_json::from_str(&written).unwrap();

        let env = parsed.get("env").and_then(Value::as_object).unwrap();
        assert_eq!(env["OTEL_EXPORTER_OTLP_ENDPOINT"], "http://127.0.0.1:4318");
        assert_eq!(env["CLAUDE_CODE_ENABLE_TELEMETRY"], "1");
        assert!(parsed.get("_trove").is_some(), "missing sentinel block");

        assert_eq!(patch.managed_block_hash.len(), 64);
        assert_eq!(patch.file_hash_at_last_write.len(), 64);
        assert_eq!(patch.format, Format::Json);
    }

    // --- 2. Idempotent re-apply ----------------------------------------------

    #[test]
    fn idempotent_reapply_does_not_change_the_file() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_first = read_config(home.path());

        let second = apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_second = read_config(home.path());
        assert_eq!(after_first, after_second);
        // The returned hashes are identical too.
        let expected = hash_hex(after_second.as_bytes());
        assert_eq!(second.file_hash_at_last_write, expected);
    }

    // --- 3. User-edited outside the managed block ---------------------------

    #[test]
    fn user_keys_outside_block_survive_apply() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // User has an unrelated top-level key plus their own env var.
        fs::write(
            &path,
            r#"{"theme":"dark","env":{"MY_USER_VAR":"keepme"}}"#,
        )
        .unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(parsed["theme"], "dark");
        let env = parsed.get("env").and_then(Value::as_object).unwrap();
        assert_eq!(env["MY_USER_VAR"], "keepme");
        // And our keys are present alongside.
        assert_eq!(env["OTEL_EXPORTER_OTLP_ENDPOINT"], "http://127.0.0.1:4318");
    }

    // --- 4. User-edited inside the managed block (conflict) ----------------

    #[test]
    fn editing_inside_the_managed_block_yields_conflict() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        // Tamper with one of Trove's managed values without touching
        // the sentinel _trove.hash field.
        let path = config_path(home.path());
        let written = read_config(home.path());
        let edited =
            written.replace("http://127.0.0.1:4318", "http://attacker.example.com");
        assert_ne!(edited, written);
        fs::write(&path, &edited).unwrap();

        let result = apply(home.path(), &ApplyOptions::default());
        match result {
            Err(IpcError::RegionConflict { path: p }) => {
                assert_eq!(p, path.display().to_string());
            }
            other => panic!("expected RegionConflict, got {other:?}"),
        }

        // The conflicting file is left untouched (no silent overwrite).
        assert_eq!(read_config(home.path()), edited);
    }

    // --- 5. Malformed file ---------------------------------------------------

    #[test]
    fn malformed_file_is_unparseable_error() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not valid json").unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
        assert!(
            matches!(err, IpcError::ConfigUnparseable { .. }),
            "expected ConfigUnparseable, got {err:?}"
        );
        // The malformed file is left untouched.
        assert_eq!(read_config(home.path()), "{not valid json");
    }

    // --- 6. Missing parent dir ----------------------------------------------

    #[test]
    fn missing_parent_dir_is_created_automatically() {
        let home = tempdir().unwrap();
        // .claude doesn't exist yet — apply must create it.
        assert!(!home.path().join(".claude").exists());
        apply(home.path(), &ApplyOptions::default()).unwrap();
        assert!(home.path().join(".claude").exists());
        assert!(config_path(home.path()).exists());
    }

    // --- 7. Read-only parent dir → IO error ---------------------------------

    #[cfg(unix)]
    #[test]
    fn readonly_parent_dir_yields_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempdir().unwrap();
        // Create the parent ahead of time and lock it down.
        let parent = home.path().join(".claude");
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();

        // Restore permissions so tempdir cleanup works.
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(
            matches!(err, IpcError::Io { .. }),
            "expected Io error, got {err:?}"
        );
    }

    // --- Revert round-trip ---------------------------------------------------

    #[test]
    fn revert_restores_byte_identical_pre_apply_file() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "{\n  \"theme\": \"dark\",\n  \"env\": {\n    \"MY\": \"keepme\"\n  }\n}\n";
        fs::write(&path, original).unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();
        revert(home.path()).unwrap();

        let after = read_config(home.path());
        // The plan agent flagged this as load-bearing: byte-identity
        // including the trailing newline.
        assert_eq!(after, original);
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

    #[test]
    fn revert_on_malformed_file_returns_unparseable_error() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{nope").unwrap();
        let err = revert(home.path()).unwrap_err();
        assert!(matches!(err, IpcError::ConfigUnparseable { .. }));
    }

    // --- Preview --------------------------------------------------------------

    #[test]
    fn preview_on_missing_file_returns_fresh_status() {
        let home = tempdir().unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Fresh);
        assert_eq!(preview.format, Format::Json);
        assert_eq!(preview.before, "");
        assert!(preview.after.contains("\"OTEL_EXPORTER_OTLP_ENDPOINT\""));
    }

    #[test]
    fn preview_after_apply_returns_idempotent_status() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Idempotent);
        // Idempotent preview's after should equal the current file.
        assert_eq!(preview.after, preview.before);
    }

    #[test]
    fn preview_with_tampered_block_returns_conflict_status() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let path = config_path(home.path());
        let edited =
            read_config(home.path()).replace("http://127.0.0.1:4318", "http://x.example.com");
        fs::write(&path, &edited).unwrap();

        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Conflict);
    }

    // --- Custom attributes & log_user_prompts -------------------------------

    #[test]
    fn custom_attributes_render_in_otel_resource_attributes() {
        let home = tempdir().unwrap();
        let mut opts = ApplyOptions::default();
        opts.custom_attributes
            .insert("team".into(), "platform".into());
        opts.custom_attributes
            .insert("env".into(), "prod".into());
        apply(home.path(), &opts).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        let env = parsed.get("env").and_then(Value::as_object).unwrap();
        let attrs = env["OTEL_RESOURCE_ATTRIBUTES"].as_str().unwrap();
        // BTreeMap is sorted; output is deterministic.
        assert_eq!(attrs, "env=prod,team=platform");
    }

    #[test]
    fn log_user_prompts_adds_dedicated_env_var_when_true() {
        let home = tempdir().unwrap();
        let opts = ApplyOptions {
            log_user_prompts: true,
            ..Default::default()
        };
        apply(home.path(), &opts).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        let env = parsed.get("env").and_then(Value::as_object).unwrap();
        assert_eq!(env["OTEL_LOG_USER_PROMPTS"], "true");
    }

    #[test]
    fn log_user_prompts_default_omits_env_var() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        let env = parsed.get("env").and_then(Value::as_object).unwrap();
        assert!(env.get("OTEL_LOG_USER_PROMPTS").is_none());
    }

    #[test]
    fn changing_options_between_applies_yields_conflict_until_reverted() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        let mut opts2 = ApplyOptions::default();
        opts2
            .custom_attributes
            .insert("team".into(), "platform".into());
        let err = apply(home.path(), &opts2).unwrap_err();
        assert!(matches!(err, IpcError::RegionConflict { .. }));

        // Revert + re-apply with the new options succeeds.
        revert(home.path()).unwrap();
        apply(home.path(), &opts2).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(
            parsed["env"]["OTEL_RESOURCE_ATTRIBUTES"],
            "team=platform"
        );
    }
}
