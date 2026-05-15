//! Aider adapter — Tier 3 best-effort. Sprint 9 PR 3.
//!
//! Aider has no native OTLP. Trove installs a Trove-managed block in
//! the user's primary shell rc that defines a shell function named
//! `aider` which execs the bundled `trove-aider` wrapper. The wrapper
//! runs the real aider binary and appends one JSON-line per
//! invocation to `~/.local/state/trove/aider.log`. A
//! [`crate::log_watcher`] tail-watcher on that file emits one OTLP
//! `LogRecord` per line.
//!
//! The adapter delegates host-file work to [`super::wrapper_common`];
//! this module only owns the per-harness `WrapperSpec` plus the OTLP
//! parser that turns one wrapper log line into one `LogRecord`-shaped
//! `serde_json::Value`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ipc::IpcError;

use super::wrapper_common::{self, WrapperSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

/// `aider` shell function name. The user invokes `aider <args>`
/// after `source`-ing the rc file; the function execs `trove-aider`
/// which delegates to the real `aider` binary.
pub const FUNCTION_NAME: &str = "aider";

/// Subdirectory under the user's state dir where the wrapper writes
/// its JSON-line log.
pub const LOG_RELATIVE_PATH: &[&str] = &[".local", "state", "trove", "aider.log"];

/// Resolve the absolute path of the wrapper's log file under `home`.
#[must_use]
pub fn log_path(home: &Path) -> PathBuf {
    let mut p = home.to_path_buf();
    for seg in LOG_RELATIVE_PATH {
        p.push(seg);
    }
    p
}

/// Resolve the path of the host file Trove writes its managed block
/// into. Returns the primary shell rc if one exists; otherwise a
/// best-guess `~/.zshrc` path that the UI surfaces in the preview as
/// the *intended* target.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    wrapper_common::primary_shell_rc(home).unwrap_or_else(|| home.join(".zshrc"))
}

/// Build the [`WrapperSpec`] for an apply with `wrapper_path`.
#[must_use]
pub fn spec(wrapper_path: PathBuf) -> WrapperSpec {
    WrapperSpec {
        function_name: FUNCTION_NAME,
        wrapper_path,
        label: "trove::aider",
    }
}

pub fn preview(
    home: &Path,
    opts: &ApplyOptions,
    wrapper_path: &Path,
) -> Result<PatchPreview, IpcError> {
    wrapper_common::preview_for_primary_shell_rc(home, &spec(wrapper_path.to_path_buf()), opts)
}

pub fn apply(
    home: &Path,
    opts: &ApplyOptions,
    wrapper_path: &Path,
) -> Result<TrovePatch, IpcError> {
    wrapper_common::apply_to_primary_shell_rc(home, &spec(wrapper_path.to_path_buf()), opts)
}

pub fn revert(home: &Path) -> Result<(), IpcError> {
    wrapper_common::revert_primary_shell_rc(home)
}

/// Parse one JSON-line emitted by the bundled `trove-aider` wrapper
/// into an OTLP/HTTP/JSON `LogRecord`-shaped payload. Returns `None`
/// when the line isn't parseable JSON or doesn't carry the expected
/// `tool` field.
#[must_use]
pub fn parse_event_line(line: &str, opts: &ApplyOptions) -> Option<Value> {
    let event: Value = serde_json::from_str(line.trim()).ok()?;
    let tool = event.get("tool").and_then(Value::as_str)?;
    if tool != "aider" {
        return None;
    }

    let argc = event.get("argc").and_then(Value::as_i64).unwrap_or(0);
    let exit = event.get("exit_code").and_then(Value::as_i64).unwrap_or(0);
    let duration = event.get("duration_ms").and_then(Value::as_i64).unwrap_or(0);
    let ts = event.get("ts").and_then(Value::as_str).unwrap_or("");
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());

    let mut attributes = vec![
        json!({"key": "trove.source", "value": {"stringValue": "aider"}}),
        json!({"key": "aider.argc", "value": {"intValue": argc.to_string()}}),
        json!({"key": "aider.exit_code", "value": {"intValue": exit.to_string()}}),
        json!({"key": "aider.duration_ms", "value": {"intValue": duration.to_string()}}),
        json!({"key": "aider.ts", "value": {"stringValue": ts}}),
    ];
    for (k, v) in &opts.custom_attributes {
        attributes.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    Some(json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "aider"}},
                    {"key": "harness.id", "value": {"stringValue": "aider"}},
                    {"key": "harness.name", "value": {"stringValue": "Aider"}},
                    {"key": "trove.source", "value": {"stringValue": "aider"}},
                ]
            },
            "scopeLogs": [{
                "scope": {"name": "trove.adapters.aider", "version": env!("CARGO_PKG_VERSION")},
                "logRecords": [{
                    "timeUnixNano": now_ns.to_string(),
                    "severityNumber": 9,
                    "severityText": "INFO",
                    "body": {"stringValue": ""},
                    "attributes": attributes,
                }]
            }]
        }]
    }))
}

