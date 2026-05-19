//! Shared adapter scaffolding.
//!
//! All Tier 1 adapters follow the same `preview` / `apply` / `revert`
//! shape — only the host file path, file format, and managed-region
//! payload vary per harness. This module factors out the rest.
//!
//! ## Adding a new adapter
//!
//! Each per-harness module declares a `const SPEC: HarnessSpec` and
//! delegates its public API to `common::{config_path, preview, apply,
//! revert}`. The only piece of code that must live in the per-harness
//! module is `build_region`, the function that turns [`ApplyOptions`]
//! into the [`ManagedRegion`] Trove will install. See
//! `documentation/adding-a-harness.md` for the step-by-step.
//!
//! ## Why a `HarnessSpec` and free functions, not a trait
//!
//! The adapters all have the same signatures and never need to be
//! stored as `dyn`-objects (the IPC layer dispatches via a static
//! `match` over [`crate::harness::HarnessId`]). A trait would add an
//! `impl Adapter for ClaudeCode {}` ceremony that buys nothing over a
//! `const SPEC` initialiser.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::ipc::IpcError;
use crate::safety::atomic::write_atomic;
use crate::safety::backup::{backup_file, prune_backups};
use crate::safety::sentinels::{
    Format, ManagedRegion, SentinelError, extract_region, remove_region, upsert_region,
};

use super::{ApplyOptions, BACKUPS_TO_KEEP, PatchPreview, PreviewStatus, TrovePatch};

/// Per-harness "where do I patch?" plus "what do I write?" descriptor.
/// Each adapter declares one as a `const SPEC` and delegates its
/// public API to the free functions in this module.
pub struct HarnessSpec {
    /// Adapter id used for shared-block dependency tracking in the
    /// comment-fence formats (TOML / YAML / Shell). The fence header
    /// records the set of adapters that depend on the block; the block
    /// is stripped only when the last dep is removed. JSON / JSONC
    /// adapters ignore this field — their `_trove` marker has a single
    /// owner today. Set to the [`crate::harness::HarnessId`]'s serde
    /// rename (e.g. `"codex-cli"`).
    pub adapter_id: &'static str,
    /// Directory under `$HOME` that holds the harness's config (e.g.
    /// `".claude"`, `".gemini"`).
    pub config_dir: &'static str,
    /// File name within `config_dir` (e.g. `"settings.json"`,
    /// `"config.toml"`).
    pub config_file: &'static str,
    /// File format the sentinel engine should target.
    pub format: Format,
    /// Per-harness region builder. Must be deterministic — the same
    /// `ApplyOptions` always produces a region with the same hash, so
    /// `apply` is idempotent across re-runs.
    pub build_region: fn(&ApplyOptions) -> Result<ManagedRegion, SentinelError>,
}

/// Resolve the absolute path of the harness's config file under `home`.
#[must_use]
pub fn config_path(spec: &HarnessSpec, home: &Path) -> PathBuf {
    home.join(spec.config_dir).join(spec.config_file)
}

/// Compute the diff between the current file and what an `apply` with
/// `opts` would write.
pub fn preview(
    spec: &HarnessSpec,
    home: &Path,
    opts: &ApplyOptions,
) -> Result<PatchPreview, IpcError> {
    let region = (spec.build_region)(opts).map_err(|e| IpcError::Internal {
        reason: format!("could not build managed region: {e}"),
    })?;
    preview_with_region(spec, home, &region)
}

/// Like [`preview`] but takes a pre-built [`ManagedRegion`]. Adapters
/// whose region depends on runtime context the static `build_region`
/// fn-pointer can't access (currently only the Cursor adapters, which
/// need a resolved hook-script path) build the region themselves and
/// call this directly.
pub fn preview_with_region(
    spec: &HarnessSpec,
    home: &Path,
    region: &ManagedRegion,
) -> Result<PatchPreview, IpcError> {
    let path = config_path(spec, home);
    let (current, _existed) = read_or_empty(&path)?;
    let working = working_value(spec.format, &current);

    let status = classify(spec.format, &working, region, spec.adapter_id, &path)?;

    let after =
        upsert_region(spec.format, &working, region, spec.adapter_id)
            .map_err(|e| map_sentinel_err(e, &path))?;

    Ok(PatchPreview {
        config_path: path,
        format: spec.format,
        before: current,
        after,
        status,
    })
}

/// Apply the patch. Backs the existing file up, atomically writes the
/// new content, and prunes old backups. Idempotent when the existing
/// managed region matches what we'd write; refuses with
/// [`IpcError::RegionConflict`] when it doesn't.
pub fn apply(
    spec: &HarnessSpec,
    home: &Path,
    opts: &ApplyOptions,
) -> Result<TrovePatch, IpcError> {
    let region = (spec.build_region)(opts).map_err(|e| IpcError::Internal {
        reason: format!("could not build managed region: {e}"),
    })?;
    apply_with_region(spec, home, &region)
}

