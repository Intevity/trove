//! Qwen Code adapter — patches `~/.qwen/settings.json`'s top-level
//! `telemetry` object. Qwen Code is a Gemini CLI fork; the settings
//! schema for telemetry is byte-for-byte the same shape, so this
//! adapter mirrors `gemini_cli` with the namespace swapped.
//!
//! Verify the field set against the upstream qwen-code docs at every
//! adapter rev — the schema may diverge from Gemini's over time.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ipc::IpcError;
use crate::safety::sentinels::{Format, ManagedRegion, SentinelError};

use super::common::{self, HarnessSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

const SPEC: HarnessSpec = HarnessSpec {
    config_dir: ".qwen",
    config_file: "settings.json",
    format: Format::Json,
    build_region,
};

/// Resolve the absolute path of the Qwen Code settings file under
/// `home`. Pure helper so tests can scope to a `tempdir`.
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

/// Permissive revert — removes any Trove-managed region present.
pub fn revert(home: &Path) -> Result<(), IpcError> {
    common::revert(&SPEC, home)
}

/// Build the [`ManagedRegion`] for a JSON merge of the `telemetry`
/// block. Mirrors `gemini_cli::build_region` because Qwen Code is a
/// Gemini CLI fork and the upstream schema is byte-identical for this
/// block. `customAttributes` is a no-op for Qwen for the same reason
/// it's a no-op for Gemini.
///
/// `logPrompts` is pinned to `false`: Trove's pipeline is metrics-only
/// by policy, so we never opt into upstream prompt-body capture.
fn build_region(_opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    let mut telemetry = serde_json::Map::new();
    telemetry.insert("enabled".to_string(), Value::Bool(true));
    telemetry.insert("target".to_string(), Value::String("local".to_string()));
    // useCollector + otlpProtocol mirror gemini_cli: required as of
    // upstream's 0.4x telemetry refactor for any signal to actually
    // leave the harness process for the local collector.
    telemetry.insert("useCollector".to_string(), Value::Bool(true));
    telemetry.insert(
        "otlpProtocol".to_string(),
        Value::String("http".to_string()),
    );
    telemetry.insert(
        "otlpEndpoint".to_string(),
        Value::String("http://127.0.0.1:4318".to_string()),
    );
    telemetry.insert("logPrompts".to_string(), Value::Bool(false));

    let mut top = serde_json::Map::new();
    top.insert("telemetry".to_string(), Value::Object(telemetry));
    ManagedRegion::for_json_patches(&top)
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

    // --- Metrics-only policy -----------------------------------------------

    #[test]
    fn log_prompts_is_always_false() {
        // Trove's pipeline is metrics-only; never opt into upstream
        // prompt-body capture even if the user toggles something later.
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(parsed["telemetry"]["logPrompts"], false);
    }

    #[test]
    fn preview_after_apply_returns_idempotent_status() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Idempotent);
    }
}
