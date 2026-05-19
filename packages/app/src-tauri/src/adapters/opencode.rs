// Module-level doc prose mentions OpenCode / OpenTelemetry / OTel as
// bare English nouns; backticking each occurrence would clutter the
// narrative. Code-identifier references inside the doc still use
// backticks normally.
#![allow(clippy::doc_markdown)]

//! OpenCode adapter — patches `~/.config/opencode/opencode.json` to
//! register the upstream `@devtheops/opencode-plugin-otel` plugin.
//! OpenCode's Bun-based runtime resolves and installs the package
//! itself at next launch; Trove ships no vendored source.
//!
//! Why no vendoring (deviation from the original Sprint 7 plan): the
//! upstream plugin pulls 14 OpenTelemetry / OpenCode SDK runtime
//! dependencies that need ~100 MB of `node_modules` to actually run,
//! and OpenCode's runtime doesn't bundle them. Vendoring the source
//! without the deps would produce a non-functional plugin; vendoring
//! source plus deps would bloat the Tauri bundle past the 30–50 MB
//! target the design decisions call for. Letting OpenCode resolve
//! the package from npm matches the plugin's own README and keeps
//! Trove's surface area narrow.
//!
//! Telemetry is configured via env vars in the user's shell (per the
//! plugin's README) — `OPENCODE_ENABLE_TELEMETRY=1` and an optional
//! `OPENCODE_OTLP_ENDPOINT`. The default endpoint is `localhost:4317`
//! (gRPC), which is exactly where Trove's collector binds; users get
//! correct routing without further config the moment they enable
//! `OPENCODE_ENABLE_TELEMETRY`. Sprint 7 PR 3's UI polish surfaces
//! this requirement to the user.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ipc::IpcError;
use crate::safety::sentinels::{Format, ManagedRegion, SentinelError};

use super::common::{self, HarnessSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

/// The npm package name OpenCode resolves and loads at startup.
pub(crate) const OTEL_PLUGIN_PACKAGE: &str = "@devtheops/opencode-plugin-otel";
/// OpenCode's documented JSON-Schema URL. Adding it gives the user
/// editor autocomplete; if their file already carries a different
/// `$schema` value Trove's leaf-path merge will overwrite it (see
/// the trade-off note on `build_region`).
const OPENCODE_SCHEMA_URL: &str = "https://opencode.ai/config.json";

const SPEC: HarnessSpec = HarnessSpec {
    config_dir: ".config/opencode",
    config_file: "opencode.json",
    format: Format::Json,
    build_region,
};

/// Resolve the absolute path of `~/.config/opencode/opencode.json`
/// under `home`. Note: this hard-codes the XDG-default path. Detection
/// (`detect/paths.rs`) honours `$XDG_CONFIG_HOME` on Linux; the adapter
/// itself follows the canonical convention, since users who relocate
/// their config dir typically also drive the file through their own
/// tooling rather than a tray app.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    common::config_path(&SPEC, home)
}

/// Compute the diff between the current file and what an apply with
/// `opts` would write.
pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    common::preview(&SPEC, home, opts)
}

/// Apply the patch. See [`common::apply`] for the safety contract.
pub fn apply(home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    common::apply(&SPEC, home, opts)
}

