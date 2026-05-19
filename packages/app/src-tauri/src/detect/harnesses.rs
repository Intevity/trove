//! Per-harness detection. Each Tier 1 harness has its own `detect_*`
//! function returning a structured [`DetectedHarness`]. Detection
//! signals are tried in order: config-dir (strongest, implies the
//! harness has been used at least once), PATH binary, then macOS
//! app-bundle.

use std::path::Path;

use crate::harness::HarnessId;
use crate::safety::sentinels::{Format, extract_region};

use super::paths::{app_bundle_path, config_search_paths};
use super::probe::{probe_path, probe_path_in};
use super::{DetectedHarness, DetectionMethod, Detector, TelemetryStatus};

/// Detect `id` against `detector`. Returns a row whose `detected`
/// field is `true` iff at least one signal fired.
pub fn detect(id: HarnessId, detector: &Detector) -> DetectedHarness {
    let config_path = config_search_paths(id, &detector.home)
        .into_iter()
        .find(|p| p.exists());

    // codex-cli and codex-desktop share `~/.codex/config.toml`, so its
    // mere presence cannot distinguish which one is installed — it
    // appears whenever either harness has run. For those two, require a
    // harness-specific signal (PATH binary for the CLI; app-bundle for
    // the desktop) to claim detection. The shared file is still read
    // below for telemetry status + trove-region presence.
    let config_is_unique_signal = !matches!(id, HarnessId::CodexCli | HarnessId::CodexDesktop);

    let mut detection_method = if config_is_unique_signal {
        config_path.as_ref().map(|_| DetectionMethod::ConfigDir)
    } else {
        None
    };

    let binary_name = path_binary_name(id);
    let path_hit = match (binary_name, detector.path_dirs.as_deref()) {
        (Some(name), Some(dirs)) => probe_path_in(name, dirs),
        (Some(name), None) => probe_path(name),
        (None, _) => None,
    };
    if detection_method.is_none() && path_hit.is_some() {
        detection_method = Some(DetectionMethod::PathBinary);
    }

    let bundle_hit = app_bundle_path(id, &detector.app_root).filter(|p| p.exists());
    if detection_method.is_none() && bundle_hit.is_some() {
        detection_method = Some(DetectionMethod::AppBundle);
    }

    // Claude Desktop ships as an Electron app on macOS (Claude.app —
    // handled above), Windows (`%LOCALAPPDATA%\AnthropicClaude\Claude.exe`
    // or similar), and Linux (per-distro AppImage / .desktop entry).
    // Probe those non-macOS install locations only when no signal has
    // fired yet so we don't override an existing detection_method.
    if matches!(id, HarnessId::ClaudeDesktop) && detection_method.is_none() {
        if let Some(p) = probe_claude_desktop_install(&detector.home) {
            if p.exists() {
                detection_method = Some(DetectionMethod::AppBundle);
            }
        }
    }

    let (telemetry, trove_region_present) = match config_path.as_deref() {
        Some(path) => (read_telemetry(id, path), read_trove_region_present(id, path)),
        None => (TelemetryStatus::Unknown, false),
    };

    DetectedHarness {
        id,
        detected: detection_method.is_some(),
        config_path,
        telemetry,
        detection_method,
        trove_region_present,
        adapter_available: id.has_adapter(),
    }
}

fn path_binary_name(id: HarnessId) -> Option<&'static str> {
    match id {
        HarnessId::ClaudeCode => Some("claude"),
        HarnessId::GeminiCli => Some("gemini"),
        HarnessId::CodexCli => Some("codex"),
        HarnessId::QwenCode => Some("qwen"),
        // Cursor CLI binary is `cursor-agent`. Cursor IDE has no canonical
        // PATH binary on macOS / Linux (the user launches it via the app
        // bundle), so detection there relies on config-dir + app-bundle.
        HarnessId::CursorCli => Some("cursor-agent"),
        HarnessId::Opencode => Some("opencode"),
        // Sprint 9 PR 3 — Tier 3 wrapper-based adapters detect via
        // PATH presence of the underlying tool. Cline (PR 2) uses
        // config-dir signals instead and returns None here.
        HarnessId::Aider => Some("aider"),
        HarnessId::CopilotCli => Some("gh"),
        // Detection-only harnesses: PATH binary is the primary signal.
        HarnessId::JunieCli => Some("junie"),
        HarnessId::Droid => Some("droid"),
        HarnessId::KimiCodeCli => Some("kimi"),
        HarnessId::Devin => Some("devin"),
        HarnessId::Forgecode => Some("forge"),
        // Claude Desktop is a GUI Electron app; it ships no canonical
        // PATH binary. Detection is bundle-only (handled in
        // `app_bundle_path`).
        _ => None,
    }
}

