//! Tauri `#[command]` functions exposed to the React UI.
//!
//! Sprint 3 PR 1 shipped detection; PR 2 added the patch trio
//! (preview / apply / revert). Sprint 5 PR 1 layers on app-state
//! persistence: `apply_patch` / `revert_patch` now upsert and remove
//! `HarnessConfig` entries in `state.json`, and three new commands
//! (`get_app_state`, `save_backend`, `clear_backend`) drive the
//! backend wizard.
//!
//! The collector reload that follows `save_backend` lands in PR 2 of
//! this sprint — for now we leave a tracing breadcrumb where the
//! reload will hook in.

use std::path::{Path, PathBuf};

use std::collections::HashMap;

use crate::adapters::{
    ApplyOptions, PatchPreview, TrovePatch, claude_code, codex_cli, cursor_cli, cursor_ide,
    gemini_cli, opencode, qwen_code,
};
use crate::app_state::{
    self, AppState, Backend, BackendDraft, backend_secret_accounts, drain_secrets_from_draft,
    harness_config_from_apply,
};
use crate::collector::codegen;
use crate::detect::{DetectedHarness, detect_all};
use crate::harness::HarnessId;
use crate::secrets;

use super::IpcError;
use super::test_export::{DEFAULT_TEST_BUDGET, TestExportResult, test_export_at};

/// Detect every Tier 1 harness on the user's machine. Always succeeds —
/// missing harnesses come back with `detected: false` rather than as
/// errors. Future expansion (Tier 2 / Tier 3) only changes the row
/// count, not the error shape.
#[tauri::command]
pub fn list_detected_harnesses() -> Result<Vec<DetectedHarness>, IpcError> {
    Ok(detect_all())
}

/// Compute the diff Trove would write for `harness_id` with `options`.
/// The UI renders the unified diff client-side from `before`/`after`.
//
// Tauri requires owned argument types for JSON deserialization, even
// when the function body only reads them — silence the lint locally.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn preview_patch(
    app: tauri::AppHandle,
    harness_id: HarnessId,
    options: ApplyOptions,
) -> Result<PatchPreview, IpcError> {
    let home = home_dir()?;
    preview_patch_inner(harness_id, &options, &home, || cursor_hook_script_path(&app))
}

/// Free-function inner for [`preview_patch`] so unit tests can exercise
/// the dispatch without synthesising a Tauri `AppHandle`. `hook_resolver`
/// is invoked only on the Cursor arms; Tier 1 / fallback arms never call
/// it (tests can pass a closure that panics).
fn preview_patch_inner<F>(
    harness_id: HarnessId,
    options: &ApplyOptions,
    home: &Path,
    hook_resolver: F,
) -> Result<PatchPreview, IpcError>
where
    F: FnOnce() -> Result<PathBuf, IpcError>,
{
    match harness_id {
        HarnessId::ClaudeCode => claude_code::preview(home, options),
        HarnessId::CodexCli => codex_cli::preview(home, options),
        HarnessId::GeminiCli => gemini_cli::preview(home, options),
        HarnessId::QwenCode => qwen_code::preview(home, options),
        HarnessId::CursorIde => cursor_ide::preview(home, options, &hook_resolver()?),
        HarnessId::CursorCli => cursor_cli::preview(home, options, &hook_resolver()?),
        HarnessId::Opencode => opencode::preview(home, options),
        // Tier 3 (Sprint 9) lands later.
        _ => Err(IpcError::HarnessNotImplemented { id: harness_id }),
    }
}

