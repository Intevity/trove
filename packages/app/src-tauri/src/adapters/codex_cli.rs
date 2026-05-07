//! Codex CLI adapter — patches `~/.codex/config.toml`'s `[otel]` section
//! with the OTLP/HTTP exporter tables Codex's config parser reads at
//! launch. Unlike the JSON adapters, the managed payload is a literal
//! TOML text block bracketed by `# trove:start` / `# trove:end` fences.
//!
//! Codex's schema (codex-rs/config/src/types.rs) is `deny_unknown_fields`
//! on every struct, so emit only the documented keys. The three
//! exporter slots — `[otel.exporter]` (logs), `[otel.trace_exporter]`,
//! `[otel.metrics_exporter]` — are independent; Trove sets all three so
//! a single Collector can ingest the full signal set. Verify the schema
//! against the upstream Codex repo at every adapter rev — codex-rs is
//! still adding fields.
//!
//! Known limitation: Codex gates `metrics_exporter` behind a separate
//! `[analytics] enabled = true` toggle. Trove does not write the
//! `[analytics]` section (it would collide with users who have their
//! own analytics config); metrics emission requires the user to opt in
//! via Codex's analytics switch independently. Surface this in the UI
//! when metrics counts stay flat.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::ipc::IpcError;
use crate::safety::atomic::write_atomic;
use crate::safety::backup::{backup_file, prune_backups};
use crate::safety::sentinels::{
    Format, ManagedRegion, SentinelError, extract_region, remove_region, upsert_region,
};

use super::{ApplyOptions, BACKUPS_TO_KEEP, PatchPreview, PreviewStatus, TrovePatch};

const CONFIG_DIR: &str = ".codex";
const CONFIG_FILE: &str = "config.toml";
const COLLECTOR_BASE: &str = "http://127.0.0.1:4318";

/// Resolve the absolute path of the Codex CLI config file under `home`.
/// Pure helper so tests can scope to a `tempdir`.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    home.join(CONFIG_DIR).join(CONFIG_FILE)
}

/// Compute the diff between the current file and what an apply with
/// `opts` would write.
pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    let path = config_path(home);
    let (current, _existed) = read_or_empty(&path)?;

    let region = build_region(opts).map_err(|e| IpcError::Internal {
        reason: format!("could not build managed region: {e}"),
    })?;

    let status = classify(&current, &region, &path)?;

    let after = upsert_region(Format::Toml, &current, &region).map_err(|e| match e {
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
        format: Format::Toml,
        before: current,
        after,
        status,
    })
}

