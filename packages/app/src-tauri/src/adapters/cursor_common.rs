//! Shared writer for the two Cursor adapters (`cursor_ide` + `cursor_cli`).
//!
//! Both Cursor harnesses share a single host file, `~/.cursor/hooks.json`,
//! and a single managed region inside it. Trove ships a vendored Node
//! script (`resources/hooks/cursor-otel-hook.cjs`) that turns each Cursor
//! hook event into an OTLP HTTP/JSON payload aimed at the local
//! collector. The adapter writes two `hooks.json` entries — one each for
//! `beforeShellExecution` and `afterShellExecution` — both pointing at
//! the bundled script's absolute path.
//!
//! Why a single shared writer:
//!
//! - The host file is shared between Cursor IDE and Cursor CLI; writing
//!   a separate sentinel block per harness would produce duplicate hook
//!   entries when both are enabled, and reverting one would silently
//!   uninstall the other's coverage.
//! - The two harness-level adapters (`cursor_ide.rs`, `cursor_cli.rs`)
//!   exist to give the user separate enable / disable toggles in the UI;
//!   the underlying patch is identical, so re-applying via either is a
//!   no-op (idempotent) and reverting via either fully removes the
//!   block.
//!
//! Why we don't use the static `HarnessSpec` `build_region` fn-pointer:
//! the region payload depends on the resolved absolute path of the
//! bundled hook script, which only the Tauri layer can resolve via
//! `tauri::path::PathResolver::resource_dir()`. Function pointers can't
//! capture environment, so we expose a `build_region(opts, hook_path)`
//! free function and call `common::apply_with_region` /
//! `common::preview_with_region` directly. The SPEC's `build_region`
//! field is a panic-loud placeholder so an accidental call to
//! `common::apply(&SPEC, ...)` surfaces a clear error rather than
//! producing a region without a hook path.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::ipc::IpcError;
use crate::safety::sentinels::{Format, ManagedRegion, SentinelError};

use super::common::{self, HarnessSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

const SPEC: HarnessSpec = HarnessSpec {
    config_dir: ".cursor",
    config_file: "hooks.json",
    format: Format::Json,
    build_region: build_region_placeholder,
};

/// Resolve `~/.cursor/hooks.json` under `home`. The two cursor adapters
/// re-export this so callers don't need to know about the shared SPEC.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    common::config_path(&SPEC, home)
}

/// Compute the diff Trove would write into `~/.cursor/hooks.json` for
/// the cursor adapters. `hook_script_path` is the absolute path of the
/// bundled `cursor-otel-hook.cjs`, resolved by the IPC layer at runtime
/// via Tauri's `resource_dir()`.
pub fn preview(
    home: &Path,
    opts: &ApplyOptions,
    hook_script_path: &Path,
) -> Result<PatchPreview, IpcError> {
    let region = build_region(opts, hook_script_path).map_err(|e| IpcError::Internal {
        reason: format!("could not build cursor managed region: {e}"),
    })?;
    common::preview_with_region(&SPEC, home, &region)
}

/// Apply the patch. Idempotent across re-applies and across the two
/// cursor harness IDs — re-applying via `cursor_ide::apply` after
/// `cursor_cli::apply` (or vice versa) is a no-op since the managed
/// region's hash is identical for the same `(opts, hook_script_path)`.
pub fn apply(
    home: &Path,
    opts: &ApplyOptions,
    hook_script_path: &Path,
) -> Result<TrovePatch, IpcError> {
    let region = build_region(opts, hook_script_path).map_err(|e| IpcError::Internal {
        reason: format!("could not build cursor managed region: {e}"),
    })?;
    common::apply_with_region(&SPEC, home, &region)
}

/// Remove any Trove-managed region from `~/.cursor/hooks.json`. No-op
/// when the file is missing or contains no managed region. Note that a
/// single `revert` removes the block for both cursor harnesses
/// simultaneously — they share one region by design.
pub fn revert(home: &Path) -> Result<(), IpcError> {
    common::revert(&SPEC, home)
}

