//! Cursor IDE adapter — installs Trove's hook script in
//! `~/.cursor/hooks.json`. The patch is shared with `cursor_cli`: both
//! harnesses delegate to [`cursor_common`] which owns a single managed
//! region inside the host file. Re-applying via `cursor_cli` after
//! `cursor_ide` (or vice versa) is a no-op when the hook path matches.
//!
//! See [`cursor_common`] for the safety contract; this module exists to
//! give the UI a separate Enable / Disable toggle for the IDE harness.

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
    fn config_path_resolves_to_dot_cursor_hooks_json() {
        let home = PathBuf::from("/home/dev");
        assert_eq!(
            config_path(&home),
            PathBuf::from("/home/dev/.cursor/hooks.json")
        );
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
    fn cursor_ide_apply_does_not_touch_cursor_cli_shell_rc() {
        // Previously the two cursor adapters shared `~/.cursor/hooks.json`
        // and applying one was byte-for-byte idempotent with the other.
        // After the cursor-cli wrapper rewrite they patch different host
        // files — cursor-ide owns hooks.json, cursor-cli owns the shell
        // rc — so the invariant we care about now is: enabling cursor-ide
        // leaves cursor-cli's host file unchanged.
        let home = tempdir().unwrap();
        // Seed a shell rc so cursor-cli has a file to *not* patch here
        // (wrapper_common refuses to apply when no rc exists).
        let zshrc = home.path().join(".zshrc");
        fs::write(&zshrc, "# pre-existing user content\n").unwrap();
        let zshrc_before = fs::read_to_string(&zshrc).unwrap();

        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();

        // cursor-ide patched hooks.json …
        let hooks = fs::read_to_string(config_path(home.path())).unwrap();
        assert!(hooks.contains("_trove"), "cursor-ide must patch hooks.json");
        // … and left cursor-cli's shell rc alone.
        let zshrc_after = fs::read_to_string(&zshrc).unwrap();
        assert_eq!(zshrc_before, zshrc_after);
    }

    #[test]
    fn revert_removes_the_managed_block() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default(), &fake_hook_path()).unwrap();
        revert(home.path()).unwrap();

        // After revert, the file should still exist (it had no user
        // content originally) but the _trove block is gone.
        let written = fs::read_to_string(config_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert!(parsed.get("_trove").is_none());
        assert!(parsed.get("hooks").is_none() || parsed["hooks"]["beforeShellExecution"].is_null());
    }
}
