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

    let mut detection_method = config_path.as_ref().map(|_| DetectionMethod::ConfigDir);

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
    }
}

fn path_binary_name(id: HarnessId) -> Option<&'static str> {
    match id {
        HarnessId::ClaudeCode => Some("claude"),
        HarnessId::GeminiCli => Some("gemini"),
        HarnessId::CodexCli => Some("codex"),
        HarnessId::QwenCode => Some("qwen"),
        _ => None,
    }
}

/// Whether `path` contains a Trove-managed region. Returns false on
/// missing or unparseable files (the dashboard surfaces the parse
/// failure separately via the telemetry-status `Unknown` and the
/// preview/apply error path).
fn read_trove_region_present(id: HarnessId, path: &Path) -> bool {
    let format = match id {
        HarnessId::ClaudeCode | HarnessId::GeminiCli | HarnessId::QwenCode => Format::Json,
        HarnessId::CodexCli => Format::Toml,
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
        HarnessId::CodexCli => check_codex_telemetry(&text),
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
    fn tier_two_harnesses_never_detected_in_sprint_3() {
        let home = tempdir().unwrap();
        for id in [
            HarnessId::Opencode,
            HarnessId::CursorIde,
            HarnessId::CursorCli,
            HarnessId::Cline,
            HarnessId::Aider,
            HarnessId::CopilotCli,
        ] {
            let result = detect(id, &detector_for(home.path()));
            assert!(
                !result.detected,
                "Tier 2/3 should not detect in Sprint 3: {id:?}"
            );
        }
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
}
