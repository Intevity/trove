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
        // Claude Desktop (formerly Cowork) is an Electron app. The
        // OTLP exporter setup lives in the Claude admin web UI for
        // Team/Enterprise tenants — there is no local config Trove
        // can patch. Detection therefore relies entirely on the app
        // bundle (macOS) or Programs-folder probe (Windows/Linux),
        // handled outside `config_search_paths`. We still leave a
        // hook for a per-user preferences file in case Anthropic
        // ships one later (verified empty for now).
        HarnessId::AntigravityCli => {
            // Antigravity CLI (`agy`) keeps its config under
            // `~/.gemini/antigravity-cli/`. It dropped Gemini CLI's native
            // OTLP exporter, so Trove registers its telemetry bridge as a
            // hook in `hooks.json` there (mirroring Cursor's `~/.cursor/
            // hooks.json`). The file only appears once a hook is
            // configured, so the appDataDir itself counts as install
            // evidence (the dir-fallback returns Unknown/false pre-apply,
            // which is the correct state).
            paths.push(
                home.join(".gemini")
                    .join("antigravity-cli")
                    .join("hooks.json"),
            );
            paths.push(home.join(".gemini").join("antigravity-cli"));
        }
        HarnessId::CodexCli | HarnessId::CodexDesktop => {
            // Codex CLI and Codex desktop both read ~/.codex/config.toml
            // — the desktop app's Electron shell still delegates to the
            // bundled Rust `codex app-server`, which reads the same TOML.
            // Trove instruments both with one shared, dep-tracked
            // managed region (see safety::sentinels::comment_fence).
            paths.push(home.join(".codex").join("config.toml"));
            // Linux codex-cli also reads $XDG_CONFIG_HOME/codex/config.toml
            // when the user has set it. Plan agent flagged this quirk.
            #[cfg(target_os = "linux")]
            if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
                paths.push(PathBuf::from(xdg).join("codex").join("config.toml"));
            }
        }
        HarnessId::QwenCode => paths.push(home.join(".qwen").join("settings.json")),
        HarnessId::Opencode => {
            paths.push(home.join(".config").join("opencode").join("opencode.json"));
            // Honour $XDG_CONFIG_HOME on Linux; OpenCode follows the
            // standard XDG path resolution for its config dir.
            #[cfg(target_os = "linux")]
            if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
                paths.push(PathBuf::from(xdg).join("opencode").join("opencode.json"));
            }
        }
        HarnessId::CursorIde | HarnessId::CursorCli => {
            // Both Cursor harnesses share `~/.cursor/hooks.json`. The
            // file may not exist before Trove runs (`~/.cursor/` itself
            // is created by Cursor on first launch, but `hooks.json`
            // only appears once hooks have been configured), so we also
            // accept the `.cursor` directory as evidence of the harness
            // being installed. The directory survives the read attempts
            // in `read_telemetry` / `read_trove_region_present` by
            // returning `Unknown` / `false` — which is the correct
            // pre-apply state.
            paths.push(home.join(".cursor").join("hooks.json"));
            paths.push(home.join(".cursor"));
        }
        HarnessId::Cline => {
            // Sprint 9 PR 2 — Cline is the VSCode extension at
            // `saoudrizwan.claude-dev`. Detection signals (in order):
            // 1. globalStorage/<id>/tasks (strongest — implies usage)
            // 2. globalStorage/<id> (extension has run at least once)
            // 3. .vscode/extensions/<id>-* (extension installed but not
            //    yet run; the suffix is a semver added by VSCode)
            // 4. ~/.cline/data/sessions (standalone CLI, npm-installed
            //    `cline` 3.x+) — same harness id, separate storage tree.
            //    Either signal source triggers detection.
            // The watcher follows config_path() → globalStorage root.
            paths.push(crate::adapters::cline::tasks_dir(home));
            paths.push(crate::adapters::cline::cline_global_storage_root(home));
            paths.extend(cline_extension_dir_candidates(home));
            paths.push(crate::adapters::cline::cli_sessions_dir(home));
        }
        // Sprint 9 PR 3 — Aider / Copilot CLI detect via PATH binary
        // only (`aider`, `gh`); no config dir to probe. The detector's
        // `read_telemetry` / `read_trove_region_present` hooks use the
        // shell-rc patch state (overlayed by the IPC layer from
        // state.json).
        // Detection-only harnesses with a canonical dotfile directory.
        // Trove has no adapter for these, so config_search_paths only
        // contributes the config-dir detection signal; the dashboard
        // shows the row but keeps its toggle disabled.
        HarnessId::JunieCli => paths.push(home.join(".junie")),
        HarnessId::Droid => paths.push(home.join(".factory").join("settings.json")),
        HarnessId::KimiCodeCli => paths.push(home.join(".kimi")),
        HarnessId::Devin => paths.push(home.join(".devin")),
        HarnessId::Forgecode => paths.push(home.join(".forge")),
        // Sentinel stores its (integrity-signed) config at
        // ~/.sentinel/settings.json; its presence implies Sentinel has run
        // at least once. Trove never patches this file — detection is the
        // only signal it contributes.
        HarnessId::Sentinel => paths.push(home.join(".sentinel").join("settings.json")),
        HarnessId::ClaudeDesktop | HarnessId::Aider | HarnessId::CopilotCli => {}
    }
    paths
}