/// Whether `path` contains a Trove-managed region. Returns false on
/// missing or unparseable files (the dashboard surfaces the parse
/// failure separately via the telemetry-status `Unknown` and the
/// preview/apply error path).
fn read_trove_region_present(id: HarnessId, path: &Path) -> bool {
    let format = match id {
        HarnessId::ClaudeCode
        | HarnessId::GeminiCli
        | HarnessId::QwenCode
        | HarnessId::CursorIde
        | HarnessId::CursorCli
        | HarnessId::Opencode => Format::Json,
        HarnessId::CodexCli | HarnessId::CodexDesktop => Format::Toml,
        _ => return false,
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    matches!(extract_region(format, &text), Ok(Some(_)))
}

fn read_telemetry(id: HarnessId, path: &Path) -> TelemetryStatus {
    let Ok(text) = std::fs::read_to_string(path) else {
        return TelemetryStatus::Unknown;
    };
    match id {
        HarnessId::ClaudeCode => check_claude_telemetry(&text),
        HarnessId::GeminiCli | HarnessId::QwenCode => check_gemini_like_telemetry(&text),
        HarnessId::CodexCli | HarnessId::CodexDesktop => check_codex_telemetry(&text),
        HarnessId::CursorIde | HarnessId::CursorCli => check_cursor_telemetry(&text),
        HarnessId::Opencode => check_opencode_telemetry(&text),
        _ => TelemetryStatus::Unknown,
    }
}

/// Claude Code's telemetry master switch is `env.CLAUDE_CODE_ENABLE_TELEMETRY`.
/// "1" or boolean true → On; anything else with the file present → Off;
/// unparseable → Unknown.
fn check_claude_telemetry(text: &str) -> TelemetryStatus {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return TelemetryStatus::Unknown;
    };
    let env = value.get("env").and_then(serde_json::Value::as_object);
    match env.and_then(|e| e.get("CLAUDE_CODE_ENABLE_TELEMETRY")) {
        Some(serde_json::Value::String(s)) if s == "1" || s == "true" => TelemetryStatus::On,
        Some(serde_json::Value::Bool(true)) => TelemetryStatus::On,
        Some(_) | None => TelemetryStatus::Off,
    }
}

/// Gemini CLI and Qwen Code use a `telemetry.enabled` boolean.
fn check_gemini_like_telemetry(text: &str) -> TelemetryStatus {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return TelemetryStatus::Unknown;
    };
    match value.get("telemetry").and_then(|t| t.get("enabled")) {
        Some(serde_json::Value::Bool(true)) => TelemetryStatus::On,
        Some(_) | None => TelemetryStatus::Off,
    }
}

/// Codex CLI's telemetry "master switch" is the kind of the exporter
/// configured under `[otel.exporter]` / `[otel.trace_exporter]` /
/// `[otel.metrics_exporter]`. Codex has no boolean enable toggle —
/// setting any exporter `kind` to `"otlp-http"` or `"otlp-grpc"` turns
/// OTLP emission on. Presence of the Trove fence also implies On
/// (Trove only writes the fence when it has installed an OTLP exporter).
/// Returns Unknown only on TOML parse failure.
fn check_codex_telemetry(text: &str) -> TelemetryStatus {
    if text.contains("# trove:start") {
        return TelemetryStatus::On;
    }
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return TelemetryStatus::Unknown;
    };
    let Some(otel) = doc.get("otel").and_then(|v| v.as_table()) else {
        return TelemetryStatus::Off;
    };
    for slot in ["exporter", "trace_exporter", "metrics_exporter"] {
        let kind = otel
            .get(slot)
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("kind"))
            .and_then(|v| v.as_str());
        if matches!(kind, Some("otlp-http" | "otlp-grpc")) {
            return TelemetryStatus::On;
        }
    }
    TelemetryStatus::Off
}