/// Build the [`ManagedRegion`] for the Cursor hooks.json patch.
/// Public so the harness-specific adapters and tests can assert on the
/// canonical hash directly without going through `apply`.
///
/// `hook_script_path` becomes part of the canonical region hash, so
/// re-applying with a different absolute path correctly invalidates
/// any prior block (Sprint 8's 3-way merge UI handles that conflict;
/// Sprint 7 reports it via `IpcError::RegionConflict`).
pub fn build_region(
    opts: &ApplyOptions,
    hook_script_path: &Path,
) -> Result<ManagedRegion, SentinelError> {
    // ApplyOptions has no Cursor-specific fields today: log_user_prompts
    // doesn't apply (Cursor hooks don't see prompt text directly), and
    // custom_attributes are the Collector's job once Sprint 8's resource
    // processor lands. Touching `_opts` keeps the signature future-proof
    // and silences the unused-arg lint cleanly.
    let _ = opts;

    let path_str = hook_script_path.to_string_lossy().into_owned();
    let entry = Value::Array(vec![Value::Object({
        let mut entry = Map::new();
        entry.insert("command".to_string(), Value::String(path_str));
        entry.insert("type".to_string(), Value::String("command".to_string()));
        entry
    })]);

    let mut hooks = Map::new();
    hooks.insert("beforeShellExecution".to_string(), entry.clone());
    hooks.insert("afterShellExecution".to_string(), entry);

    let mut top = Map::new();
    top.insert("version".to_string(), Value::Number(1.into()));
    top.insert("hooks".to_string(), Value::Object(hooks));

    ManagedRegion::for_json_patches(&top)
}