/// Cline ships under `~/.vscode/extensions/saoudrizwan.claude-dev-<semver>/`
/// where `<semver>` matches whichever version `VSCode` installed. Returns
/// every directory entry that matches the prefix; the detector accepts
/// the first one that exists.
#[must_use]
pub fn cline_extension_dir_candidates(home: &Path) -> Vec<PathBuf> {
    let parents = [
        home.join(".vscode").join("extensions"),
        home.join(".vscode-insiders").join("extensions"),
        // Cursor reuses the VSCode extensions layout; check there too
        // since Cline runs under Cursor as well as vanilla VSCode.
        home.join(".cursor").join("extensions"),
    ];
    let prefix = format!("{}-", crate::adapters::cline::EXTENSION_ID);
    let mut out = Vec::new();
    for parent in parents {
        let Ok(rd) = std::fs::read_dir(&parent) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name();
            let Some(s) = name.to_str() else { continue };
            if s.starts_with(&prefix) {
                out.push(entry.path());
            }
        }
    }
    out
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
        // Anthropic's `/Applications/Claude.app` is the desktop app
        // (formerly Cowork) that hosts the OTLP emitter. Trove's
        // Claude Code (CLI) detection relied on this same bundle as a
        // secondary signal; we leave that in place so an existing user
        // who only has the desktop app still sees Claude Code detected
        // when the `claude` CLI ships alongside, but the canonical
        // owner of the bundle from a UX point of view is Claude Desktop.
        HarnessId::ClaudeCode | HarnessId::ClaudeDesktop => Some(app_root.join("Claude.app")),
        // Cursor IDE ships /Applications/Cursor.app on macOS. Cursor CLI
        // (`cursor-agent`) doesn't get its own bundle — it's detected via
        // the binary on PATH instead.
        HarnessId::CursorIde => Some(app_root.join("Cursor.app")),
        // OpenAI Codex desktop app ships as /Applications/Codex.app.
        // Codex CLI is detected via the `codex` binary on PATH instead.
        HarnessId::CodexDesktop => Some(app_root.join("Codex.app")),
        // Sentinel ships as /Applications/Sentinel.app (productName
        // "Sentinel", bundle id com.jeffwooden.claude-sentinel).
        HarnessId::Sentinel => Some(app_root.join("Sentinel.app")),
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
    fn antigravity_cli_resolves_to_appdata_hooks_then_dir() {
        let home = PathBuf::from("/home/dev");
        let paths = config_search_paths(HarnessId::AntigravityCli, &home);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/home/dev/.gemini/antigravity-cli/hooks.json"),
                PathBuf::from("/home/dev/.gemini/antigravity-cli"),
            ]
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
    fn cursor_harnesses_resolve_to_dot_cursor_hooks_json_with_dir_fallback() {
        let home = PathBuf::from("/home/dev");
        for id in [HarnessId::CursorIde, HarnessId::CursorCli] {
            let paths = config_search_paths(id, &home);
            assert_eq!(paths.len(), 2, "expected hooks.json + dir for {id:?}");
            assert_eq!(paths[0], PathBuf::from("/home/dev/.cursor/hooks.json"));
            assert_eq!(paths[1], PathBuf::from("/home/dev/.cursor"));
        }
    }

    #[test]
    fn opencode_resolves_to_dot_config_opencode_opencode_json() {
        let home = PathBuf::from("/home/dev");
        let paths = config_search_paths(HarnessId::Opencode, &home);
        assert_eq!(
            paths[0],
            PathBuf::from("/home/dev/.config/opencode/opencode.json"),
        );
    }

    #[test]
    fn aider_and_copilot_return_empty_search_paths_until_pr_3() {
        // Sprint 9 PR 2 wires Cline; PR 3 wires the other two.
        let home = PathBuf::from("/home/dev");
        for id in [HarnessId::Aider, HarnessId::CopilotCli] {
            assert!(
                config_search_paths(id, &home).is_empty(),
                "unexpected paths returned for {id:?}"
            );
        }
    }

    #[test]
    fn cline_search_paths_include_global_storage_and_tasks_dir() {
        let home = PathBuf::from("/home/dev");
        let paths = config_search_paths(HarnessId::Cline, &home);
        assert!(
            !paths.is_empty(),
            "expected at least one cline search path"
        );
        let joined: String = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(",");
        assert!(joined.contains("globalStorage"), "{joined}");
        assert!(joined.contains("saoudrizwan.claude-dev"), "{joined}");
        // First entry is the strongest (tasks) — implies usage.
        assert_eq!(paths[0].file_name().unwrap(), "tasks");
    }

    #[test]
    fn cline_extension_dir_candidates_include_vscode_and_cursor_extension_roots() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();

        let make_ext = |parent_segments: &[&str]| {
            let mut p = home.to_path_buf();
            for seg in parent_segments {
                p.push(seg);
            }
            std::fs::create_dir_all(&p).unwrap();
            // Plant a directory shaped like VSCode's install path:
            // <id>-<semver>
            let ext = p.join("saoudrizwan.claude-dev-3.4.5");
            std::fs::create_dir_all(&ext).unwrap();
            ext
        };
        make_ext(&[".vscode", "extensions"]);
        make_ext(&[".cursor", "extensions"]);

        let candidates = cline_extension_dir_candidates(home);
        assert_eq!(candidates.len(), 2, "{candidates:?}");
        for c in &candidates {
            let s = c.to_string_lossy();
            assert!(s.contains("saoudrizwan.claude-dev"), "{s}");
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
    fn claude_desktop_app_bundle_is_the_same_claude_app() {
        // Both Claude Code (CLI) and Claude Desktop (Electron app) ship
        // alongside `/Applications/Claude.app` — the bundle is the same
        // for both. Independent detection means the user sees two rows:
        // the CLI (managed via ~/.claude/settings.json) and the desktop
        // app (managed via the Claude admin web UI).
        let root = PathBuf::from("/tmp/Applications");
        assert_eq!(
            app_bundle_path(HarnessId::ClaudeDesktop, &root),
            Some(PathBuf::from("/tmp/Applications/Claude.app"))
        );
    }

    #[test]
    fn claude_desktop_has_no_config_search_paths() {
        // Claude Desktop's OTLP setup is admin-managed; there is no
        // local config file Trove probes for it.
        let home = PathBuf::from("/home/dev");
        assert!(config_search_paths(HarnessId::ClaudeDesktop, &home).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cursor_ide_app_bundle_under_apps_root() {
        let root = PathBuf::from("/tmp/Applications");
        assert_eq!(
            app_bundle_path(HarnessId::CursorIde, &root),
            Some(PathBuf::from("/tmp/Applications/Cursor.app"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn codex_desktop_app_bundle_under_apps_root() {
        let root = PathBuf::from("/tmp/Applications");
        assert_eq!(
            app_bundle_path(HarnessId::CodexDesktop, &root),
            Some(PathBuf::from("/tmp/Applications/Codex.app"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sentinel_app_bundle_under_apps_root() {
        let root = PathBuf::from("/tmp/Applications");
        assert_eq!(
            app_bundle_path(HarnessId::Sentinel, &root),
            Some(PathBuf::from("/tmp/Applications/Sentinel.app"))
        );
    }

    #[test]
    fn sentinel_config_search_path_is_dot_sentinel_settings_json() {
        let home = PathBuf::from("/home/dev");
        let paths = config_search_paths(HarnessId::Sentinel, &home);
        assert_eq!(
            paths[0],
            PathBuf::from("/home/dev/.sentinel/settings.json"),
        );
    }

    #[test]
    fn codex_desktop_resolves_to_same_config_toml_as_codex_cli() {
        let home = PathBuf::from("/home/dev");
        let cli = config_search_paths(HarnessId::CodexCli, &home);
        let desk = config_search_paths(HarnessId::CodexDesktop, &home);
        assert_eq!(cli, desk);
        assert_eq!(desk[0], PathBuf::from("/home/dev/.codex/config.toml"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cursor_cli_has_no_app_bundle() {
        // Cursor CLI is detected via the cursor-agent binary on PATH;
        // it has no native macOS bundle of its own.
        let root = PathBuf::from("/tmp/Applications");
        assert!(app_bundle_path(HarnessId::CursorCli, &root).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn other_tier_one_harnesses_have_no_app_bundle() {
        let root = PathBuf::from("/Applications");
        for id in [
            HarnessId::AntigravityCli,
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