/// Cursor has no native telemetry toggle — Trove's only signal is
/// whether our own hook block is installed in `hooks.json`. Returns
/// `On` if a `_trove` block is present, `Off` otherwise. JSON parse
/// failures bubble up as `Unknown`.
fn check_cursor_telemetry(text: &str) -> TelemetryStatus {
    match extract_region(Format::Json, text) {
        Ok(Some(_)) => TelemetryStatus::On,
        Ok(None) => TelemetryStatus::Off,
        Err(_) => TelemetryStatus::Unknown,
    }
}

/// `OpenCode`'s telemetry signal is whether the `OTel` plugin is
/// registered in the top-level `plugin` array (or, equivalently,
/// whether Trove's own block is present — Trove only writes the block
/// when it's registering the plugin). Returns `On` for either; `Off`
/// otherwise. JSON parse failures bubble up as `Unknown`.
/// Best-effort probe for a Claude Desktop install on Windows / Linux.
/// macOS is covered by [`super::paths::app_bundle_path`]; this helper
/// fills in the gap for the other two platforms. Returns the first
/// candidate path that *might* exist; the caller still has to call
/// `.exists()` against it. Returns `None` on macOS.
fn probe_claude_desktop_install(home: &Path) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let _ = home;
        None
    }
    #[cfg(target_os = "windows")]
    {
        // The official installer drops the app under
        // `%LOCALAPPDATA%\AnthropicClaude\Claude.exe`. Fall back to
        // `%LOCALAPPDATA%\Programs\Claude\Claude.exe` for the older
        // Programs-folder layout.
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = std::path::PathBuf::from(local);
            let primary = local.join("AnthropicClaude").join("Claude.exe");
            if primary.exists() {
                return Some(primary);
            }
            let fallback = local.join("Programs").join("Claude").join("Claude.exe");
            if fallback.exists() {
                return Some(fallback);
            }
        }
        let _ = home;
        None
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        // Linux: the AppImage typically lands under
        // ~/.local/share/applications/ or ~/.local/bin/claude — neither
        // is canonical, so we probe a small set of common spots.
        for tail in [
            ".local/share/applications/claude.desktop",
            ".local/bin/claude-desktop",
            ".local/share/Claude/Claude",
            "Applications/Claude.AppImage",
        ] {
            let p = home.join(tail);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }
}

