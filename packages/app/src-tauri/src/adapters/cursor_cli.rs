//! Cursor CLI adapter — same hook surface as `cursor_ide`. Both
//! harnesses share a single managed region in `~/.cursor/hooks.json`,
//! delegating to [`cursor_common`].
//!
//! ## Partial event coverage
//!
//! Cursor CLI (`cursor-agent`) supports a strict subset of the events
//! Cursor IDE fires. As of this writing, only `beforeShellExecution`
//! and `afterShellExecution` reliably reach hook scripts when invoked
//! from the CLI. The MVP installs only those two events anyway, so the
//! coverage gap is invisible at the wire level — but the UI surfaces
//! a "partial event coverage" advisory on the cursor-cli row to manage
//! user expectations. See [`HarnessList.tsx`] for that copy.

use std::path::{Path, PathBuf};

use crate::ipc::IpcError;

use super::cursor_common;
use super::{ApplyOptions, PatchPreview, TrovePatch};

/// Resolve the absolute path of `~/.cursor/hooks.json` under `home`.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    cursor_common::config_path(home)
}

/// See [`cursor_common::preview`].
pub fn preview(
    home: &Path,
    opts: &ApplyOptions,
    hook_script_path: &Path,
) -> Result<PatchPreview, IpcError> {
    cursor_common::preview(home, opts, hook_script_path)
}

/// See [`cursor_common::apply`].
pub fn apply(
    home: &Path,
    opts: &ApplyOptions,
    hook_script_path: &Path,
) -> Result<TrovePatch, IpcError> {
    cursor_common::apply(home, opts, hook_script_path)
}

/// See [`cursor_common::revert`].
pub fn revert(home: &Path) -> Result<(), IpcError> {
    cursor_common::revert(home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn fake_hook_path() -> PathBuf {
        PathBuf::from("/opt/trove/resources/hooks/cursor-otel-hook.cjs")
    }

    #[test]
    fn config_path_matches_cursor_ide() {
        // Both cursor harnesses must resolve to the same host file.
        let home = PathBuf::from("/home/dev");
        assert_eq!(config_path(&home), super::super::cursor_ide::config_path(&home));
    }

    #[test]
    fn apply_creates_a_valid_hooks_file() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        let written = fs::read_to_string(config_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed["version"], 1);
        assert!(parsed["hooks"]["beforeShellExecution"].is_array());
        assert!(parsed.get("_trove").is_some());
    }

    #[test]
    fn cli_apply_then_ide_apply_is_idempotent() {
        // Inverse of cursor_ide's matching test — the CLI side first.
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        let after_cli = fs::read_to_string(config_path(home.path())).unwrap();

        super::super::cursor_ide::apply(
            home.path(),
            &ApplyOptions::default(),
            &fake_hook_path(),
        )
        .unwrap();
        let after_ide = fs::read_to_string(config_path(home.path())).unwrap();

        assert_eq!(after_cli, after_ide);
    }

    #[test]
    fn revert_via_cli_clears_block_written_by_ide() {
        // The shared-region invariant: a revert via either harness must
        // remove the block entirely. This catches the regression where
        // the two harnesses might accidentally write distinct blocks.
        let home = tempdir().unwrap();
        super::super::cursor_ide::apply(
            home.path(),
            &ApplyOptions::default(),
            &fake_hook_path(),
        )
        .unwrap();

        revert(home.path()).unwrap();

        let written = fs::read_to_string(config_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert!(parsed.get("_trove").is_none());
    }
}
