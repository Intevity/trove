//! GitHub Copilot CLI adapter — Tier 3 best-effort. Sprint 9 PR 3.
//!
//! GitHub Copilot CLI has no native OTLP. Trove installs a Trove-managed
//! block in the user's primary shell rc that defines two shell functions
//! — `copilot` (the new standalone CLI; binary name `copilot`) and
//! `gh-copilot` (the deprecated `gh copilot` gh-extension, sunset
//! 2025-09-25 but still in widespread use through ~2026). Both
//! functions exec the bundled `trove-copilot` wrapper, which prefers
//! the new standalone binary and falls back to `gh copilot` only when
//! the new one isn't on PATH. The wrapper appends one JSON-line per
//! invocation to `~/.local/state/trove/copilot.log`; a
//! [`crate::log_watcher`] tail emits one OTLP `LogRecord` per line.
//!
//! ## Why both function names
//!
//! Users on a clean install of the new standalone CLI invoke
//! `copilot suggest "..."` and never `gh copilot`. Users still on the
//! deprecated gh-extension invoke `gh-copilot suggest "..."`. The
//! adapter covers both by defining a shell function for each name;
//! once a user migrates to the standalone CLI they keep observability
//! seamlessly. Shadowing `gh` itself would route every gh subcommand
//! through Trove's wrapper — invasive for `gh pr`, `gh repo`, etc. and
//! a UX hazard if our wrapper has a bug. Renaming the deprecated path
//! to `gh-copilot` keeps the blast radius scoped to the one subcommand
//! we're observing.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ipc::IpcError;

use super::wrapper_common::{self, WrapperSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

/// Shell function names the adapter installs. `copilot` covers the new
/// standalone CLI (`github/copilot-cli`); `gh-copilot` covers the
/// deprecated `gh copilot` gh-extension (sunset 2025-09-25 but still
/// widespread). Both functions exec the same wrapper script, which
/// forwards to whichever real binary is on PATH (prefers the new one).
pub const FUNCTION_NAMES: &[&str] = &["copilot", "gh-copilot"];

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
        function_names: FUNCTION_NAMES,
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
pub fn parse_event_metric_payload(
    line: &str,
    opts: &ApplyOptions,
    mappings: std::sync::Arc<crate::mappings::MappingState>,
) -> Option<Value> {
    super::wrapper_common_metrics::build_invocation_metrics(
        line,
        opts,
        &super::wrapper_common_metrics::WrapperMetricsSpec {
            expected_tool: "copilot-cli",
            service_name: "copilot-cli",
            harness: crate::harness::HarnessId::CopilotCli,
            harness_id: "copilot-cli",
            harness_name: "GitHub Copilot CLI",
            scope_name: "trove.adapters.copilot_cli",
        },
        mappings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn function_names_covers_both_copilot_and_gh_copilot() {
        // `copilot` (new standalone CLI) is listed first so the block
        // emits its definition before the deprecated `gh-copilot` alias.
        // Both must be present so neither path drops out of observability.
        assert_eq!(FUNCTION_NAMES, &["copilot", "gh-copilot"]);
    }

    #[test]
    fn wrapper_block_contains_both_function_names() {
        use crate::adapters::wrapper_common::build_managed_block;
        let s = spec(PathBuf::from("/tmp/trove-copilot"));
        let block = build_managed_block(&s, &ApplyOptions::default());
        assert!(block.contains("copilot() { \"/tmp/trove-copilot\" \"$@\"; }"));
        assert!(block.contains("gh-copilot() { \"/tmp/trove-copilot\" \"$@\"; }"));
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
