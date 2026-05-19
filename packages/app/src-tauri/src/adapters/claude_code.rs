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

use crate::ipc::IpcError;
use crate::safety::sentinels::{Format, ManagedRegion, SentinelError};

use super::common::{self, HarnessSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

/// The base set of `OTel` env vars Trove writes into `env`. Custom
/// attributes are appended at apply time based on [`ApplyOptions`].
const MANAGED_ENV_KEYS: &[(&str, &str)] = &[
    ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
    ("OTEL_METRICS_EXPORTER", "otlp"),
    ("OTEL_LOGS_EXPORTER", "otlp"),
    ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
    ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:4318"),
    ("OTEL_METRIC_EXPORT_INTERVAL", "60000"),
    ("OTEL_LOGS_EXPORT_INTERVAL", "5000"),
];

const SPEC: HarnessSpec = HarnessSpec {
    adapter_id: "claude-code",
    config_dir: ".claude",
    config_file: "settings.json",
    format: Format::Json,
    build_region,
};

/// Resolve the absolute path of the Claude Code settings file under
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

/// Remove any Trove-managed region from the host file.
pub fn revert(home: &Path) -> Result<(), IpcError> {
    common::revert(&SPEC, home)
}

/// Build the [`ManagedRegion`] for a JSON merge of the env block.
fn build_region(opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    let mut env = serde_json::Map::new();
    for (k, v) in MANAGED_ENV_KEYS {
        env.insert((*k).to_string(), Value::String((*v).to_string()));
    }
    // Always tag emissions with a stable harness identifier so backends
    // can filter Claude Code's signals out of a mixed-harness pipeline.
    // The pair is intentionally pinned in code (not user-configurable);
    // any per-install custom attributes append after them.
    let mut attr_pairs: Vec<(String, String)> = vec![
        ("harness.id".to_string(), "claude-code".to_string()),
        ("harness.name".to_string(), "Claude Code".to_string()),
    ];
    // BTreeMap iteration is already sorted, so the hash is deterministic.
    for (k, v) in &opts.custom_attributes {
        attr_pairs.push((k.clone(), v.clone()));
    }
    let attrs = attr_pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    env.insert(
        "OTEL_RESOURCE_ATTRIBUTES".to_string(),
        Value::String(attrs),
    );

    let mut top = serde_json::Map::new();
    top.insert("env".to_string(), Value::Object(env));
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

    // --- 1. Fresh install -----------------------------------------------------

    #[test]
    fn fresh_install_creates_a_valid_settings_file() {
        let home = tempdir().unwrap();
        let patch = apply(home.path(), &ApplyOptions::default()).unwrap();

        let written = read_config(home.path());
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
        let expected = common::hash_hex(after_second.as_bytes());
        assert_eq!(second.file_hash_at_last_write, expected);
    }

    // --- 3. User-edited outside the managed block ---------------------------

    #[test]
    fn user_keys_outside_block_survive_apply() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
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
        assert_eq!(env["OTEL_EXPORTER_OTLP_ENDPOINT"], "http://127.0.0.1:4318");
    }

    // --- 4. User-edited inside the managed block (conflict) ----------------

    #[test]
    fn editing_inside_the_managed_block_yields_conflict() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

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
        assert_eq!(read_config(home.path()), "{not valid json");
    }

    // --- 6. Missing parent dir ----------------------------------------------

    #[test]
    fn missing_parent_dir_is_created_automatically() {
        let home = tempdir().unwrap();
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
        let parent = home.path().join(".claude");
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
        let original = "{\n  \"theme\": \"dark\",\n  \"env\": {\n    \"MY\": \"keepme\"\n  }\n}\n";
        fs::write(&path, original).unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();
        revert(home.path()).unwrap();

        let after = read_config(home.path());
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

    // --- Custom attributes --------------------------------------------------

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
        assert_eq!(
            attrs,
            "harness.id=claude-code,harness.name=Claude Code,env=prod,team=platform"
        );
    }

    #[test]
    fn harness_id_is_always_emitted_even_without_custom_attributes() {
        // Telemetry attribution: every Claude Code apply must include
        // the harness.id/harness.name pair so SigNoz can filter Claude
        // Code activity out of a mixed-harness pipeline.
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        let env = parsed.get("env").and_then(Value::as_object).unwrap();
        let attrs = env["OTEL_RESOURCE_ATTRIBUTES"].as_str().unwrap();
        assert_eq!(attrs, "harness.id=claude-code,harness.name=Claude Code");
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

        revert(home.path()).unwrap();
        apply(home.path(), &opts2).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(
            parsed["env"]["OTEL_RESOURCE_ATTRIBUTES"],
            "harness.id=claude-code,harness.name=Claude Code,team=platform"
        );
    }
}
