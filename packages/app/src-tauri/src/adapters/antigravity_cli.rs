//! Antigravity CLI (`agy`) adapter — the successor to the discontinued
//! Gemini CLI.
//!
//! Google dropped the native OTLP exporter Gemini CLI had (upstream
//! request `google-antigravity/antigravity-cli#366`). Antigravity *did*
//! inherit Gemini CLI's **Hooks** mechanism, so Trove bridges it exactly
//! like the Cursor adapters: it installs a Trove-managed region into the
//! host hooks file (`~/.gemini/antigravity-cli/hooks.json`) whose entries
//! point at a bundled Node script (`resources/hooks/antigravity-otel-hook
//! .cjs`). On each agent event agy pipes the event JSON to the script on
//! stdin; the script turns it into OTLP metrics posted directly to the
//! local collector (`:4318`) with `harness.id=antigravity-cli` set inline
//! in the resource attributes — no native service.name, no collector
//! tierA/diag overlay, no supplemental watcher.
//!
//! Host-file format (confirmed against `agy` v1.0.x): a single top-level
//! object keyed by canonical event name, each value a `JSONHookSpec`
//! object `{ "type": "command", "command": "<abs path>" }`. (Gemini
//! CLI's older `{ "<event>": [ { matcher, hooks: [...] } ] }` array shape
//! is *not* what Antigravity parses.) Trove's `_trove` JSON sentinel is
//! added alongside the event keys; agy ignores unknown top-level keys.
//!
//! Like `cursor_common`, the managed region depends on the resolved
//! absolute path of the bundled hook script (only the Tauri layer can
//! resolve it via `resource_dir()`), so we expose a free
//! `build_region(opts, hook_path)` and call `common::apply_with_region` /
//! `common::preview_with_region` directly. The SPEC's `build_region`
//! field is a panic-loud placeholder.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::ipc::IpcError;
use crate::safety::sentinels::{Format, ManagedRegion, SentinelError};

use super::common::{self, HarnessSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

const SPEC: HarnessSpec = HarnessSpec {
    adapter_id: "antigravity-cli",
    // Antigravity keeps its config under `~/.gemini/antigravity-cli/`
    // (the `~/.gemini` parent is shared with the legacy Gemini CLI dir).
    config_dir: ".gemini/antigravity-cli",
    config_file: "hooks.json",
    format: Format::Json,
    build_region: build_region_placeholder,
};

/// Canonical agy hook events Trove registers. `UserPromptSubmit` /
/// `BeforeShellExecution` are turn-start markers (the bundled hook
/// stashes start time + sizes); the others carry the observations the
/// `antigravity_cli_defaults()` mapping rules key off. Every entry
/// points at the same bundled wrapper script, which branches on the
/// event name it receives on stdin.
const HOOK_EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "Stop",
    "BeforeShellExecution",
    "AfterShellExecution",
    "ErrorOccurred",
];

/// Resolve the absolute path of `~/.gemini/antigravity-cli/hooks.json`
/// under `home`. Pure helper so tests can scope to a `tempdir`.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    common::config_path(&SPEC, home)
}

/// Compute the diff Trove would write into the Antigravity hooks file.
/// `hook_script_path` is the absolute path of the bundled
/// `antigravity-otel-hook.cjs`, resolved by the IPC layer at runtime.
pub fn preview(
    home: &Path,
    opts: &ApplyOptions,
    hook_script_path: &Path,
) -> Result<PatchPreview, IpcError> {
    let region = build_region(opts, hook_script_path).map_err(|e| IpcError::Internal {
        reason: format!("could not build antigravity managed region: {e}"),
    })?;
    common::preview_with_region(&SPEC, home, &region)
}

/// Apply the patch. Idempotent across re-applies for the same
/// `(opts, hook_script_path)` — the managed region's hash is stable.
pub fn apply(
    home: &Path,
    opts: &ApplyOptions,
    hook_script_path: &Path,
) -> Result<TrovePatch, IpcError> {
    let region = build_region(opts, hook_script_path).map_err(|e| IpcError::Internal {
        reason: format!("could not build antigravity managed region: {e}"),
    })?;
    common::apply_with_region(&SPEC, home, &region)
}

/// Remove any Trove-managed region from the Antigravity hooks file.
/// No-op when the file is missing or carries no managed region.
pub fn revert(home: &Path) -> Result<(), IpcError> {
    common::revert(&SPEC, home)
}

