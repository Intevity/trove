//! Harness detection sweep — scans the user's machine for installed
//! Tier 1 harnesses and reports their config paths plus a parsed
//! telemetry status. Sprint 3's IPC layer surfaces the result to the
//! React UI; later sprints add Tier 2 / Tier 3 detection by extending
//! [`crate::harness::HarnessId::tier_1`] / friends.

mod harnesses;
mod paths;
mod probe;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::harness::HarnessId;

/// Where Trove found evidence of a given harness installation. Listed
/// in preference order — `ConfigDir` wins over `PathBinary` wins over
/// `AppBundle` when multiple signals fire (config-dir means the harness
/// has actually been used at least once, which is the strongest proof).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetectionMethod {
    PathBinary,
    ConfigDir,
    AppBundle,
}

/// Whether the host config currently emits telemetry. Tri-state because
/// malformed or missing files can't honestly answer; the dashboard maps
/// `Unknown` to a neutral icon ("we couldn't read this config").
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TelemetryStatus {
    On,
    Off,
    Unknown,
}

/// One row in the dashboard's "Detected harnesses" list.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedHarness {
    pub id: HarnessId,
    pub detected: bool,
    pub config_path: Option<PathBuf>,
    pub telemetry: TelemetryStatus,
    pub detection_method: Option<DetectionMethod>,
    /// Whether the host config currently contains a Trove-managed
    /// region. The dashboard uses this to decide whether the toggle
    /// shows "Enable" (no region — apply will write one) or "Disable"
    /// (region present — revert will remove it). Distinct from
    /// `telemetry`, which can be `On` even when Trove didn't write it.
    pub trove_region_present: bool,
}

/// Scoping struct that controls where the detector looks. Production
/// code constructs one via [`Detector::from_environment`]; tests build
/// one with a `tempdir`-scoped home and an explicit PATH list so
/// detection stays hermetic.
#[derive(Clone, Debug)]
pub struct Detector {
    pub home: PathBuf,
    /// `Some(dirs)` overrides the process `$PATH`; `None` uses the env.
    pub path_dirs: Option<Vec<PathBuf>>,
    /// Root for macOS app-bundle scanning (defaults to `/Applications`).
    pub app_root: PathBuf,
}

impl Detector {
    /// Build a detector that reads from the real environment.
    /// Used by the Tauri IPC entry point.
    #[must_use]
    pub fn from_environment() -> Self {
        Self {
            home: dirs::home_dir().unwrap_or_default(),
            path_dirs: None,
            app_root: paths::default_app_root(),
        }
    }

    /// Detect every Tier 1 harness Trove knows about. Sprint 7 / Sprint 9
    /// will extend the iteration to Tier 2 and Tier 3 by adding entries
    /// to [`HarnessId::tier_1`] (or sister helpers) and to the
    /// per-harness path/probe tables.
    #[must_use]
    pub fn detect_all(&self) -> Vec<DetectedHarness> {
        HarnessId::tier_1()
            .iter()
            .map(|id| harnesses::detect(*id, self))
            .collect()
    }
}

/// Convenience entry point used by the Tauri IPC command. Detects every
/// Tier 1 harness against the real environment.
#[must_use]
pub fn detect_all() -> Vec<DetectedHarness> {
    Detector::from_environment().detect_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detect_all_returns_one_row_per_tier_1_harness() {
        let home = tempdir().unwrap();
        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(Vec::new()),
            app_root: home.path().to_path_buf(),
        };
        let results = detector.detect_all();
        assert_eq!(results.len(), HarnessId::tier_1().len());

        let returned_ids: Vec<HarnessId> = results.iter().map(|r| r.id).collect();
        assert_eq!(returned_ids, HarnessId::tier_1().to_vec());
    }

    #[test]
    fn detect_all_marks_present_harnesses_as_detected() {
        let home = tempdir().unwrap();
        // Lay down a claude config and a gemini config; codex/qwen
        // remain absent.
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(home.path().join(".claude").join("settings.json"), "{}").unwrap();
        fs::create_dir_all(home.path().join(".gemini")).unwrap();
        fs::write(home.path().join(".gemini").join("settings.json"), "{}").unwrap();

        let detector = Detector {
            home: home.path().to_path_buf(),
            path_dirs: Some(Vec::new()),
            app_root: home.path().to_path_buf(),
        };

        let results = detector.detect_all();
        let by_id: std::collections::HashMap<HarnessId, &DetectedHarness> =
            results.iter().map(|r| (r.id, r)).collect();
        assert!(by_id[&HarnessId::ClaudeCode].detected);
        assert!(by_id[&HarnessId::GeminiCli].detected);
        assert!(!by_id[&HarnessId::CodexCli].detected);
        assert!(!by_id[&HarnessId::QwenCode].detected);
    }

    #[test]
    fn detected_harness_serializes_camel_case() {
        let h = DetectedHarness {
            id: HarnessId::ClaudeCode,
            detected: true,
            config_path: Some(PathBuf::from("/tmp/x")),
            telemetry: TelemetryStatus::On,
            detection_method: Some(DetectionMethod::ConfigDir),
            trove_region_present: false,
        };
        let json = serde_json::to_string(&h).unwrap();
        // The TS-side Zod schema expects camelCase keys; check the
        // load-bearing ones that don't match Rust's snake_case.
        assert!(json.contains("\"configPath\""));
        assert!(json.contains("\"detectionMethod\""));
        assert!(json.contains("\"troveRegionPresent\""));
        assert!(!json.contains("\"config_path\""));
        assert!(!json.contains("\"trove_region_present\""));
    }

    #[test]
    fn telemetry_status_serializes_kebab_case() {
        assert_eq!(
            serde_json::to_string(&TelemetryStatus::Unknown).unwrap(),
            "\"unknown\""
        );
    }

    #[test]
    fn detection_method_serializes_kebab_case() {
        assert_eq!(
            serde_json::to_string(&DetectionMethod::PathBinary).unwrap(),
            "\"path-binary\""
        );
        assert_eq!(
            serde_json::to_string(&DetectionMethod::ConfigDir).unwrap(),
            "\"config-dir\""
        );
        assert_eq!(
            serde_json::to_string(&DetectionMethod::AppBundle).unwrap(),
            "\"app-bundle\""
        );
    }
}
