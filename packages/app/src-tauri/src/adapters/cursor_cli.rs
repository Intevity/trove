//! Cursor CLI adapter — Tier 3 wrapper, distinct from `cursor_ide`.
//!
//! ## Why a shell-function wrapper and not the hooks.json patch
//!
//! Cursor's hook system (`beforeShellExecution`, `afterShellExecution`,
//! `beforeSubmitPrompt`, `afterAgentResponse`) only fires inside the
//! Cursor IDE. The headless CLI `cursor-agent` does not invoke
//! `~/.cursor/hooks.json` on any of its commands, so enabling the
//! cursor-cli harness via the shared hooks.json patch produced **zero**
//! telemetry — verified during the v0.5 release pairing tests, see
//! `documentation/harness-platform-matrix.md`.
//!
//! This adapter now follows the aider / copilot-cli pattern: Trove
//! installs a managed block in the user's primary shell rc that defines
//! a shell function named `cursor-agent` which execs the bundled
//! `trove-cursor-agent` wrapper. The wrapper runs the real cursor-agent
//! and appends one JSON-line per invocation to
//! `~/.local/state/trove/cursor-agent.log`. A [`crate::log_watcher`]
//! tail emits one OTLP `LogRecord` + one Tier A metric payload per
//! line.
//!
//! Cursor IDE coverage is unaffected: [`super::cursor_ide`] still
//! patches `~/.cursor/hooks.json` via [`super::cursor_common`]. Both
//! harnesses can be enabled simultaneously without conflict — they
//! patch different host files.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ipc::IpcError;

use super::wrapper_common::{self, WrapperSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

/// `cursor-agent` shell function name. The user invokes
/// `cursor-agent <args>` exactly as before; the function defined in
/// the user's shell rc transparently routes through the wrapper.
pub const FUNCTION_NAMES: &[&str] = &["cursor-agent"];

/// Subdirectory under the user's state dir where the wrapper writes
/// its JSON-line log.
pub const LOG_RELATIVE_PATH: &[&str] = &[".local", "state", "trove", "cursor-agent.log"];

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
        adapter_id: "cursor-cli",
        function_names: FUNCTION_NAMES,
        wrapper_path,
        label: "trove::cursor-cli",
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
    wrapper_common::revert_primary_shell_rc(home, "cursor-cli", FUNCTION_NAMES)
}

/// Parse one JSON-line emitted by the bundled `trove-cursor-agent`
/// wrapper into an OTLP/HTTP/JSON `LogRecord`-shaped payload. Returns
/// `None` when the line isn't parseable or doesn't carry the expected
/// `tool` field.
#[must_use]
pub fn parse_event_line(line: &str, opts: &ApplyOptions) -> Option<Value> {
    let event: Value = serde_json::from_str(line.trim()).ok()?;
    let tool = event.get("tool").and_then(Value::as_str)?;
    if tool != "cursor-cli" {
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
        json!({"key": "trove.source", "value": {"stringValue": "cursor-cli"}}),
        json!({"key": "cursor.argc", "value": {"intValue": argc.to_string()}}),
        json!({"key": "cursor.exit_code", "value": {"intValue": exit.to_string()}}),
        json!({"key": "cursor.duration_ms", "value": {"intValue": duration.to_string()}}),
        json!({"key": "cursor.ts", "value": {"stringValue": ts}}),
    ];
    for (k, v) in &opts.custom_attributes {
        attributes.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    Some(json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "cursor-cli"}},
                    {"key": "harness.id", "value": {"stringValue": "cursor-cli"}},
                    {"key": "harness.name", "value": {"stringValue": "Cursor CLI"}},
                    {"key": "trove.source", "value": {"stringValue": "cursor-cli"}},
                ]
            },
            "scopeLogs": [{
                "scope": {"name": "trove.adapters.cursor_cli", "version": env!("CARGO_PKG_VERSION")},
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
/// invocation it represents. Same shape as the Aider / Copilot CLI
/// variants — see [`crate::adapters::aider::parse_event_metric_payload`].
/// Cursor CLI exposes no tokenizer or rate-table either, so token and
/// cost metrics are intentionally absent.
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
            expected_tool: "cursor-cli",
            service_name: "cursor-cli",
            harness: crate::harness::HarnessId::CursorCli,
            harness_id: "cursor-cli",
            harness_name: "Cursor CLI",
            scope_name: "trove.adapters.cursor_cli",
        },
        mappings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn function_names_is_just_cursor_agent() {
        assert_eq!(FUNCTION_NAMES, &["cursor-agent"]);
    }

    #[test]
    fn log_path_resolves_under_state_dir() {
        let home = PathBuf::from("/home/dev");
        let p = log_path(&home);
        assert_eq!(p, PathBuf::from("/home/dev/.local/state/trove/cursor-agent.log"));
    }

    #[test]
    fn parse_event_returns_none_for_unrelated_tool() {
        let line = r#"{"tool":"aider","argc":1,"exit_code":0}"#;
        assert!(parse_event_line(line, &ApplyOptions::default()).is_none());
        let line = r#"{"tool":"copilot-cli","argc":1,"exit_code":0}"#;
        assert!(parse_event_line(line, &ApplyOptions::default()).is_none());
    }

    #[test]
    fn parse_event_returns_none_for_garbage() {
        assert!(parse_event_line("not json", &ApplyOptions::default()).is_none());
        assert!(parse_event_line("", &ApplyOptions::default()).is_none());
        assert!(parse_event_line("{}", &ApplyOptions::default()).is_none());
    }

    #[test]
    fn parse_event_emits_canonical_otlp_shape_with_cursor_attributes() {
        let line = r#"{"ts":"2026-05-18T18:00:00Z","tool":"cursor-cli","argc":3,"exit_code":0,"duration_ms":17}"#;
        let payload = parse_event_line(line, &ApplyOptions::default()).unwrap();
        let resource_attrs = payload["resourceLogs"][0]["resource"]["attributes"]
            .as_array()
            .unwrap();
        let by_resource_key = |k: &str| {
            resource_attrs
                .iter()
                .find(|a| a["key"] == k)
                .map(|a| a["value"]["stringValue"].as_str().unwrap().to_string())
                .unwrap()
        };
        assert_eq!(by_resource_key("service.name"), "cursor-cli");
        assert_eq!(by_resource_key("harness.id"), "cursor-cli");
        assert_eq!(by_resource_key("harness.name"), "Cursor CLI");

        let log_attrs = payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
            .as_array()
            .unwrap();
        let by_key = |k: &str| {
            log_attrs
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
        assert_eq!(by_key("trove.source"), "cursor-cli");
        assert_eq!(by_key("cursor.argc"), "3");
        assert_eq!(by_key("cursor.duration_ms"), "17");
        assert_eq!(by_key("cursor.exit_code"), "0");
    }

    #[test]
    fn parse_event_includes_custom_attributes() {
        let line = r#"{"ts":"2026-05-18T18:00:00Z","tool":"cursor-cli","argc":1,"exit_code":0,"duration_ms":5}"#;
        let opts = ApplyOptions {
            custom_attributes: std::collections::BTreeMap::from([(
                "team".to_string(),
                "platform".to_string(),
            )]),
        };
        let payload = parse_event_line(line, &opts).unwrap();
        let attrs = payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
            .as_array()
            .unwrap();
        assert!(
            attrs.iter().any(|a| a["key"] == "team"
                && a["value"]["stringValue"] == "platform"),
            "expected custom attribute `team=platform`, got: {attrs:#?}",
        );
    }
}