/// Placeholder used as the static SPEC's `build_region` slot. Never
/// called in normal execution because cursor adapters always go through
/// [`apply`] / [`preview`] (which call `*_with_region` directly). If
/// reached, returns a clear error so the bug surfaces immediately.
fn build_region_placeholder(_: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    Err(SentinelError::EmitFailed(
        "cursor adapter requires a runtime hook script path; call cursor_common::apply / preview \
         instead of common::apply / preview"
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    use crate::adapters::PreviewStatus;
    use crate::adapters::common::HarnessSpec;

    fn read_config(home: &Path) -> String {
        fs::read_to_string(config_path(home)).unwrap()
    }

    fn fake_hook_path() -> PathBuf {
        // Deterministic path so the canonical hash stays stable across
        // tests on the same machine. Anything that exists is fine; tests
        // never invoke the script, only embed its path in the region.
        PathBuf::from("/opt/trove/resources/hooks/cursor-otel-hook.cjs")
    }

    // --- 1. Fresh install -----------------------------------------------------

    #[test]
    fn fresh_install_creates_a_valid_hooks_file() {
        let home = tempdir().unwrap();
        let patch =
            apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        let written = read_config(home.path());
        let parsed: Value = serde_json::from_str(&written).unwrap();

        assert_eq!(parsed["version"], 1);
        let hooks = parsed.get("hooks").and_then(Value::as_object).unwrap();
        let before = hooks
            .get("beforeShellExecution")
            .and_then(Value::as_array)
            .unwrap();
        let after_arr = hooks
            .get("afterShellExecution")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(after_arr.len(), 1);
        assert_eq!(before[0]["type"], "command");
        assert_eq!(
            before[0]["command"],
            "/opt/trove/resources/hooks/cursor-otel-hook.cjs"
        );
        assert!(parsed.get("_trove").is_some());

        assert_eq!(patch.managed_block_hash.len(), 64);
        assert_eq!(patch.file_hash_at_last_write.len(), 64);
        assert_eq!(patch.format, Format::Json);
    }

    // --- 2. Idempotent re-apply ----------------------------------------------

    #[test]
    fn idempotent_reapply_does_not_change_the_file() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        let after_first = read_config(home.path());

        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        let after_second = read_config(home.path());
        assert_eq!(after_first, after_second);
    }

    // --- 3. User-edited outside the managed block ---------------------------

    #[test]
    fn user_keys_outside_block_survive_apply() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"unrelatedUserKey":"keepme"}"#).unwrap();

        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(parsed["unrelatedUserKey"], "keepme");
        assert_eq!(parsed["version"], 1);
        assert!(
            parsed["hooks"]["beforeShellExecution"]
                .as_array()
                .is_some()
        );
    }

    // --- 4. User-edited inside the managed block (conflict) ----------------

    #[test]
    fn editing_inside_the_managed_block_yields_conflict() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        // Tamper with the managed array — change the hook command path.
        let path = config_path(home.path());
        let written = read_config(home.path());
        let edited = written.replace(
            "/opt/trove/resources/hooks/cursor-otel-hook.cjs",
            "/opt/attacker/evil.js",
        );
        assert_ne!(edited, written);
        fs::write(&path, &edited).unwrap();

        let result = apply(home.path(), &ApplyOptions::default(), &fake_hook_path());
        match result {
            Err(IpcError::RegionConflict { path: p }) => {
                assert_eq!(p, path.display().to_string());
            }
            other => panic!("expected RegionConflict, got {other:?}"),
        }
        // The on-disk file is untouched.
        assert_eq!(read_config(home.path()), edited);
    }

    // --- 5. Malformed file ---------------------------------------------------

    #[test]
    fn malformed_file_is_unparseable_error() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not valid json").unwrap();

        let err = apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap_err();
        assert!(
            matches!(err, IpcError::ConfigUnparseable { .. }),
            "expected ConfigUnparseable, got {err:?}"
        );
        // File is unchanged.
        assert_eq!(read_config(home.path()), "{not valid json");
    }

    // --- 6. Missing parent dir ----------------------------------------------

    #[test]
    fn missing_parent_dir_is_created_automatically() {
        let home = tempdir().unwrap();
        assert!(!home.path().join(".cursor").exists());
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        assert!(home.path().join(".cursor").exists());
        assert!(config_path(home.path()).exists());
    }

    // --- 7. Read-only parent dir → IO error ---------------------------------

    #[cfg(unix)]
    #[test]
    fn readonly_parent_dir_yields_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempdir().unwrap();
        let parent = home.path().join(".cursor");
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();

        let err = apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap_err();
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
        let original = "{\n  \"unrelatedUserKey\": \"keepme\"\n}\n";
        fs::write(&path, original).unwrap();

        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
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
        let user_only = "{\"version\":1,\"hooks\":{}}";
        fs::write(&path, user_only).unwrap();
        revert(home.path()).unwrap();
        assert_eq!(read_config(home.path()), user_only);
    }

    // --- Preview --------------------------------------------------------------

    #[test]
    fn preview_on_missing_file_returns_fresh_status() {
        let home = tempdir().unwrap();
        let preview =
            preview(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Fresh);
        assert_eq!(preview.format, Format::Json);
        assert_eq!(preview.before, "");
        assert!(preview.after.contains("beforeShellExecution"));
        assert!(preview.after.contains("/opt/trove/resources/hooks/cursor-otel-hook.cjs"));
    }

    #[test]
    fn preview_after_apply_returns_idempotent_status() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        let preview =
            preview(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Idempotent);
        assert_eq!(preview.after, preview.before);
    }

    // --- Shared-region invariant: cursor_ide::apply then cursor_cli::apply --

    #[test]
    fn applying_with_same_hook_path_is_idempotent_across_callers() {
        // This test simulates the "both cursor harnesses share a region"
        // contract: a second apply with identical inputs must be a no-op.
        // The two harness-level adapters are thin wrappers over the same
        // build_region/apply code paths and produce the same hash.
        let home = tempdir().unwrap();
        let p = fake_hook_path();

        apply(home.path(), &ApplyOptions::default(), &p).unwrap();
        let after_first = read_config(home.path());

        apply(home.path(), &ApplyOptions::default(), &p).unwrap();
        let after_second = read_config(home.path());

        assert_eq!(after_first, after_second);
    }

    #[test]
    fn changing_hook_path_yields_conflict_until_reverted() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        let other_path = PathBuf::from("/different/path/cursor-otel-hook.cjs");
        let err = apply(home.path(), &ApplyOptions::default(), &other_path).unwrap_err();
        assert!(matches!(err, IpcError::RegionConflict { .. }));

        revert(home.path()).unwrap();
        apply(home.path(), &ApplyOptions::default(), &other_path).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(
            parsed["hooks"]["beforeShellExecution"][0]["command"],
            "/different/path/cursor-otel-hook.cjs"
        );
    }

    #[test]
    fn build_region_placeholder_errors_when_called() {
        // The SPEC's static fn-pointer should never be invoked in
        // production — but if a future refactor accidentally routes a
        // cursor SPEC through common::apply, we want a loud error.
        let opts = ApplyOptions::default();
        let result = (SPEC.build_region)(&opts);
        assert!(result.is_err(), "placeholder should always error");
    }

    #[test]
    fn spec_describes_the_cursor_hooks_file() {
        // Pin the SPEC's static fields. Any change to these is a wire-
        // format compatibility break for users with existing hooks.json.
        fn assert_spec(s: &HarnessSpec) {
            assert_eq!(s.config_dir, ".cursor");
            assert_eq!(s.config_file, "hooks.json");
            assert_eq!(s.format, Format::Json);
        }
        assert_spec(&SPEC);
    }
}
