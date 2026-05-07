//! Platform-aware lookup of the standard config-dir path for each
//! Tier 1 harness, plus the macOS app-bundle path where one exists.
//!
//! Each Tier 1 harness uses a single canonical install location across
//! platforms (`~/.<name>/`); on Linux, codex-cli also honors
//! `$XDG_CONFIG_HOME/codex/`. Detection accepts the home dir as an
//! argument so tests can scope it to a `tempdir()`.

use std::path::{Path, PathBuf};

use crate::harness::HarnessId;

/// Returns every config path Trove will probe to decide whether
/// `harness` is installed. Order matters — the first existing entry is
/// reported as the harness's `config_path`. Returns an empty vector for
/// harnesses Trove does not yet detect (Tier 2 / Tier 3).
#[must_use]
pub fn config_search_paths(harness: HarnessId, home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(2);
    match harness {
        HarnessId::ClaudeCode => paths.push(home.join(".claude").join("settings.json")),
        HarnessId::GeminiCli => paths.push(home.join(".gemini").join("settings.json")),
        HarnessId::CodexCli => {
            paths.push(home.join(".codex").join("config.toml"));
            // Linux codex-cli also reads $XDG_CONFIG_HOME/codex/config.toml
            // when the user has set it. Plan agent flagged this quirk.
            #[cfg(target_os = "linux")]
            if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
                paths.push(PathBuf::from(xdg).join("codex").join("config.toml"));
            }
        }
        HarnessId::QwenCode => paths.push(home.join(".qwen").join("settings.json")),
        // Tier 2 / Tier 3 land in later sprints. Returning an empty
        // search path keeps the detector's "not detected" answer honest
        // until those adapters arrive.
        _ => {}
    }
    paths
}

/// Returns the standard macOS application-bundle path for `harness`
/// when one exists, scoped under `app_root` (defaults to
/// `/Applications`; tests pass a tempdir). Returns `None` for harnesses
/// without a native macOS app bundle and on non-macOS targets.
#[must_use]
pub fn app_bundle_path(harness: HarnessId, app_root: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    match harness {
        // Claude Code ships /Applications/Claude.app on macOS.
        HarnessId::ClaudeCode => Some(app_root.join("Claude.app")),
        // Cursor IDE ships /Applications/Cursor.app — Sprint 7 will
        // start probing for it but Sprint 3 doesn't yet.
        _ => None,
    }
}

/// Default macOS `/Applications` root. Production code calls this;
/// tests pass an explicit `Path` to [`app_bundle_path`] instead.
#[cfg(target_os = "macos")]
#[must_use]
pub fn default_app_root() -> PathBuf {
    PathBuf::from("/Applications")
}

#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn default_app_root() -> PathBuf {
    PathBuf::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn claude_code_resolves_to_dot_claude_settings() {
        let home = PathBuf::from("/home/dev");
        let paths = config_search_paths(HarnessId::ClaudeCode, &home);
        assert_eq!(
            paths,
            vec![PathBuf::from("/home/dev/.claude/settings.json")]
        );
    }

    #[test]
    fn gemini_cli_resolves_to_dot_gemini_settings() {
        let home = PathBuf::from("/home/dev");
        let paths = config_search_paths(HarnessId::GeminiCli, &home);
        assert_eq!(
            paths,
            vec![PathBuf::from("/home/dev/.gemini/settings.json")]
        );
    }

    #[test]
    fn codex_cli_resolves_to_dot_codex_config_toml() {
        let home = PathBuf::from("/home/dev");
        let paths = config_search_paths(HarnessId::CodexCli, &home);
        // First entry is always ~/.codex/config.toml; Linux may add a
        // second XDG entry depending on env at compile time.
        assert_eq!(paths[0], PathBuf::from("/home/dev/.codex/config.toml"));
    }

    #[test]
    fn qwen_code_resolves_to_dot_qwen_settings() {
        let home = PathBuf::from("/home/dev");
        let paths = config_search_paths(HarnessId::QwenCode, &home);
        assert_eq!(paths, vec![PathBuf::from("/home/dev/.qwen/settings.json")]);
    }

    #[test]
    fn tier_two_returns_empty_search_paths() {
        let home = PathBuf::from("/home/dev");
        for id in [
            HarnessId::Opencode,
            HarnessId::CursorIde,
            HarnessId::CursorCli,
            HarnessId::Cline,
            HarnessId::Aider,
            HarnessId::CopilotCli,
        ] {
            assert!(
                config_search_paths(id, &home).is_empty(),
                "unexpected paths returned for {id:?}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn claude_code_app_bundle_under_apps_root() {
        let root = PathBuf::from("/tmp/Applications");
        assert_eq!(
            app_bundle_path(HarnessId::ClaudeCode, &root),
            Some(PathBuf::from("/tmp/Applications/Claude.app"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn other_tier_one_harnesses_have_no_app_bundle() {
        let root = PathBuf::from("/Applications");
        for id in [
            HarnessId::GeminiCli,
            HarnessId::CodexCli,
            HarnessId::QwenCode,
        ] {
            assert!(
                app_bundle_path(id, &root).is_none(),
                "unexpected bundle for {id:?}"
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn no_app_bundles_outside_macos() {
        let root = PathBuf::from("/Applications");
        assert!(app_bundle_path(HarnessId::ClaudeCode, &root).is_none());
    }
}