/// Parse one wrapper line into a Tier A metric payload covering the
/// invocation it represents:
///
/// - `trove.harness.events` with `event.kind = chat.turn`, count = 1.
/// - `trove.harness.turn.duration` (histogram) with one observation at
///   `duration_ms / 1000` seconds.
/// - `trove.harness.errors` with `error.kind = unknown`, count = 1, when
///   the wrapper reports a non-zero `exit_code`.
///
/// Token and cost are intentionally absent — the wrapper has no
/// visibility into Aider's tokenizer or upstream rate table, so
/// fabricating those numbers would be worse than skipping.
#[must_use]
pub fn parse_event_metric_payload(
    line: &str,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
) -> Option<Value> {
    super::wrapper_common_metrics::build_invocation_metrics(
        line,
        opts,
        &super::wrapper_common_metrics::WrapperMetricsSpec {
            expected_tool: "aider",
            service_name: "aider",
            harness: crate::harness::HarnessId::Aider,
            harness_id: "aider",
            harness_name: "Aider",
            scope_name: "trove.adapters.aider",
        },
        mappings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn function_name_is_aider() {
        assert_eq!(FUNCTION_NAME, "aider");
    }

    #[test]
    fn log_path_resolves_under_state_dir() {
        let home = PathBuf::from("/home/dev");
        let p = log_path(&home);
        assert_eq!(p, PathBuf::from("/home/dev/.local/state/trove/aider.log"));
    }

    #[test]
    fn config_path_falls_back_to_zshrc_when_no_rc_exists() {
        let dir = tempdir().unwrap();
        let p = config_path(dir.path());
        assert_eq!(p, dir.path().join(".zshrc"));
    }

    #[test]
    fn parse_event_returns_none_for_non_json() {
        assert!(parse_event_line("not json", &ApplyOptions::default()).is_none());
    }

    #[test]
    fn parse_event_returns_none_for_unrelated_tool() {
        let line = r#"{"tool":"notaider","argc":1,"exit_code":0,"duration_ms":50}"#;
        assert!(parse_event_line(line, &ApplyOptions::default()).is_none());
    }

    #[test]
    fn parse_event_emits_canonical_otlp_log_record_shape() {
        let line = r#"{"ts":"2026-05-09T15:00:00Z","tool":"aider","argc":3,"exit_code":0,"duration_ms":1234}"#;
        let payload = parse_event_line(line, &ApplyOptions::default()).unwrap();
        let log = &payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        let attrs = log["attributes"].as_array().unwrap();
        let by_key = |k: &str| {
            attrs
                .iter()
                .find(|a| a["key"] == k)
                .map(|a| {
                    a["value"]["stringValue"]
                        .as_str()
                        .or_else(|| a["value"]["intValue"].as_str())
                        .unwrap()
                        .to_string()
                })
                .unwrap()
        };
        assert_eq!(by_key("trove.source"), "aider");
        assert_eq!(by_key("aider.argc"), "3");
        assert_eq!(by_key("aider.exit_code"), "0");
        assert_eq!(by_key("aider.duration_ms"), "1234");
        assert_eq!(by_key("aider.ts"), "2026-05-09T15:00:00Z");
    }

    #[test]
    fn parse_event_attaches_custom_attributes() {
        let line = r#"{"tool":"aider","argc":1,"exit_code":0,"duration_ms":5,"ts":""}"#;
        let mut opts = ApplyOptions::default();
        opts.custom_attributes.insert("team".into(), "platform".into());
        let payload = parse_event_line(line, &opts).unwrap();
        let attrs = payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
            .as_array()
            .unwrap();
        assert!(attrs
            .iter()
            .any(|a| a["key"] == "team" && a["value"]["stringValue"] == "platform"));
    }
}
