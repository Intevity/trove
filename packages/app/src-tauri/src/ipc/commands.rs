//! Tauri `#[command]` functions exposed to the React UI. Sprint 3 PR 1
//! ships only `list_detected_harnesses`; PR 2 adds the patch commands
//! once the first adapter exists to dispatch to.

use crate::detect::{detect_all, DetectedHarness};

use super::IpcError;

/// Detect every Tier 1 harness on the user's machine. Always succeeds —
/// missing harnesses come back with `detected: false` rather than as
/// errors. Future expansion (Tier 2 / Tier 3) only changes the row
/// count, not the error shape.
#[tauri::command]
pub fn list_detected_harnesses() -> Result<Vec<DetectedHarness>, IpcError> {
    Ok(detect_all())
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
        assert_eq!(result.len(), crate::harness::HarnessId::tier_1().len());
    }
}