/// Apply Trove's patch to `harness_id`'s host config. On success, upsert
/// a [`HarnessConfig`] entry into `state.json` so Sprint 8's three-way
/// conflict UI has the metadata it needs (managed-block hash, post-write
/// file hash, options snapshot, last-patched timestamp).
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn apply_patch(
    app: tauri::AppHandle,
    harness_id: HarnessId,
    options: ApplyOptions,
) -> Result<TrovePatch, IpcError> {
    let home = home_dir()?;
    let patch = match harness_id {
        HarnessId::ClaudeCode => claude_code::apply(&home, &options),
        HarnessId::CodexCli => codex_cli::apply(&home, &options),
        HarnessId::GeminiCli => gemini_cli::apply(&home, &options),
        HarnessId::QwenCode => qwen_code::apply(&home, &options),
        HarnessId::CursorIde => {
            let hook = cursor_hook_script_path(&app)?;
            cursor_ide::apply(&home, &options, &hook)
        }
        HarnessId::CursorCli => {
            let hook = cursor_hook_script_path(&app)?;
            cursor_cli::apply(&home, &options, &hook)
        }
        HarnessId::Opencode => opencode::apply(&home, &options),
        _ => Err(IpcError::HarnessNotImplemented { id: harness_id }),
    }?;

    let harness_config = harness_config_from_apply(
        harness_id,
        &harness_config_path(harness_id, &home),
        options,
        patch.clone(),
    );

    app_state::upsert_harness(&app, harness_config)?;

    Ok(patch)
}

/// Remove Trove's patch from `harness_id`'s host config and drop the
/// matching entry from `state.json`. Permissive — any managed region
/// present in the host file is removed even when state.json has no
/// record of the harness, so a fresh reinstall can still unwire a
/// previous machine's patch.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn revert_patch(app: tauri::AppHandle, harness_id: HarnessId) -> Result<(), IpcError> {
    let home = home_dir()?;
    match harness_id {
        HarnessId::ClaudeCode => claude_code::revert(&home),
        HarnessId::CodexCli => codex_cli::revert(&home),
        HarnessId::GeminiCli => gemini_cli::revert(&home),
        HarnessId::QwenCode => qwen_code::revert(&home),
        // Both Cursor harnesses share `~/.cursor/hooks.json`; either
        // adapter's revert removes the entire managed block. We keep
        // the dispatch separate (rather than collapsing onto a shared
        // arm) so the per-harness state.json upsert/remove call below
        // still receives the correct id.
        HarnessId::CursorIde => cursor_ide::revert(&home),
        HarnessId::CursorCli => cursor_cli::revert(&home),
        HarnessId::Opencode => opencode::revert(&home),
        _ => Err(IpcError::HarnessNotImplemented { id: harness_id }),
    }?;

    app_state::remove_harness(&app, harness_id)?;

    Ok(())
}

/// Return the persisted [`AppState`]. A fresh launch (no `state.json`
/// yet) gets [`AppState::default`]: schema v2, null backend, empty
/// harness list.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn get_app_state(app: tauri::AppHandle) -> Result<AppState, IpcError> {
    Ok(app_state::load(&app)?)
}

/// Persist a new backend chosen by the wizard. Stores each secret in
/// the OS keychain, replaces the raw values in `draft` with [`SecretRef`]
/// handles, writes the resulting [`Backend`] to `state.json`, then
/// regenerates `collector.yaml` and recycles the supervised sidecar so
/// telemetry begins flowing to the new destination.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn save_backend(app: tauri::AppHandle, draft: BackendDraft) -> Result<Backend, IpcError> {
    let (backend, secrets_to_store) = drain_secrets_from_draft(draft);

    for secret in &secrets_to_store {
        secrets::store(&secret.account, &secret.value)?;
    }

    let mut state = app_state::load(&app)?;
    state.backend = Some(backend.clone());
    app_state::save(&app, &state)?;

    let rendered = codegen::render(&backend).map_err(render_error_to_ipc)?;
    let env = unwrap_env(rendered.env);
    crate::reload_collector(&app, &rendered.yaml, env).map_err(|e| boot_error_to_ipc(&e))?;

    Ok(backend)
}