/// Like [`apply`] but takes a pre-built [`ManagedRegion`]. Same caveat
/// as [`preview_with_region`] — used by adapters whose payload depends
/// on runtime context.
pub fn apply_with_region(
    spec: &HarnessSpec,
    home: &Path,
    region: &ManagedRegion,
) -> Result<TrovePatch, IpcError> {
    let path = config_path(spec, home);
    let (current, existed) = read_or_empty(&path)?;
    let working = working_value(spec.format, &current);

    match classify(spec.format, &working, region, spec.adapter_id, &path)? {
        PreviewStatus::Idempotent => {
            return Ok(TrovePatch {
                managed_block_hash: region.hash.clone(),
                file_hash_at_last_write: hash_hex(current.as_bytes()),
                format: spec.format,
                last_written_region_payload: region.payload.clone(),
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

    let after =
        upsert_region(spec.format, &working, region, spec.adapter_id)
            .map_err(|e| map_sentinel_err(e, &path))?;

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
        format: spec.format,
        last_written_region_payload: region.payload.clone(),
    })
}

/// Permissive revert — removes any Trove-managed region present. No-op
/// when the file is missing or contains no managed region.
pub fn revert(spec: &HarnessSpec, home: &Path) -> Result<(), IpcError> {
    let path = config_path(spec, home);
    let (current, existed) = read_or_empty(&path)?;
    if !existed {
        return Ok(());
    }

    match extract_region(spec.format, &current) {
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

    let after = remove_region(spec.format, &current, spec.adapter_id).map_err(|e| {
        IpcError::ConfigUnparseable {
            path: path.display().to_string(),
            reason: e.to_string(),
        }
    })?;

    write_atomic(&path, after.as_bytes()).map_err(|e| IpcError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let _ = prune_backups(&path, BACKUPS_TO_KEEP);

    Ok(())
}

/// Read the host file or return an empty string if absent. The boolean
/// distinguishes "missing" from "present and empty" — `apply` skips
/// the backup step when the file didn't exist before.
pub(super) fn read_or_empty(path: &Path) -> Result<(String, bool), IpcError> {
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
///
/// `adapter_id` is consulted for the comment-fence formats (TOML /
/// YAML / Shell): if a managed block already exists with a matching
/// hash but `adapter_id` is not yet in its dependents list, the
/// classification is `Fresh` (apply will write to add the dep). For
/// JSON / JSONC the dependents list is always empty and `adapter_id`
/// is ignored.
pub(super) fn classify(
    format: Format,
    current: &str,
    region: &ManagedRegion,
    adapter_id: &str,
    path: &Path,
) -> Result<PreviewStatus, IpcError> {
    match extract_region(format, current) {
        Ok(Some(existing)) if existing.hash == region.hash => {
            let needs_dep_mutation = matches!(
                format,
                Format::Toml | Format::Yaml | Format::Shell
            ) && !existing.dependents.iter().any(|d| d == adapter_id);
            if needs_dep_mutation {
                Ok(PreviewStatus::Fresh)
            } else {
                Ok(PreviewStatus::Idempotent)
            }
        }
        Ok(Some(_)) => Ok(PreviewStatus::Conflict),
        Ok(None) => Ok(PreviewStatus::Fresh),
        Err(e) => Err(IpcError::ConfigUnparseable {
            path: path.display().to_string(),
            reason: e.to_string(),
        }),
    }
}

/// Hex-encoded SHA-256 of `bytes`. Used by adapters to record the
/// post-write file hash in the returned [`TrovePatch`].
#[must_use]
pub(super) fn hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Fallback content the upsert path operates on when the host file is
/// absent or empty. JSON-family formats need an empty object so the
/// JSON parser is happy; comment-fenced formats can start from an
/// empty string because [`crate::safety::sentinels`] accepts empty
/// TOML/YAML input.
fn working_value(format: Format, current: &str) -> String {
    if !current.is_empty() {
        return current.to_string();
    }
    match format {
        Format::Json | Format::Jsonc => "{}".to_string(),
        Format::Toml | Format::Yaml | Format::Shell => String::new(),
    }
}

/// Map a [`SentinelError`] from `upsert_region` to the right
/// [`IpcError`] variant. Malformed input or a malformed managed region
/// surface as `ConfigUnparseable` (the UI shows a parse error and
/// disables apply); anything else is an `Internal` bug.
fn map_sentinel_err(err: SentinelError, path: &Path) -> IpcError {
    match err {
        SentinelError::Malformed { .. } | SentinelError::RegionMalformed(_) => {
            IpcError::ConfigUnparseable {
                path: path.display().to_string(),
                reason: err.to_string(),
            }
        }
        other => IpcError::Internal {
            reason: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_value_substitutes_empty_object_for_json_when_current_is_empty() {
        assert_eq!(working_value(Format::Json, ""), "{}");
        assert_eq!(working_value(Format::Jsonc, ""), "{}");
    }

    #[test]
    fn working_value_substitutes_empty_string_for_toml_yaml_shell_when_current_is_empty() {
        assert_eq!(working_value(Format::Toml, ""), "");
        assert_eq!(working_value(Format::Yaml, ""), "");
        assert_eq!(working_value(Format::Shell, ""), "");
    }

    #[test]
    fn working_value_preserves_nonempty_input_regardless_of_format() {
        for f in [
            Format::Json,
            Format::Jsonc,
            Format::Toml,
            Format::Yaml,
            Format::Shell,
        ] {
            assert_eq!(working_value(f, r#"{"a":1}"#), r#"{"a":1}"#);
        }
    }

    #[test]
    fn hash_hex_is_64_hex_chars() {
        let h = hash_hex(b"hello world");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_hex_is_deterministic() {
        assert_eq!(hash_hex(b"hello"), hash_hex(b"hello"));
        assert_ne!(hash_hex(b"hello"), hash_hex(b"world"));
    }

    #[test]
    fn map_sentinel_err_routes_malformed_to_unparseable() {
        let err = SentinelError::Malformed {
            format: Format::Json,
            message: "bad".into(),
        };
        let mapped = map_sentinel_err(err, Path::new("/tmp/x"));
        assert!(matches!(mapped, IpcError::ConfigUnparseable { .. }));
    }

    #[test]
    fn map_sentinel_err_routes_other_to_internal() {
        let err = SentinelError::MultipleRegions;
        let mapped = map_sentinel_err(err, Path::new("/tmp/x"));
        assert!(matches!(mapped, IpcError::Internal { .. }));
    }
}