fn check_opencode_telemetry(text: &str) -> TelemetryStatus {
    if matches!(extract_region(Format::Json, text), Ok(Some(_))) {
        return TelemetryStatus::On;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return TelemetryStatus::Unknown;
    };
    let plugins = value.get("plugin").and_then(serde_json::Value::as_array);
    let has_otel = plugins.is_some_and(|arr| {
        arr.iter().any(|v| {
            v.as_str()
                .is_some_and(|s| s == crate::adapters::opencode::OTEL_PLUGIN_PACKAGE)
        })
    });
    if has_otel {
        TelemetryStatus::On
    } else {
        TelemetryStatus::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn detector_for(home: &Path) -> Detector {
        Detector {
            home: home.to_path_buf(),
            path_dirs: Some(Vec::new()),  // empty = no PATH hits in tests
            app_root: home.to_path_buf(), // far away from /Applications
        }
    }

    fn write_settings(path: &Path, json: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, json).unwrap();
    }

    #[test]
    fn claude_code_not_detected_when_nothing_present() {
        let home = tempdir().unwrap();
        let result = detect(HarnessId::ClaudeCode, &detector_for(home.path()));
        assert!(!result.detected);
        assert!(result.config_path.is_none());
        assert_eq!(result.telemetry, TelemetryStatus::Unknown);
        assert_eq!(result.detection_method, None);
    }

    #[test]
    fn claude_code_detected_via_config_dir() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".claude").join("settings.json");
        write_settings(&cfg, "{}");

        let result = detect(HarnessId::ClaudeCode, &detector_for(home.path()));
        assert!(result.detected);
        assert_eq!(result.config_path.as_deref(), Some(cfg.as_path()));
        assert_eq!(result.detection_method, Some(DetectionMethod::ConfigDir));
        assert_eq!(result.telemetry, TelemetryStatus::Off);
    }

    #[test]
    fn claude_code_telemetry_on_when_enable_var_is_one() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".claude").join("settings.json");
        write_settings(&cfg, r#"{"env":{"CLAUDE_CODE_ENABLE_TELEMETRY":"1"}}"#);

        let result = detect(HarnessId::ClaudeCode, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn claude_code_telemetry_unknown_when_config_malformed() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".claude").join("settings.json");
        write_settings(&cfg, "{not valid json");

        let result = detect(HarnessId::ClaudeCode, &detector_for(home.path()));
        assert!(
            result.detected,
            "presence of file still implies installation"
        );
        assert_eq!(result.telemetry, TelemetryStatus::Unknown);
    }

    #[test]
    fn claude_code_detected_via_path_when_no_config() {
        let home = tempdir().unwrap();
        let bin_dir = tempdir().unwrap();
        let exe_name = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        let exe = bin_dir.path().join(exe_name);
        fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(vec![bin_dir.path().to_path_buf()]),
            app_root: home.path().to_path_buf(),
        };

        let result = detect(HarnessId::ClaudeCode, &detector);
        assert!(result.detected);
        assert!(result.config_path.is_none());
        assert_eq!(result.detection_method, Some(DetectionMethod::PathBinary));
    }

    #[test]
    fn gemini_telemetry_on_when_enabled_true() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".gemini").join("settings.json");
        write_settings(&cfg, r#"{"telemetry":{"enabled":true}}"#);

        let result = detect(HarnessId::GeminiCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn gemini_telemetry_off_when_enabled_false() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".gemini").join("settings.json");
        write_settings(&cfg, r#"{"telemetry":{"enabled":false}}"#);

        let result = detect(HarnessId::GeminiCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::Off);
    }

    #[test]
    fn qwen_uses_gemini_like_check() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".qwen").join("settings.json");
        write_settings(&cfg, r#"{"telemetry":{"enabled":true}}"#);

        let result = detect(HarnessId::QwenCode, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn codex_telemetry_on_when_otlp_http_exporter_configured() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(
            &cfg,
            "[otel.exporter]\nkind = \"otlp-http\"\nendpoint = \"http://127.0.0.1:4318/v1/logs\"\nprotocol = \"binary\"\n",
        );

        let result = detect(HarnessId::CodexCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn codex_telemetry_on_when_only_trace_or_metrics_exporter_is_otlp() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(
            &cfg,
            "[otel.metrics_exporter]\nkind = \"otlp-grpc\"\nendpoint = \"http://127.0.0.1:4317\"\n",
        );

        let result = detect(HarnessId::CodexCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn codex_telemetry_on_when_trove_fence_present() {
        // The fence is Trove's signature; treat it as authoritative even
        // before TOML parsing succeeds.
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(
            &cfg,
            "# trove:start hash=abc keys=otel.exporter\n[otel.exporter]\nkind = \"otlp-http\"\nendpoint = \"http://127.0.0.1:4318/v1/logs\"\nprotocol = \"binary\"\n# trove:end\n",
        );

        let result = detect(HarnessId::CodexCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn codex_telemetry_off_when_exporter_kind_is_none() {
        // Codex's `none` exporter means "do not emit" — the user has
        // explicitly disabled, so we should not surface as On.
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(&cfg, "[otel.exporter]\nkind = \"none\"\n");

        let result = detect(HarnessId::CodexCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::Off);
    }

    #[test]
    fn codex_telemetry_off_when_otel_table_missing() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(&cfg, "[user]\nname = \"a\"\n");

        let result = detect(HarnessId::CodexCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::Off);
    }

    #[test]
    fn codex_telemetry_off_when_otel_table_has_only_unrelated_keys() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(&cfg, "[otel]\nlog_user_prompt = false\n");

        let result = detect(HarnessId::CodexCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::Off);
    }

    #[test]
    fn codex_telemetry_unknown_when_toml_malformed() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(&cfg, "{ not = toml [unclosed");

        let result = detect(HarnessId::CodexCli, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::Unknown);
    }

    #[test]
    fn codex_cli_not_detected_when_only_shared_config_present_without_binary() {
        // ~/.codex/config.toml exists but `codex` is not on PATH and
        // /Applications/Codex.app is absent. The config file alone is
        // ambiguous (it could belong to either harness), so detection
        // must NOT fire.
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(&cfg, "[user]\nname = \"a\"\n");

        let result = detect(HarnessId::CodexCli, &detector_for(home.path()));
        assert!(!result.detected, "codex-cli should require `codex` on PATH");
        assert_eq!(result.detection_method, None);
        // The shared config file is still surfaced (so the dashboard
        // and the adapter can read telemetry status from it).
        assert_eq!(result.config_path.as_deref(), Some(cfg.as_path()));
    }

    #[test]
    fn codex_desktop_not_detected_when_only_shared_config_present_without_bundle() {
        // Same ambiguity from the other side: config.toml exists, but
        // /Applications/Codex.app is absent in the tempdir app_root.
        // codex-desktop must not falsely fire on the CLI's config file.
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(&cfg, "[user]\nname = \"a\"\n");

        let result = detect(HarnessId::CodexDesktop, &detector_for(home.path()));
        assert!(!result.detected, "codex-desktop should require Codex.app");
        assert_eq!(result.detection_method, None);
        assert_eq!(result.config_path.as_deref(), Some(cfg.as_path()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn codex_desktop_detected_via_app_bundle_even_when_config_also_present() {
        // When both signals are present, app-bundle is the authoritative
        // signal for codex-desktop (config.toml is shared).
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(&cfg, "[user]\nname = \"a\"\n");
        let app_root = tempdir().unwrap();
        fs::create_dir_all(app_root.path().join("Codex.app")).unwrap();

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(Vec::new()),
            app_root: app_root.path().to_path_buf(),
        };

        let result = detect(HarnessId::CodexDesktop, &detector);
        assert!(result.detected);
        assert_eq!(result.detection_method, Some(DetectionMethod::AppBundle));
    }

    #[test]
    fn codex_cli_detected_via_path_binary_even_when_config_also_present() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".codex").join("config.toml");
        write_settings(&cfg, "[user]\nname = \"a\"\n");

        let bin_dir = tempdir().unwrap();
        let exe_name = if cfg!(windows) { "codex.exe" } else { "codex" };
        let exe = bin_dir.path().join(exe_name);
        fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(vec![bin_dir.path().to_path_buf()]),
            app_root: home.path().to_path_buf(),
        };

        let result = detect(HarnessId::CodexCli, &detector);
        assert!(result.detected);
        assert_eq!(result.detection_method, Some(DetectionMethod::PathBinary));
    }

    #[test]
    fn trove_region_present_false_for_user_only_settings() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".claude").join("settings.json");
        write_settings(&cfg, r#"{"theme":"dark"}"#);
        let result = detect(HarnessId::ClaudeCode, &detector_for(home.path()));
        assert!(!result.trove_region_present);
    }

    #[test]
    fn trove_region_present_true_after_trove_writes_block() {
        // Stub a settings.json that already carries the _trove sentinel
        // a real apply would write. extract_region should see it.
        let home = tempdir().unwrap();
        let cfg = home.path().join(".claude").join("settings.json");
        write_settings(
            &cfg,
            r#"{
              "_trove": {
                "managed_keys": ["env.OTEL_FOO"],
                "hash": "deadbeef"
              },
              "env": {"OTEL_FOO": "bar"}
            }"#,
        );
        let result = detect(HarnessId::ClaudeCode, &detector_for(home.path()));
        assert!(result.trove_region_present);
    }

    #[test]
    fn tier_three_harnesses_remain_undetected_until_their_sprint() {
        // Sprint 9 lands the Tier 3 trio (cline, aider, copilot-cli);
        // until then, an empty home should report them as undetected.
        let home = tempdir().unwrap();
        for id in [HarnessId::Cline, HarnessId::Aider, HarnessId::CopilotCli] {
            let result = detect(id, &detector_for(home.path()));
            assert!(
                !result.detected,
                "should not detect {id:?} on a clean home dir",
            );
        }
    }

    // --- OpenCode detection ------------------------------------------------

    #[test]
    fn opencode_detected_via_opencode_json() {
        let home = tempdir().unwrap();
        let cfg = home
            .path()
            .join(".config")
            .join("opencode")
            .join("opencode.json");
        write_settings(&cfg, "{}");

        let result = detect(HarnessId::Opencode, &detector_for(home.path()));
        assert!(result.detected);
        assert_eq!(result.config_path.as_deref(), Some(cfg.as_path()));
        assert_eq!(result.detection_method, Some(DetectionMethod::ConfigDir));
        assert_eq!(result.telemetry, TelemetryStatus::Off);
    }

    #[test]
    fn opencode_telemetry_on_when_otel_plugin_registered() {
        let home = tempdir().unwrap();
        let cfg = home
            .path()
            .join(".config")
            .join("opencode")
            .join("opencode.json");
        write_settings(
            &cfg,
            r#"{"plugin":["@devtheops/opencode-plugin-otel","@some/other"]}"#,
        );

        let result = detect(HarnessId::Opencode, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn opencode_telemetry_on_when_trove_region_present() {
        let home = tempdir().unwrap();
        let cfg = home
            .path()
            .join(".config")
            .join("opencode")
            .join("opencode.json");
        write_settings(
            &cfg,
            r#"{
              "_trove": { "managed_keys": ["plugin"], "hash": "deadbeef" },
              "plugin": ["@devtheops/opencode-plugin-otel"]
            }"#,
        );

        let result = detect(HarnessId::Opencode, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
        assert!(result.trove_region_present);
    }

    #[test]
    fn opencode_telemetry_off_when_unrelated_plugins_only() {
        let home = tempdir().unwrap();
        let cfg = home
            .path()
            .join(".config")
            .join("opencode")
            .join("opencode.json");
        write_settings(&cfg, r#"{"plugin":["@some/other"]}"#);

        let result = detect(HarnessId::Opencode, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::Off);
    }

    #[test]
    fn opencode_telemetry_unknown_when_config_malformed() {
        let home = tempdir().unwrap();
        let cfg = home
            .path()
            .join(".config")
            .join("opencode")
            .join("opencode.json");
        write_settings(&cfg, "{not valid json");

        let result = detect(HarnessId::Opencode, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::Unknown);
    }

    #[test]
    fn opencode_detected_via_path_when_no_config() {
        let home = tempdir().unwrap();
        let bin_dir = tempdir().unwrap();
        let exe_name = if cfg!(windows) { "opencode.exe" } else { "opencode" };
        let exe = bin_dir.path().join(exe_name);
        fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(vec![bin_dir.path().to_path_buf()]),
            app_root: home.path().to_path_buf(),
        };

        let result = detect(HarnessId::Opencode, &detector);
        assert!(result.detected);
        assert_eq!(result.detection_method, Some(DetectionMethod::PathBinary));
    }

    // --- Cursor IDE / Cursor CLI detection ---------------------------------

    #[test]
    fn cursor_ide_detected_via_hooks_json() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".cursor").join("hooks.json");
        write_settings(&cfg, r#"{"version":1}"#);

        let result = detect(HarnessId::CursorIde, &detector_for(home.path()));
        assert!(result.detected);
        assert_eq!(result.config_path.as_deref(), Some(cfg.as_path()));
        assert_eq!(result.detection_method, Some(DetectionMethod::ConfigDir));
        assert_eq!(result.telemetry, TelemetryStatus::Off);
        assert!(!result.trove_region_present);
    }

    #[test]
    fn cursor_ide_detected_via_dot_cursor_directory_when_hooks_json_missing() {
        let home = tempdir().unwrap();
        // Create the .cursor dir but no hooks.json.
        fs::create_dir_all(home.path().join(".cursor")).unwrap();

        let result = detect(HarnessId::CursorIde, &detector_for(home.path()));
        assert!(result.detected);
        assert_eq!(
            result.config_path.as_deref(),
            Some(home.path().join(".cursor").as_path())
        );
        assert_eq!(result.detection_method, Some(DetectionMethod::ConfigDir));
        // Telemetry status read against a directory falls back to Unknown.
        assert_eq!(result.telemetry, TelemetryStatus::Unknown);
    }

    #[test]
    fn cursor_telemetry_on_when_trove_hook_block_present() {
        let home = tempdir().unwrap();
        let cfg = home.path().join(".cursor").join("hooks.json");
        write_settings(
            &cfg,
            r#"{
              "_trove": { "managed_keys": ["version", "hooks.beforeShellExecution"], "hash": "abc" },
              "version": 1,
              "hooks": { "beforeShellExecution": [] }
            }"#,
        );

        let result = detect(HarnessId::CursorIde, &detector_for(home.path()));
        assert_eq!(result.telemetry, TelemetryStatus::On);
        assert!(result.trove_region_present);
    }

    #[test]
    fn cursor_cli_detected_via_path_when_no_config() {
        let home = tempdir().unwrap();
        let bin_dir = tempdir().unwrap();
        let exe_name = if cfg!(windows) {
            "cursor-agent.exe"
        } else {
            "cursor-agent"
        };
        let exe = bin_dir.path().join(exe_name);
        fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(vec![bin_dir.path().to_path_buf()]),
            app_root: home.path().to_path_buf(),
        };

        let result = detect(HarnessId::CursorCli, &detector);
        assert!(result.detected);
        assert_eq!(result.detection_method, Some(DetectionMethod::PathBinary));
    }

    #[test]
    fn cursor_ide_not_detected_via_cursor_agent_path_binary() {
        // Sanity: only Cursor CLI probes for `cursor-agent` on PATH.
        // The IDE has no canonical PATH binary.
        let home = tempdir().unwrap();
        let bin_dir = tempdir().unwrap();
        let exe = bin_dir.path().join("cursor-agent");
        fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(vec![bin_dir.path().to_path_buf()]),
            app_root: home.path().to_path_buf(),
        };

        let result = detect(HarnessId::CursorIde, &detector);
        assert!(
            !result.detected,
            "cursor-agent on PATH must not falsely detect Cursor IDE"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cursor_ide_detected_via_app_bundle() {
        let home = tempdir().unwrap();
        let app_root = tempdir().unwrap();
        fs::create_dir_all(app_root.path().join("Cursor.app")).unwrap();

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(Vec::new()),
            app_root: app_root.path().to_path_buf(),
        };

        let result = detect(HarnessId::CursorIde, &detector);
        assert!(result.detected);
        assert_eq!(result.detection_method, Some(DetectionMethod::AppBundle));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn claude_code_detected_via_app_bundle() {
        let home = tempdir().unwrap();
        let app_root = tempdir().unwrap();
        fs::create_dir_all(app_root.path().join("Claude.app")).unwrap();

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(Vec::new()),
            app_root: app_root.path().to_path_buf(),
        };

        let result = detect(HarnessId::ClaudeCode, &detector);
        assert!(result.detected);
        assert_eq!(result.detection_method, Some(DetectionMethod::AppBundle));
    }

    // --- Claude Desktop detection -------------------------------------------

    #[cfg(target_os = "macos")]
    #[test]
    fn claude_desktop_detected_via_app_bundle() {
        let home = tempdir().unwrap();
        let app_root = tempdir().unwrap();
        fs::create_dir_all(app_root.path().join("Claude.app")).unwrap();

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(Vec::new()),
            app_root: app_root.path().to_path_buf(),
        };

        let result = detect(HarnessId::ClaudeDesktop, &detector);
        assert!(result.detected);
        assert_eq!(result.detection_method, Some(DetectionMethod::AppBundle));
        // Local detection doesn't know whether the audit-log tap is
        // emitting yet, so we leave telemetry at Unknown here. The
        // IPC overlay (live counter + sticky observation) flips it to
        // On once the first Cowork turn lands.
        assert_eq!(result.telemetry, TelemetryStatus::Unknown);
        assert!(result.config_path.is_none());
        // Claude Desktop is now adapter-backed via the audit-log tap
        // (see `adapters::claude_desktop`). The Harnesses tab uses
        // this to show an Enable/Disable toggle just like every other
        // detected harness.
        assert!(result.adapter_available);
    }

    #[test]
    fn claude_desktop_not_detected_when_nothing_is_installed() {
        let home = tempdir().unwrap();
        let result = detect(HarnessId::ClaudeDesktop, &detector_for(home.path()));
        assert!(!result.detected);
        assert!(result.config_path.is_none());
        // adapter_available reflects the *adapter*, not detection:
        // even if Claude isn't installed on this machine, the
        // adapter still exists. (Other harnesses follow the same
        // convention.)
        assert!(result.adapter_available);
    }
}