/// Send a synthetic OTLP/HTTP traces payload through the local
/// collector and return the wizard's "Test export" result. See
/// [`super::test_export`] for the underlying logic.
///
/// The endpoint is hard-coded to `127.0.0.1:4318/v1/traces` — the
/// loopback address the supervised collector binds, identical across
/// every backend. Whether the export succeeds end-to-end depends on
/// the user's saved backend (which the collector forwards to).
#[tauri::command]
pub async fn test_export(app: tauri::AppHandle) -> Result<TestExportResult, IpcError> {
    let log_path = crate::collector_log_path(&app).map_err(|e| boot_error_to_ipc(&e))?;
    let result = test_export_at(
        "http://127.0.0.1:4318/v1/traces",
        &log_path,
        DEFAULT_TEST_BUDGET,
    )
    .await;
    Ok(result)
}

/// Wipe the active backend: delete every keychain entry it referenced,
/// null out `state.backend`, restore the smoke `collector.yaml`, and
/// recycle the supervisor with no env vars set. Idempotent — a no-op
/// when no backend is currently saved.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn clear_backend(app: tauri::AppHandle) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    if let Some(backend) = state.backend.take() {
        for account in backend_secret_accounts(&backend) {
            secrets::delete(&account)?;
        }
        app_state::save(&app, &state)?;

        crate::reload_collector(&app, crate::smoke_config_yaml(), HashMap::new())
            .map_err(|e| boot_error_to_ipc(&e))?;
    }
    Ok(())
}

/// Convert codegen's secrecy-wrapped env map into the plain map the
/// supervisor's `Command::envs` consumes. Each [`Zeroizing`] string
/// drops as the iterator advances; a momentary plain-`String` copy
/// lives in the new `HashMap` until the supervisor passes it to the
/// child. We accept that trade-off — `OsString` (which is what
/// `Command::env` actually stores) doesn't impl `Zeroize`.
fn unwrap_env(env: HashMap<String, zeroize::Zeroizing<String>>) -> HashMap<String, String> {
    env.into_iter().map(|(k, v)| (k, (*v).clone())).collect()
}

fn render_error_to_ipc(err: codegen::RenderError) -> IpcError {
    match err {
        codegen::RenderError::Keychain { account, source } => IpcError::Internal {
            reason: format!("codegen could not read keychain entry {account}: {source}"),
        },
    }
}

fn boot_error_to_ipc(err: &crate::CollectorBootError) -> IpcError {
    IpcError::Internal {
        reason: format!("collector reload failed: {err}"),
    }
}

fn home_dir() -> Result<PathBuf, IpcError> {
    dirs::home_dir().ok_or(IpcError::Internal {
        reason: "could not resolve user home directory".into(),
    })
}

/// Resolve the absolute path of the harness's host config file under
/// `home`. Used to populate [`HarnessConfig.config_path`] after a
/// successful apply.
fn harness_config_path(id: HarnessId, home: &Path) -> PathBuf {
    match id {
        HarnessId::ClaudeCode => claude_code::config_path(home),
        HarnessId::CodexCli => codex_cli::config_path(home),
        HarnessId::GeminiCli => gemini_cli::config_path(home),
        HarnessId::QwenCode => qwen_code::config_path(home),
        // Both Cursor harnesses share `~/.cursor/hooks.json`. The
        // state.json entry records the same path under either id; the
        // shared underlying file is the source of truth.
        HarnessId::CursorIde => cursor_ide::config_path(home),
        HarnessId::CursorCli => cursor_cli::config_path(home),
        HarnessId::Opencode => opencode::config_path(home),
        // Tier 3 doesn't reach this code path because their adapters
        // error out before we try to record state.
        _ => PathBuf::new(),
    }
}