/// Regenerate the JSON sidecar the bundled Antigravity hook script reads
/// at startup to discover the user's current rules. Mirrors
/// [`crate::adapters::cursor_common::regenerate_hooks_for_rules`]; called
/// from `apply_mappings` and on relaunch so edits take effect without a
/// re-apply.
///
/// Path: `~/.gemini/antigravity-cli/trove-hook-rules.json`. Best-effort —
/// failure must not block the `apply_mappings` IPC.
pub fn regenerate_hooks_for_rules(
    app: &tauri::AppHandle,
    mapping_state: &crate::mappings::MappingState,
) -> Result<(), std::io::Error> {
    use tauri::Manager as _;
    let Ok(home) = app.path().home_dir() else {
        return Ok(());
    };
    let dir = home.join(".gemini").join("antigravity-cli");
    if !dir.exists() {
        return Ok(());
    }
    let target = dir.join("trove-hook-rules.json");
    let snapshot = crate::adapters::cursor_hook_codegen::serialize_for_hook_ids(
        mapping_state,
        &[crate::harness::HarnessId::AntigravityCli],
    );
    let body = serde_json::to_vec_pretty(&snapshot).map_err(std::io::Error::other)?;
    crate::safety::atomic::write_atomic(&target, &body)
}

/// Build the [`ManagedRegion`] for the Antigravity hooks.json patch.
/// Public so tests can assert on the canonical hash directly.
///
/// `hook_script_path` becomes part of the canonical region hash, so
/// re-applying with a different absolute path invalidates the prior
/// block (reported via `IpcError::RegionConflict`).
pub fn build_region(
    opts: &ApplyOptions,
    hook_script_path: &Path,
) -> Result<ManagedRegion, SentinelError> {
    // ApplyOptions carries no Antigravity-specific fields; custom
    // attributes are the collector's job. Touch it to stay future-proof.
    let _ = opts;

    let path_str = hook_script_path.to_string_lossy().into_owned();

    // Each event maps to a single JSONHookSpec object (NOT an array).
    let mut top = Map::new();
    for event in HOOK_EVENTS {
        let mut spec = Map::new();
        spec.insert("type".to_string(), Value::String("command".to_string()));
        spec.insert("command".to_string(), Value::String(path_str.clone()));
        top.insert((*event).to_string(), Value::Object(spec));
    }

    ManagedRegion::for_json_patches(&top)
}

/// Placeholder for the static SPEC's `build_region` slot. Never called
/// in normal execution because this adapter always goes through
/// [`apply`] / [`preview`] (which call `*_with_region` directly).
fn build_region_placeholder(_: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    Err(SentinelError::EmitFailed(
        "antigravity adapter requires a runtime hook script path; call antigravity_cli::apply / \
         preview instead of common::apply / preview"
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    use crate::adapters::PreviewStatus;

    fn fake_hook_path() -> PathBuf {
        PathBuf::from("/opt/trove/resources/hooks/antigravity-otel-hook.cjs")
    }

    fn read_config(home: &Path) -> String {
        fs::read_to_string(config_path(home)).unwrap()
    }

    #[test]
    fn config_path_resolves_under_antigravity_cli_dir() {
        let home = PathBuf::from("/home/dev");
        assert_eq!(
            config_path(&home),
            PathBuf::from("/home/dev/.gemini/antigravity-cli/hooks.json")
        );
    }

    #[test]
    fn apply_writes_event_objects_pointing_at_the_hook() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        // agy's shape: each event value is an object (not an array).
        assert_eq!(parsed["Stop"]["type"], "command");
        assert_eq!(
            parsed["Stop"]["command"],
            "/opt/trove/resources/hooks/antigravity-otel-hook.cjs"
        );
        assert_eq!(parsed["UserPromptSubmit"]["type"], "command");
        assert!(parsed["AfterShellExecution"].is_object());
        assert!(parsed.get("_trove").is_some());
    }

    #[test]
    fn idempotent_reapply_does_not_change_the_file() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        let first = read_config(home.path());
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        assert_eq!(first, read_config(home.path()));
    }

    #[test]
    fn user_keys_outside_block_survive_apply() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"PreToolUse":{"type":"command","command":"/x/user"}}"#).unwrap();

        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        // The user's own hook for an event Trove doesn't manage survives.
        assert_eq!(parsed["PreToolUse"]["command"], "/x/user");
        assert!(parsed.get("_trove").is_some());
    }

    #[test]
    fn revert_removes_the_managed_block() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        revert(home.path()).unwrap();

        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert!(parsed.get("_trove").is_none());
        assert!(parsed.get("Stop").is_none());
    }

    #[test]
    fn preview_after_apply_is_idempotent() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        let preview = preview(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Idempotent);
    }

    #[test]
    fn changing_hook_path_yields_conflict_until_reverted() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        let other = PathBuf::from("/different/antigravity-otel-hook.cjs");
        let err = apply(home.path(), &ApplyOptions::default(), &other).unwrap_err();
        assert!(matches!(err, IpcError::RegionConflict { .. }));

        revert(home.path()).unwrap();
        apply(home.path(), &ApplyOptions::default(), &other).unwrap();
        let parsed: Value = serde_json::from_str(&read_config(home.path())).unwrap();
        assert_eq!(
            parsed["Stop"]["command"],
            "/different/antigravity-otel-hook.cjs"
        );
    }

    #[test]
    fn build_region_placeholder_errors_when_called() {
        let opts = ApplyOptions::default();
        assert!((SPEC.build_region)(&opts).is_err());
    }
}
