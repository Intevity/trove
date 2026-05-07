//! Tauri `#[command]` functions exposed to the React UI. Sprint 3 PR 1
//! shipped detection; PR 2 adds the patch trio (preview / apply / revert)
//! and dispatches by `HarnessId` into the per-adapter free functions.

use std::path::PathBuf;

use crate::adapters::{ApplyOptions, PatchPreview, TrovePatch, claude_code};
use crate::detect::{DetectedHarness, detect_all};
use crate::harness::HarnessId;

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
        // PR 3 swaps GeminiCli's arm to gemini_cli::preview.
        // codex-cli + qwen-code adapters land in Sprint 4.
        // Tier 2 / Tier 3 land in later sprints.
        _ => Err(IpcError::HarnessNotImplemented { id: harness_id }),
    }
}

/// Apply Trove's patch to `harness_id`'s host config. Returns the
/// `TrovePatch` metadata callers (Sprint 5+) can persist into
/// `state.json` for later three-way conflict detection.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn apply_patch(
    harness_id: HarnessId,
    options: ApplyOptions,
) -> Result<TrovePatch, IpcError> {
    let home = home_dir()?;
    match harness_id {
        HarnessId::ClaudeCode => claude_code::apply(&home, &options),
        _ => Err(IpcError::HarnessNotImplemented { id: harness_id }),
    }
}

/// Remove Trove's patch from `harness_id`'s host config. Permissive —
/// any managed region present is removed even without stored metadata,
/// so a fresh reinstall can still unwire a previous machine's patch.
#[tauri::command]
pub fn revert_patch(harness_id: HarnessId) -> Result<(), IpcError> {
    let home = home_dir()?;
    match harness_id {
        HarnessId::ClaudeCode => claude_code::revert(&home),
        _ => Err(IpcError::HarnessNotImplemented { id: harness_id }),
    }
}

fn home_dir() -> Result<PathBuf, IpcError> {
    dirs::home_dir().ok_or(IpcError::Internal {
        reason: "could not resolve user home directory".into(),
    })
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
        let err = preview_patch(HarnessId::CodexCli, ApplyOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            IpcError::HarnessNotImplemented {
                id: HarnessId::CodexCli
            }
        ));
    }

    #[test]
    fn apply_patch_for_unimplemented_harness_returns_not_implemented() {
        let err = apply_patch(HarnessId::QwenCode, ApplyOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            IpcError::HarnessNotImplemented {
                id: HarnessId::QwenCode
            }
        ));
    }

    #[test]
    fn revert_patch_for_unimplemented_harness_returns_not_implemented() {
        let err = revert_patch(HarnessId::Opencode).unwrap_err();
        assert!(matches!(
            err,
            IpcError::HarnessNotImplemented {
                id: HarnessId::Opencode
            }
        ));
    }

    #[test]
    fn gemini_cli_dispatch_is_not_implemented_in_pr_2() {
        // PR 3 will replace this with a real adapter call; this test
        // documents the current contract so PR 3 has to update it.
        let err = preview_patch(HarnessId::GeminiCli, ApplyOptions::default()).unwrap_err();
        assert!(matches!(
            err,
            IpcError::HarnessNotImplemented {
                id: HarnessId::GeminiCli
            }
        ));
    }
}