/// Resolve the absolute path of the bundled `cursor-otel-hook.cjs`
/// script. Tauri stages the file under the app's `resource_dir()` at
/// install time via the `bundle.resources` entry in `tauri.conf.json`;
/// in development, it resolves to the equivalent path under the
/// `target/<profile>` build output.
fn cursor_hook_script_path(app: &tauri::AppHandle) -> Result<PathBuf, IpcError> {
    use tauri::Manager as _;
    let resource_dir = app.path().resource_dir().map_err(|e| IpcError::Internal {
        reason: format!("could not resolve Tauri resource_dir for cursor hook: {e}"),
    })?;
    Ok(resource_dir
        .join("resources")
        .join("hooks")
        .join("cursor-otel-hook.cjs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_detected_harnesses_returns_a_row_per_supported_harness() {
        // Calls into the real environment — every supported harness
        // returns a row even when nothing is installed (`detected:
        // false`). The detect module's hermetic tests already cover
        // scoping; this test asserts the IPC entry point doesn't drop
        // or reorder rows.
        let result = list_detected_harnesses().unwrap();
        let expected = HarnessId::tier_1().len() + HarnessId::tier_2().len();
        assert_eq!(result.len(), expected);
    }

    #[test]
    fn preview_patch_inner_for_unimplemented_harness_returns_not_implemented() {
        // Cline is Tier 3 (Sprint 9), still unimplemented at this point.
        // The hook resolver should never be called for non-cursor harnesses.
        let home = std::path::PathBuf::from("/tmp/should-not-be-touched");
        let err = preview_patch_inner(
            HarnessId::Cline,
            &ApplyOptions::default(),
            &home,
            || panic!("hook resolver must not be invoked for Tier 3 harnesses"),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            IpcError::HarnessNotImplemented {
                id: HarnessId::Cline
            }
        ));
    }

    #[test]
    fn preview_patch_inner_routes_opencode_through_adapter_without_resolver() {
        // OpenCode is now wired in Sprint 7 PR 2 with a standard SPEC; the
        // hook resolver must not be invoked for it.
        let home = tempfile::tempdir().unwrap();
        let result = preview_patch_inner(
            HarnessId::Opencode,
            &ApplyOptions::default(),
            home.path(),
            || panic!("hook resolver must not be invoked for opencode"),
        )
        .unwrap();
        assert!(
            result.after.contains("@devtheops/opencode-plugin-otel"),
            "preview should embed the OTel plugin package name"
        );
    }

    #[test]
    fn preview_patch_inner_routes_cursor_ide_through_resolver_and_adapter() {
        let home = tempfile::tempdir().unwrap();
        let fake_hook = std::path::PathBuf::from("/opt/trove/cursor-otel-hook.cjs");
        let resolver_called = std::cell::Cell::new(false);
        let result = preview_patch_inner(
            HarnessId::CursorIde,
            &ApplyOptions::default(),
            home.path(),
            || {
                resolver_called.set(true);
                Ok(fake_hook.clone())
            },
        )
        .unwrap();
        assert!(resolver_called.get(), "resolver must be called for cursor-ide");
        assert!(result.after.contains("/opt/trove/cursor-otel-hook.cjs"));
    }

    #[test]
    fn preview_patch_inner_routes_cursor_cli_through_resolver_and_adapter() {
        let home = tempfile::tempdir().unwrap();
        let fake_hook = std::path::PathBuf::from("/opt/trove/cursor-otel-hook.cjs");
        let result = preview_patch_inner(
            HarnessId::CursorCli,
            &ApplyOptions::default(),
            home.path(),
            || Ok(fake_hook.clone()),
        )
        .unwrap();
        assert!(result.after.contains("/opt/trove/cursor-otel-hook.cjs"));
    }

    #[test]
    fn preview_patch_inner_propagates_hook_resolver_errors() {
        let home = std::path::PathBuf::from("/tmp/should-not-be-touched");
        let err = preview_patch_inner(
            HarnessId::CursorIde,
            &ApplyOptions::default(),
            &home,
            || {
                Err(IpcError::Internal {
                    reason: "could not resolve resource_dir".to_string(),
                })
            },
        )
        .unwrap_err();
        assert!(matches!(err, IpcError::Internal { .. }));
    }

    // apply_patch / revert_patch / save_backend / clear_backend / get_app_state
    // require a Tauri AppHandle and are exercised by the integration tests
    // under `tests/app_state_*.rs` against a real temp HOME and config dir.
}