/// Apply the patch. Same safety contract as the JSON adapters: backup,
/// atomic write, prune, and refuse to overwrite a managed region whose
/// hash differs from the new patch.
pub fn apply(home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    let path = config_path(home);
    let (current, existed) = read_or_empty(&path)?;

    let region = build_region(opts).map_err(|e| IpcError::Internal {
        reason: format!("could not build managed region: {e}"),
    })?;

    match classify(&current, &region, &path)? {
        PreviewStatus::Idempotent => {
            return Ok(TrovePatch {
                managed_block_hash: region.hash.clone(),
                file_hash_at_last_write: hash_hex(current.as_bytes()),
                format: Format::Toml,
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

    let after = upsert_region(Format::Toml, &current, &region).map_err(|e| match e {
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
        format: Format::Toml,
    })
}

/// Permissive revert — removes any Trove-managed region present.
pub fn revert(home: &Path) -> Result<(), IpcError> {
    let path = config_path(home);
    let (current, existed) = read_or_empty(&path)?;
    if !existed {
        return Ok(());
    }

    match extract_region(Format::Toml, &current) {
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

    let after = remove_region(Format::Toml, &current).map_err(|e| IpcError::ConfigUnparseable {
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
    match extract_region(Format::Toml, current) {
        Ok(Some(existing)) if existing.hash == region.hash => Ok(PreviewStatus::Idempotent),
        Ok(Some(_)) => Ok(PreviewStatus::Conflict),
        Ok(None) => Ok(PreviewStatus::Fresh),
        Err(e) => Err(IpcError::ConfigUnparseable {
            path: path.display().to_string(),
            reason: e.to_string(),
        }),
    }
}

/// Build the [`ManagedRegion`] for Codex's `[otel]` block. The payload
/// is rendered as deterministic TOML text — no `toml_edit` writer here
/// because the sentinel block is verbatim text between fences and the
/// engine validates the whole document parses after splicing.
///
/// `customAttributes` is intentionally a no-op for Codex: the upstream
/// schema has `deny_unknown_fields` and exposes no equivalent of
/// `OTEL_RESOURCE_ATTRIBUTES`. Trove's Collector pipeline tags signals
/// via the `resource/source` processor regardless, so cross-tool
/// dashboards still distinguish `trove.source = codex-cli`.
//
// The Result return is intentional even though `for_text_block` is
// infallible: it keeps this signature aligned with the JSON adapters
// (whose `for_json_patches` is fallible) so the Sprint-4 PR-3
// `HarnessSpec` refactor can use a single shared `fn` pointer type.
#[allow(clippy::unnecessary_wraps)]
fn build_region(opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    use std::fmt::Write as _;

    let mut payload = String::new();

    // Only emit the bare [otel] table when there's a top-level key to
    // place under it. Otherwise the sub-tables [otel.exporter] etc.
    // implicitly establish the namespace, and we avoid colliding with
    // any user-written [otel] section elsewhere in the file (TOML
    // rejects duplicate top-level table definitions).
    if opts.log_user_prompts {
        payload.push_str("[otel]\n");
        payload.push_str("log_user_prompt = true\n\n");
    }

    payload.push_str("[otel.exporter]\n");
    payload.push_str("kind = \"otlp-http\"\n");
    let _ = writeln!(payload, "endpoint = \"{COLLECTOR_BASE}/v1/logs\"");
    payload.push_str("protocol = \"binary\"\n\n");

    payload.push_str("[otel.trace_exporter]\n");
    payload.push_str("kind = \"otlp-http\"\n");
    let _ = writeln!(payload, "endpoint = \"{COLLECTOR_BASE}/v1/traces\"");
    payload.push_str("protocol = \"binary\"\n\n");

    payload.push_str("[otel.metrics_exporter]\n");
    payload.push_str("kind = \"otlp-http\"\n");
    let _ = writeln!(payload, "endpoint = \"{COLLECTOR_BASE}/v1/metrics\"");
    payload.push_str("protocol = \"binary\"\n");

    let mut keys = vec![
        "otel.exporter".to_string(),
        "otel.trace_exporter".to_string(),
        "otel.metrics_exporter".to_string(),
    ];
    if opts.log_user_prompts {
        keys.insert(0, "otel.log_user_prompt".to_string());
    }

    Ok(ManagedRegion::for_text_block(payload, keys))
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
    fn fresh_install_creates_a_valid_config_file() {
        let home = tempdir().unwrap();
        let patch = apply(home.path(), &ApplyOptions::default()).unwrap();

        let written = read_config(home.path());
        // The file must parse as TOML with our exporter tables in place.
        let doc: toml_edit::DocumentMut = written.parse().unwrap();
        let exporter = doc["otel"]["exporter"].as_table().unwrap();
        assert_eq!(exporter["kind"].as_str(), Some("otlp-http"));
        assert_eq!(
            exporter["endpoint"].as_str(),
            Some("http://127.0.0.1:4318/v1/logs")
        );
        assert_eq!(exporter["protocol"].as_str(), Some("binary"));
        assert!(doc["otel"]["trace_exporter"].is_table());
        assert!(doc["otel"]["metrics_exporter"].is_table());

        assert!(written.contains("# trove:start"), "missing sentinel fence");
        assert!(written.contains("# trove:end"));

        assert_eq!(patch.managed_block_hash.len(), 64);
        assert_eq!(patch.file_hash_at_last_write.len(), 64);
        assert_eq!(patch.format, Format::Toml);
    }

    // --- 2. Idempotent re-apply ---------------------------------------------

    #[test]
    fn idempotent_reapply_does_not_change_the_file() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_first = read_config(home.path());

        let second = apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_second = read_config(home.path());
        assert_eq!(after_first, after_second);
        let expected = hash_hex(after_second.as_bytes());
        assert_eq!(second.file_hash_at_last_write, expected);
    }

    // --- 3. User-edited outside the managed block --------------------------

    #[test]
    fn user_keys_outside_block_survive_apply() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // User has unrelated top-level tables. We deliberately pick a
        // namespace that doesn't collide with [otel] — duplicate top-level
        // tables would be a TOML parse error post-splice.
        fs::write(
            &path,
            "[user]\nname = \"jeff\"\n\n[model]\ndefault = \"o1\"\n",
        )
        .unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after = read_config(home.path());
        let doc: toml_edit::DocumentMut = after.parse().unwrap();
        assert_eq!(doc["user"]["name"].as_str(), Some("jeff"));
        assert_eq!(doc["model"]["default"].as_str(), Some("o1"));
        assert_eq!(
            doc["otel"]["exporter"]["endpoint"].as_str(),
            Some("http://127.0.0.1:4318/v1/logs")
        );
    }

    // --- 4. User-edited inside the managed block (conflict) ----------------

    #[test]
    fn editing_inside_the_managed_block_yields_conflict() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        // Tamper with one of Trove's managed values without touching
        // the sentinel header.
        let path = config_path(home.path());
        let written = read_config(home.path());
        let edited = written.replace(
            "http://127.0.0.1:4318/v1/logs",
            "http://attacker.example.com/v1/logs",
        );
        assert_ne!(edited, written);
        fs::write(&path, &edited).unwrap();

        let result = apply(home.path(), &ApplyOptions::default());
        match result {
            Err(IpcError::RegionConflict { path: p }) => {
                assert_eq!(p, path.display().to_string());
            }
            other => panic!("expected RegionConflict, got {other:?}"),
        }
        // Conflicting file is left untouched.
        assert_eq!(read_config(home.path()), edited);
    }

    // --- 5. Malformed file --------------------------------------------------

    #[test]
    fn malformed_file_is_unparseable_error() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not = toml [unclosed").unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
        assert!(
            matches!(err, IpcError::ConfigUnparseable { .. }),
            "expected ConfigUnparseable, got {err:?}"
        );
        // Untouched.
        assert_eq!(read_config(home.path()), "{ not = toml [unclosed");
    }

    // --- 6. Missing parent dir ---------------------------------------------

    #[test]
    fn missing_parent_dir_is_created_automatically() {
        let home = tempdir().unwrap();
        assert!(!home.path().join(".codex").exists());
        apply(home.path(), &ApplyOptions::default()).unwrap();
        assert!(home.path().join(".codex").exists());
        assert!(config_path(home.path()).exists());
    }

    // --- 7. Read-only parent dir → IO error --------------------------------

    #[cfg(unix)]
    #[test]
    fn readonly_parent_dir_yields_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempdir().unwrap();
        let parent = home.path().join(".codex");
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
        // Restore so tempdir cleanup works.
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(err, IpcError::Io { .. }),
            "expected Io error, got {err:?}"
        );
    }

    // --- Revert round-trip --------------------------------------------------

    #[test]
    fn revert_restores_byte_identical_pre_apply_file() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "[user]\nname = \"jeff\"\n\n[model]\ndefault = \"o1\"\n";
        fs::write(&path, original).unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();
        revert(home.path()).unwrap();

        let after = read_config(home.path());
        // Byte-identity including the trailing newline.
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
        let user_only = "[user]\nname = \"jeff\"\n";
        fs::write(&path, user_only).unwrap();
        revert(home.path()).unwrap();
        assert_eq!(read_config(home.path()), user_only);
    }

    #[test]
    fn revert_on_malformed_file_returns_unparseable_error() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ broken").unwrap();
        let err = revert(home.path()).unwrap_err();
        assert!(matches!(err, IpcError::ConfigUnparseable { .. }));
    }

    // --- Preview --------------------------------------------------------------

    #[test]
    fn preview_on_missing_file_returns_fresh_status() {
        let home = tempdir().unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Fresh);
        assert_eq!(preview.format, Format::Toml);
        assert_eq!(preview.before, "");
        assert!(preview.after.contains("[otel.exporter]"));
        assert!(preview.after.contains("# trove:start"));
    }

    #[test]
    fn preview_after_apply_returns_idempotent_status() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Idempotent);
        assert_eq!(preview.after, preview.before);
    }

    #[test]
    fn preview_with_tampered_block_returns_conflict_status() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let path = config_path(home.path());
        let edited = read_config(home.path()).replace(
            "http://127.0.0.1:4318/v1/logs",
            "http://x.example.com/v1/logs",
        );
        fs::write(&path, &edited).unwrap();

        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Conflict);
    }

    // --- log_user_prompts toggle --------------------------------------------

    #[test]
    fn log_user_prompts_propagates_to_log_user_prompt_when_true() {
        let home = tempdir().unwrap();
        let opts = ApplyOptions {
            log_user_prompts: true,
            ..Default::default()
        };
        apply(home.path(), &opts).unwrap();

        let written = read_config(home.path());
        let doc: toml_edit::DocumentMut = written.parse().unwrap();
        // log_user_prompt lives on the [otel] table itself, not on an
        // exporter. Note the singular spelling — that's Codex's name.
        assert_eq!(
            doc["otel"]["log_user_prompt"].as_bool(),
            Some(true),
            "expected otel.log_user_prompt = true; got {written}"
        );
    }

    #[test]
    fn log_user_prompts_default_omits_the_key() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let written = read_config(home.path());
        // No bare [otel] section appears — only the exporter sub-tables.
        assert!(
            !written.contains("log_user_prompt"),
            "log_user_prompt should be omitted by default; got {written}"
        );
    }

    // --- Custom attributes are intentionally a no-op for Codex --------------

    #[test]
    fn custom_attributes_are_a_noop_for_codex() {
        // Codex's deny_unknown_fields schema has no equivalent of
        // OTEL_RESOURCE_ATTRIBUTES; resource tagging happens at the
        // Collector. Two applies with different custom_attributes
        // produce identical bytes.
        let home_a = tempdir().unwrap();
        let home_b = tempdir().unwrap();

        let mut opts = ApplyOptions::default();
        opts.custom_attributes
            .insert("team".into(), "platform".into());

        apply(home_a.path(), &ApplyOptions::default()).unwrap();
        apply(home_b.path(), &opts).unwrap();

        assert_eq!(read_config(home_a.path()), read_config(home_b.path()));
    }

    // --- Conflict surfacing when changing options ---------------------------

    #[test]
    fn changing_options_between_applies_yields_conflict_until_reverted() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        let opts2 = ApplyOptions {
            log_user_prompts: true,
            ..Default::default()
        };
        let err = apply(home.path(), &opts2).unwrap_err();
        assert!(matches!(err, IpcError::RegionConflict { .. }));

        revert(home.path()).unwrap();
        apply(home.path(), &opts2).unwrap();
        let written = read_config(home.path());
        let doc: toml_edit::DocumentMut = written.parse().unwrap();
        assert_eq!(doc["otel"]["log_user_prompt"].as_bool(), Some(true));
    }

    // --- Pre-existing user-managed [otel.exporter] section conflicts -------

    #[test]
    fn pre_existing_otel_exporter_section_yields_unparseable_on_apply() {
        // If the user already has [otel.exporter] in their file, our
        // splice produces a duplicate top-level table → TOML parse
        // error post-splice. The safety contract surfaces this as
        // ConfigUnparseable rather than silently overwriting.
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "[otel.exporter]\nkind = \"none\"\n";
        fs::write(&path, original).unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
        assert!(
            matches!(err, IpcError::ConfigUnparseable { .. }),
            "expected ConfigUnparseable, got {err:?}"
        );
        // The user's file is untouched on failure.
        assert_eq!(read_config(home.path()), original);
    }
}
