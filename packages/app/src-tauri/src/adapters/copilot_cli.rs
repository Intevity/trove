//! GitHub Copilot CLI adapter — Tier 3 best-effort. Sprint 9 PR 3.
//!
//! `gh copilot` has no native OTLP. Trove installs a Trove-managed
//! block in the user's primary shell rc that defines a shell function
//! named `gh-copilot` (with a hyphen, not a space) which execs the
//! bundled `trove-copilot` wrapper. The wrapper runs `gh copilot` and
//! appends one JSON-line per invocation to
//! `~/.local/state/trove/copilot.log`. A [`crate::log_watcher`] tail
//! emits one OTLP `LogRecord` per line.
//!
//! ## Why a `gh-copilot` rename and not a `gh` shadow
//!
//! Shadowing `gh` itself routes every gh subcommand through Trove's
//! wrapper — fine for observability of `gh copilot` but invasive for
//! `gh pr`, `gh repo`, etc. (and a UX hazard if our wrapper has a
//! bug). Renaming Copilot's invocation to `gh-copilot` keeps the
//! blast radius scoped to the one subcommand we're observing. The UI
//! surfaces this rename in the row's coverage-note tooltip.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ipc::IpcError;

use super::wrapper_common::{self, WrapperSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

/// `gh-copilot` shell function name (with a hyphen). Users invoke
/// `gh-copilot suggest "..."` instead of `gh copilot suggest "..."`
/// while Trove is observing.
pub const FUNCTION_NAME: &str = "gh-copilot";

/// Subdirectory under the user's state dir where the wrapper writes
/// its JSON-line log.
pub const LOG_RELATIVE_PATH: &[&str] = &[".local", "state", "trove", "copilot.log"];

#[must_use]
pub fn log_path(home: &Path) -> PathBuf {
    let mut p = home.to_path_buf();
    for seg in LOG_RELATIVE_PATH {
        p.push(seg);
    }
    p
}

#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    wrapper_common::primary_shell_rc(home).unwrap_or_else(|| home.join(".zshrc"))
}

#[must_use]
pub fn spec(wrapper_path: PathBuf) -> WrapperSpec {
    WrapperSpec {
        function_name: FUNCTION_NAME,
        wrapper_path,
        label: "trove::copilot-cli",
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

/// Parse one JSON-line emitted by the bundled `trove-copilot` wrapper
/// into an OTLP/HTTP/JSON `LogRecord`-shaped payload. Returns `None`
/// when the line isn't parseable or doesn't carry the expected `tool`
/// field.
#[must_use]
pub fn parse_event_line(line: &str, opts: &ApplyOptions) -> Option<Value> {
    let event: Value = serde_json::from_str(line.trim()).ok()?;
    let tool = event.get("tool").and_then(Value::as_str)?;
    if tool != "copilot-cli" {
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
        json!({"key": "trove.source", "value": {"stringValue": "copilot-cli"}}),
        json!({"key": "copilot.argc", "value": {"intValue": argc.to_string()}}),
        json!({"key": "copilot.exit_code", "value": {"intValue": exit.to_string()}}),
        json!({"key": "copilot.duration_ms", "value": {"intValue": duration.to_string()}}),
        json!({"key": "copilot.ts", "value": {"stringValue": ts}}),
    ];
    for (k, v) in &opts.custom_attributes {
        attributes.push(json!({"key": k, "value": {"stringValue": v}}));
    }

    Some(json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "copilot-cli"}},
                    {"key": "harness.id", "value": {"stringValue": "copilot-cli"}},
                    {"key": "harness.name", "value": {"stringValue": "GitHub Copilot CLI"}},
                    {"key": "trove.source", "value": {"stringValue": "copilot-cli"}},
                ]
            },
            "scopeLogs": [{
                "scope": {"name": "trove.adapters.copilot_cli", "version": env!("CARGO_PKG_VERSION")},
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
/// invocation it represents. Same shape as the Aider variant
/// — see [`crate::adapters::aider::parse_event_metric_payload`].
/// Copilot CLI exposes no tokenizer or rate-table either, so token and
/// cost metrics are intentionally absent.
#[must_use]
pub fn parse_event_metric_payload(line: &str, opts: &ApplyOptions) -> Option<Value> {
    super::wrapper_common_metrics::build_invocation_metrics(
        line,
        opts,
        &super::wrapper_common_metrics::WrapperMetricsSpec {
            expected_tool: "copilot-cli",
            service_name: "copilot-cli",
            harness_id: "copilot-cli",
            harness_name: "GitHub Copilot CLI",
            scope_name: "trove.adapters.copilot_cli",
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn function_name_is_gh_copilot_with_hyphen() {
        assert_eq!(FUNCTION_NAME, "gh-copilot");
    }

    #[test]
    fn log_path_resolves_under_state_dir() {
        let home = PathBuf::from("/home/dev");
        let p = log_path(&home);
        assert_eq!(p, PathBuf::from("/home/dev/.local/state/trove/copilot.log"));
    }

    #[test]
    fn parse_event_returns_none_for_unrelated_tool() {
        let line = r#"{"tool":"aider","argc":1,"exit_code":0}"#;
        assert!(parse_event_line(line, &ApplyOptions::default()).is_none());
    }

    #[test]
    fn parse_event_emits_canonical_otlp_shape_with_copilot_attributes() {
        let line = r#"{"ts":"2026-05-09T15:00:00Z","tool":"copilot-cli","argc":2,"exit_code":0,"duration_ms":42}"#;
        let payload = parse_event_line(line, &ApplyOptions::default()).unwrap();
        let attrs = payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
            .as_array()
            .unwrap();
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
        assert_eq!(by_key("trove.source"), "copilot-cli");
        assert_eq!(by_key("copilot.argc"), "2");
        assert_eq!(by_key("copilot.duration_ms"), "42");
    }
}