/// Remove the OpenCode managed leaves from the host file.
///
/// Custom (does not delegate to [`common::revert`]) because the adapter
/// opts out of the `_trove` top-level marker — opencode's strict JSON
/// schema rejects unknown top-level keys. Without the marker the
/// generic revert path can't tell which leaves are ours, so we read
/// the canonical `managed_keys` from [`build_region`] and strip them
/// directly. Also tolerates old-style files that still carry `_trove`
/// (from a build that predated the opt-out) by removing it too.
pub fn revert(home: &Path) -> Result<(), IpcError> {
    use crate::safety::atomic::write_atomic;
    use crate::safety::backup::{backup_file, prune_backups};
    use crate::adapters::BACKUPS_TO_KEEP;

    let path = config_path(home);
    let current = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(IpcError::Io {
                path: path.display().to_string(),
                reason: e.to_string(),
            });
        }
    };

    let region =
        build_region(&ApplyOptions::default()).map_err(|e| IpcError::Internal {
            reason: format!("could not derive managed_keys for revert: {e}"),
        })?;

    let mut value: Value =
        serde_json::from_str(&current).map_err(|e| IpcError::ConfigUnparseable {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
    let Value::Object(obj) = &mut value else {
        return Err(IpcError::ConfigUnparseable {
            path: path.display().to_string(),
            reason: "host JSON document is not an object".into(),
        });
    };

    let mut changed = false;
    // Remove any legacy `_trove` marker left over from a previous build.
    if obj.shift_remove("_trove").is_some() {
        changed = true;
    }
    // Remove the leaves the adapter manages. `managed_keys` for opencode
    // are top-level only (`$schema`, `plugin`), so dotted-path traversal
    // isn't required.
    for key in &region.managed_keys {
        if obj.shift_remove(key).is_some() {
            changed = true;
        }
    }

    if !changed {
        return Ok(());
    }

    let mut after = serde_json::to_string_pretty(&value).map_err(|e| IpcError::Internal {
        reason: format!("could not emit revert payload: {e}"),
    })?;
    after.push('\n');

    backup_file(&path).map_err(|e| IpcError::Io {
        path: path.display().to_string(),
        reason: format!("backup failed: {e}"),
    })?;
    write_atomic(&path, after.as_bytes()).map_err(|e| IpcError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let _ = prune_backups(&path, BACKUPS_TO_KEEP);
    Ok(())
}

/// Build the [`ManagedRegion`] for the OpenCode patch.
///
/// **Trade-off note:** the region's `plugin` leaf is the *full array*
/// `["@devtheops/opencode-plugin-otel"]`. If the user already has
/// other plugins registered there (rare but possible), Trove's apply
/// will replace the array. Recovery: revert (which removes the array
/// entirely), then the user re-registers their plugins alongside the
/// OTel one. The Tier 1 adapters carry the same trade-off for their
/// merged leaves (e.g. Claude Code's `env.OTEL_*`); the cursor
/// adapters carry it for `hooks.beforeShellExecution`. Sprint 8's
/// three-way merge UI handles the conflict path; Sprint 7 just
/// refuses with `IpcError::RegionConflict` if a non-Trove value is
/// present at the leaf.
fn build_region(opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    // ApplyOptions has no OpenCode-specific fields today: the plugin's
    // `OPENCODE_LOG_USER_PROMPTS`-equivalent is gated by env vars Trove
    // doesn't manage (the plugin reads them from the user's shell). The
    // unused-arg lint stays silenced via the discard below.
    let _ = opts;

    let mut top = serde_json::Map::new();
    top.insert(
        "$schema".to_string(),
        Value::String(OPENCODE_SCHEMA_URL.to_string()),
    );
    top.insert(
        "plugin".to_string(),
        Value::Array(vec![Value::String(OTEL_PLUGIN_PACKAGE.to_string())]),
    );

    // Opt out of the `_trove` top-level marker: opencode's JSON schema
    // (`https://opencode.ai/config.json`) rejects unknown top-level keys
    // and the CLI refuses to start with `Unrecognized key: _trove`.
    // `revert` (above) re-derives `managed_keys` from this same
    // `build_region` call, so the marker isn't needed for cleanup.
    ManagedRegion::for_json_patches_no_marker(&top)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::PreviewStatus;
    use std::fs;
    use tempfile::tempdir;

    fn read_config(home: &Path) -> String {
        fs::read_to_string(config_path(home)).unwrap()
    }

    // --- 1. Fresh install -----------------------------------------------------

    #[test]
    fn fresh_install_creates_a_valid_opencode_json() {
        let home = tempdir().unwrap();
        let patch = apply(home.path(), &ApplyOptions::default()).unwrap();

        let written = read_config(home.path());
        let parsed: Value = serde_json::from_str(&written).unwrap();

        assert_eq!(parsed["$schema"], OPENCODE_SCHEMA_URL);
        let plugins = parsed.get("plugin").and_then(Value::as_array).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0], OTEL_PLUGIN_PACKAGE);
        // Opencode's CLI rejects unknown top-level keys (it validates
        // against its strict JSON schema). Trove opts out of the
        // `_trove` marker for this host — see `build_region`.
        assert!(
            parsed.get("_trove").is_none(),
            "opencode opts out of the _trove marker"
        );

        assert_eq!(patch.managed_block_hash.len(), 64);
        assert_eq!(patch.file_hash_at_last_write.len(), 64);
        assert_eq!(patch.format, Format::Json);
    }

    // --- 2. Idempotent re-apply ----------------------------------------------

    #[test]
    fn reapply_produces_byte_identical_content() {
        // Without the `_trove` marker the conflict-detector treats every
        // re-apply as Fresh. The write path still produces deterministic
        // output (sorted keys, fixed leaves) so the resulting file is
        // byte-identical even though a new backup is created.
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_first = read_config(home.path());

        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_second = read_config(home.path());
        assert_eq!(after_first, after_second);
    }

    // --- 3. User-edited outside the managed block ---------------------------

    #[test]
    fn user_keys_outside_block_survive_apply() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"theme":"midnight","mcp":{"someServer":"keepme"}}"#,
        )
        .unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(parsed["theme"], "midnight");
        assert_eq!(parsed["mcp"]["someServer"], "keepme");
        assert_eq!(parsed["plugin"][0], OTEL_PLUGIN_PACKAGE);
    }

    // --- 4. User-edited inside the managed leaves (re-apply restores) -----

    #[test]
    fn re_apply_after_user_edit_restores_managed_leaves() {
        // Trade-off: opencode opts out of `_trove`, so the conflict
        // detector can't tell that a user replaced our `plugin` value
        // with something else. Apply silently restores the canonical
        // value. Documented in `build_region`'s trade-off note.
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        let path = config_path(home.path());
        let written = read_config(home.path());
        let edited = written.replace(
            OTEL_PLUGIN_PACKAGE,
            "@attacker/totally-not-malicious",
        );
        assert_ne!(edited, written);
        fs::write(&path, &edited).unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(parsed["plugin"][0], OTEL_PLUGIN_PACKAGE);
        assert!(
            !read_config(home.path()).contains("@attacker"),
            "re-apply must overwrite the attacker entry"
        );
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
        assert_eq!(read_config(home.path()), "{not valid json");
    }

    // --- 6. Missing parent dir ----------------------------------------------

    #[test]
    fn missing_parent_dir_is_created_automatically() {
        let home = tempdir().unwrap();
        assert!(!home.path().join(".config").join("opencode").exists());
        apply(home.path(), &ApplyOptions::default()).unwrap();
        assert!(home.path().join(".config").join("opencode").exists());
        assert!(config_path(home.path()).exists());
    }

    // --- 7. Read-only parent dir → IO error ---------------------------------

    #[cfg(unix)]
    #[test]
    fn readonly_parent_dir_yields_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempdir().unwrap();
        let parent = home.path().join(".config").join("opencode");
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
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
        let original = "{\n  \"theme\": \"midnight\",\n  \"mcp\": {\n    \"someServer\": \"keepme\"\n  }\n}\n";
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
        let user_only = "{\"theme\":\"midnight\"}";
        fs::write(&path, user_only).unwrap();
        revert(home.path()).unwrap();
        assert_eq!(read_config(home.path()), user_only);
    }

    // --- Bug C: `_trove` marker opt-out -------------------------------------

    #[test]
    fn apply_does_not_write_top_level_trove_key() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert!(parsed.get("_trove").is_none());
        assert_eq!(parsed["$schema"], OPENCODE_SCHEMA_URL);
        assert_eq!(parsed["plugin"][0], OTEL_PLUGIN_PACKAGE);
    }

    #[test]
    fn revert_uses_build_region_managed_keys_when_no_trove_marker() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Hand-write the canonical applied form (no `_trove`, has the
        // two managed leaves) — simulates the new no-marker apply.
        fs::write(
            &path,
            "{\n  \"theme\": \"midnight\",\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"plugin\": [\"@devtheops/opencode-plugin-otel\"]\n}\n",
        )
        .unwrap();

        revert(home.path()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert!(parsed.get("$schema").is_none(), "schema leaf removed");
        assert!(parsed.get("plugin").is_none(), "plugin leaf removed");
        assert_eq!(parsed["theme"], "midnight", "user key survives");
    }

    #[test]
    fn revert_strips_legacy_trove_marker_if_present() {
        // Files from a previous build still carry `_trove`. Revert
        // must remove it alongside the managed leaves so a clean
        // re-apply doesn't drag the deprecated marker forward.
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"theme":"midnight","$schema":"https://opencode.ai/config.json","plugin":["@devtheops/opencode-plugin-otel"],"_trove":{"managed_keys":["$schema","plugin"],"hash":"deadbeef"}}"#,
        )
        .unwrap();

        revert(home.path()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert!(parsed.get("_trove").is_none());
        assert!(parsed.get("$schema").is_none());
        assert!(parsed.get("plugin").is_none());
        assert_eq!(parsed["theme"], "midnight");
    }

    // --- Preview --------------------------------------------------------------

    #[test]
    fn preview_on_missing_file_returns_fresh_status() {
        let home = tempdir().unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Fresh);
        assert_eq!(preview.format, Format::Json);
        assert_eq!(preview.before, "");
        assert!(preview.after.contains(OTEL_PLUGIN_PACKAGE));
        assert!(preview.after.contains(OPENCODE_SCHEMA_URL));
    }

    #[test]
    fn preview_after_apply_reports_fresh_with_unchanged_diff() {
        // Trade-off of the `_trove` opt-out: classify can't tell that
        // the leaves are already installed, so status stays Fresh on
        // every re-preview. The diff is empty (before == after), which
        // the UI uses to suppress the apply-button affordance.
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Fresh);
        assert_eq!(preview.after, preview.before);
    }

    #[test]
    fn preview_with_tampered_block_reports_fresh_diff() {
        // Same trade-off — the conflict detector is unavailable without
        // `_trove`. The diff is non-empty (apply would overwrite the
        // edit) and the UI surfaces that to the user before they apply.
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let path = config_path(home.path());
        let edited = read_config(home.path()).replace(
            OTEL_PLUGIN_PACKAGE,
            "@attacker/evil",
        );
        fs::write(&path, &edited).unwrap();

        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Fresh);
        assert_ne!(preview.after, preview.before);
        assert!(preview.after.contains(OTEL_PLUGIN_PACKAGE));
        assert!(!preview.after.contains("@attacker"));
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
}
