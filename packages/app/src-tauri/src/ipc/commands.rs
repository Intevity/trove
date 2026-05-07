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

use crate::adapters::{
    ApplyOptions, PatchPreview, TrovePatch, claude_code, codex_cli, gemini_cli, qwen_code,
};
use crate::app_state::{
    self, AppState, Backend, BackendDraft, backend_secret_accounts, drain_secrets_from_draft,
    harness_config_from_apply,
};
use crate::detect::{DetectedHarness, detect_all};
use crate::harness::HarnessId;
use crate::secrets;

use super::IpcError;

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
    harness_id: HarnessId,
    options: ApplyOptions,
) -> Result<PatchPreview, IpcError> {
    let home = home_dir()?;
    match harness_id {
        HarnessId::ClaudeCode => claude_code::preview(&home, &options),
        HarnessId::CodexCli => codex_cli::preview(&home, &options),
        HarnessId::GeminiCli => gemini_cli::preview(&home, &options),
        HarnessId::QwenCode => qwen_code::preview(&home, &options),
        // Tier 2 / Tier 3 land in later sprints.
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
/// handles, and writes the resulting [`Backend`] to `state.json`.
///
/// PR 2 of this sprint hooks the collector restart in here — once the
/// state is saved, we'll regenerate `collector.yaml` from the new
/// preset and recycle the supervised sidecar.
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

    // PR 2: regenerate collector.yaml + restart the supervisor here.
    tracing::info!(
        kind = ?std::mem::discriminant(&backend),
        "backend saved; collector reload pending PR 2",
    );

    Ok(backend)
}

/// Wipe the active backend: delete every keychain entry it referenced
/// and null out `state.backend`. Idempotent — a no-op when no backend
/// is currently saved.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn clear_backend(app: tauri::AppHandle) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    if let Some(backend) = state.backend.take() {
        for account in backend_secret_accounts(&backend) {
            secrets::delete(&account)?;
        }
        app_state::save(&app, &state)?;
    }
    Ok(())
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
        // Tier 2 / Tier 3 don't reach this code path because their
        // adapters error out before we try to record state.
        _ => PathBuf::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_detected_harnesses_returns_a_row_per_tier_1_harness() {
        // Calls into the real environment — every Tier 1 harness still
        // returns a row even when nothing is installed (`detected: false`).
        // The detect module's hermetic tests already cover scoping; this
        // test asserts the IPC entry point doesn't drop or reorder rows.
        let result = list_detected_harnesses().unwrap();
        assert_eq!(result.len(), HarnessId::tier_1().len());
    }

    #[test]
    fn preview_patch_for_unimplemented_harness_returns_not_implemented() {
        // Cline is Tier 3 (Sprint 9), still unimplemented at this point.
        let err = preview_patch(HarnessId::Cline, ApplyOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            IpcError::HarnessNotImplemented {
                id: HarnessId::Cline
            }
        ));
    }

    // apply_patch / revert_patch / save_backend / clear_backend / get_app_state
    // require a Tauri AppHandle and are exercised by the integration tests
    // under `tests/app_state_*.rs` against a real temp HOME and config dir.
}
