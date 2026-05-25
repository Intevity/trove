//! `test_export` — the wizard's "Test export" button.
//!
//! Sends a synthetic OTLP/HTTP traces payload to the supervised local
//! Collector, then tails its log to surface any export failure. Three
//! outcomes:
//!
//! - `ok` — the payload was accepted (HTTP 200) and the collector log
//!   showed no failure markers within a 5s budget. We treat that as
//!   "telemetry would land at your backend right now."
//! - `failed` — either the local Collector rejected the payload (a
//!   non-2xx response from the receiver) or the Collector log surfaced
//!   one of the otelcol exporter failure markers within the budget.
//! - `timeout` — the Collector log file is unreachable, or the request
//!   itself never returned. Distinguished from `failed` so the wizard
//!   can suggest "is the sidecar running?" rather than "credentials
//!   wrong."
//!
//! The synthetic payload is one trace with one span, name `test_export`,
//! kind `internal`, timestamps `now`. The trace and span IDs are
//! hard-coded canaries so the wizard's UI can mention them ("the test
//! span uses `trace_id` 0xtrove…").

use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::otlp_emit::{self, OtlpEmitError};

/// Return type — mirrors the Zod `TestExportResult` schema.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestExportResult {
    pub status: TestExportStatus,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestExportStatus {
    /// Synthetic payload accepted and no exporter failures observed.
    Ok,
    /// Either the receiver returned non-2xx or the log surfaced an
    /// exporter failure marker.
    Failed,
    /// The HTTP request never returned, or the log file could not be
    /// read at all.
    Timeout,
}

/// Substrings that signal an otelcol exporter failure in the log. Every
/// otelcol exporter funnels through `exporterhelper`, which logs one of
/// these wordings on a failed send. False positives are unlikely
/// because we scope the scan to the bytes written *after* our request
/// landed.
const FAILURE_MARKERS: &[&str] = &[
    "Permanent error",
    "error sending",
    "Failed to send",
    "export request failed",
    "connection refused",
    "no such host",
    "Unauthorized",
];

/// Total wall-clock budget for `test_export`.
pub const DEFAULT_TEST_BUDGET: Duration = Duration::from_secs(5);

/// Hard-coded synthetic trace identifier (32 hex chars, lowercase).
/// The constant makes the canary recognisable in any backend's UI.
pub const SYNTHETIC_TRACE_ID: &str = "74726f7665740000c0deca5e0d5705ed";
/// Hard-coded synthetic span identifier (16 hex chars, lowercase).
pub const SYNTHETIC_SPAN_ID: &str = "74726f7665747465";

/// Build the synthetic OTLP/HTTP/JSON traces payload. Pure function so
/// tests can snapshot the structure without touching the network.
#[must_use]
pub fn synthetic_payload() -> serde_json::Value {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let start_ns = now_ns.saturating_sub(1_000_000); // 1ms before "now"
    let end_ns = now_ns;

    serde_json::json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [
                    { "key": "service.name", "value": { "stringValue": "trove-test-export" } },
                    { "key": "trove.synthetic", "value": { "boolValue": true } }
                ]
            },
            "scopeSpans": [{
                "scope": { "name": "trove.test_export", "version": env!("CARGO_PKG_VERSION") },
                "spans": [{
                    "traceId": SYNTHETIC_TRACE_ID,
                    "spanId": SYNTHETIC_SPAN_ID,
                    "name": "test_export",
                    "kind": 1,
                    "startTimeUnixNano": start_ns.to_string(),
                    "endTimeUnixNano": end_ns.to_string(),
                    "attributes": [
                        { "key": "trove.test", "value": { "stringValue": "wizard-button" } }
                    ],
                    "status": { "code": 1 }
                }]
            }]
        }]
    })
}

/// Run a synthetic export against `endpoint` and watch `log_path` for
/// failure markers until `budget` elapses. Pure function: takes paths
/// and URLs, no `AppHandle`. The Tauri `#[command]` wrapper resolves
/// these from `app.path()` and calls in.
///
/// `exporter_filter` scopes the log scan to lines that mention the
/// given `OTel` component id substring (e.g. `otlphttp/sentry-3a0c9975`).
/// When `None`, every [`FAILURE_MARKERS`] hit triggers a `failed`
/// result — the original behavior, kept as a fallback for callers that
/// don't know which exporter is under test. With a filter, the scan
/// ignores failures from unrelated exporters (Bug-I fix from 2026-05-25).
pub async fn test_export_at(
    endpoint: &str,
    log_path: &Path,
    budget: Duration,
    exporter_filter: Option<&str>,
) -> TestExportResult {
    let deadline = Instant::now() + budget;
    let log_offset = current_log_size(log_path);

    let body = synthetic_payload();
    if let Err(emit_err) = otlp_emit::post_at_url(endpoint, &body, budget).await {
        return match emit_err {
            OtlpEmitError::Client(e) => TestExportResult {
                status: TestExportStatus::Timeout,
                detail: format!("could not build HTTP client: {e}"),
            },
            OtlpEmitError::Transport { endpoint, source } if source.is_timeout() => {
                TestExportResult {
                    status: TestExportStatus::Timeout,
                    detail: append_log_trailer(
                        format!("HTTP request to {endpoint} timed out: {source}"),
                        log_path,
                    ),
                }
            }
            OtlpEmitError::Transport { endpoint, source } => TestExportResult {
                status: TestExportStatus::Failed,
                detail: append_log_trailer(
                    format!("HTTP request to {endpoint} failed: {source}"),
                    log_path,
                ),
            },
            OtlpEmitError::NonOk { status, .. } => TestExportResult {
                status: TestExportStatus::Failed,
                detail: format!("collector receiver returned HTTP {status}"),
            },
        };
    }

    // The local collector accepted the payload. Now watch the log to
    // see whether the exporter forwarded it successfully. Otelcol
    // exporters log on failure within seconds; absence of a failure
    // marker by `deadline` means we report `ok`.
    while Instant::now() < deadline {
        if let Some(marker) = scan_log_for_marker(log_path, log_offset, exporter_filter) {
            return TestExportResult {
                status: TestExportStatus::Failed,
                detail: format!("collector log surfaced exporter failure: {marker}"),
            };
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    TestExportResult {
        status: TestExportStatus::Ok,
        detail: "synthetic payload accepted; no exporter failures observed in 5s".to_string(),
    }
}

fn current_log_size(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

/// Maximum number of bytes from the end of `collector.log` we examine
/// when surfacing diagnostic context on a transport failure.
const LOG_TAIL_BYTES: usize = 16 * 1024;
/// Cap on how many error lines we surface in the wizard's failure
/// detail. Keeps the message short while still giving users enough
/// context to diagnose a misconfiguration.
const LOG_TAIL_MAX_LINES: usize = 5;

/// Read up to the last `LOG_TAIL_BYTES` of `log_path`, return the most
/// recent lines whose lowercase form contains `"error"` or which match
/// any [`FAILURE_MARKERS`] entry. Bounded to [`LOG_TAIL_MAX_LINES`].
/// Used to surface "why isn't the local collector reachable?" context
/// when the synthetic-span POST cannot connect to 127.0.0.1:4318.
fn tail_log_errors(log_path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(log_path) else {
        return Vec::new();
    };
    if bytes.is_empty() {
        return Vec::new();
    }
    let start = bytes.len().saturating_sub(LOG_TAIL_BYTES);
    // Step forward from `start` until we land on a UTF-8 char boundary
    // so the slice decodes cleanly even on a mid-codepoint cut.
    let mut safe_start = start;
    while safe_start < bytes.len() && (bytes[safe_start] & 0b1100_0000) == 0b1000_0000 {
        safe_start += 1;
    }
    let Ok(tail) = std::str::from_utf8(&bytes[safe_start..]) else {
        return Vec::new();
    };
    let mut errors: Vec<String> = Vec::new();
    for line in tail.lines() {
        let lower = line.to_ascii_lowercase();
        let is_error = lower.contains("error")
            || FAILURE_MARKERS.iter().any(|m| line.contains(m));
        if is_error {
            errors.push(line.chars().take(240).collect());
        }
    }
    if errors.len() > LOG_TAIL_MAX_LINES {
        let drop = errors.len() - LOG_TAIL_MAX_LINES;
        errors.drain(0..drop);
    }
    errors
}

/// Append a "Recent collector log:" trailer to `detail` when
/// [`tail_log_errors`] finds anything. No-op when the log is missing,
/// empty, or contains no error lines.
fn append_log_trailer(detail: String, log_path: &Path) -> String {
    let lines = tail_log_errors(log_path);
    if lines.is_empty() {
        return detail;
    }
    let mut out = detail;
    out.push_str("\n\nRecent collector log:");
    for line in lines {
        out.push_str("\n> ");
        out.push_str(&line);
    }
    out
}

/// Read `log_path` from `since` to current length and return the first
/// failure-marker line found, or `None` if no marker is present in the
/// new bytes.
fn scan_log_for_marker(
    log_path: &Path,
    since: u64,
    exporter_filter: Option<&str>,
) -> Option<String> {
    let bytes = std::fs::read(log_path).ok()?;
    if (bytes.len() as u64) <= since {
        return None;
    }
    let since_usize = usize::try_from(since).unwrap_or(usize::MAX);
    let new_slice = &bytes[since_usize..];
    let new_text = std::str::from_utf8(new_slice).ok()?;
    for line in new_text.lines() {
        // Bug-I scoping: when an exporter component id is provided,
        // skip any failure line that doesn't mention it. otelcol logs
        // the exporter id as `otelcol.component.id: "<kind>/<name>-<suffix>"`
        // on every record; the substring match works for both the
        // structured JSON encoding and the human-readable console form.
        if let Some(needle) = exporter_filter {
            if !line.contains(needle) {
                continue;
            }
        }
        for marker in FAILURE_MARKERS {
            if line.contains(marker) {
                // Trim absurdly long lines so we don't dump the whole
                // collector trace into the wizard error pane.
                let snippet = line.chars().take(240).collect::<String>();
                return Some(snippet);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_payload_carries_recognisable_canary_ids() {
        let body = synthetic_payload();
        let span = &body["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span["traceId"], SYNTHETIC_TRACE_ID);
        assert_eq!(span["spanId"], SYNTHETIC_SPAN_ID);
        assert_eq!(span["name"], "test_export");
    }

    #[test]
    fn synthetic_payload_includes_resource_attributes() {
        let body = synthetic_payload();
        let attrs = body["resourceSpans"][0]["resource"]["attributes"]
            .as_array()
            .unwrap();
        let names: Vec<&str> = attrs.iter().map(|a| a["key"].as_str().unwrap()).collect();
        assert!(names.contains(&"service.name"));
        assert!(names.contains(&"trove.synthetic"));
    }

    #[test]
    fn scan_returns_none_on_missing_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.log");
        assert!(scan_log_for_marker(&path, 0, None).is_none());
    }

    #[test]
    fn scan_returns_first_marker_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        std::fs::write(
            &path,
            b"hello\nPermanent error: 401 Unauthorized\nstill running\n",
        )
        .unwrap();
        let line = scan_log_for_marker(&path, 0, None).unwrap();
        assert!(line.contains("Permanent error"));
    }

    #[test]
    fn scan_only_examines_bytes_after_since() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        let prefix = "Permanent error: stale\n";
        std::fs::write(&path, prefix).unwrap();
        let suffix_offset = prefix.len() as u64;
        // No new bytes after the offset → no marker.
        assert!(scan_log_for_marker(&path, suffix_offset, None).is_none());

        // Append new bytes containing a marker; should now match.
        let appended = "Permanent error: fresh\n";
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::fs::write(&path, format!("{prefix}{appended}").as_bytes()).unwrap();
        let line = scan_log_for_marker(&path, suffix_offset, None).unwrap();
        assert!(line.contains("fresh"));
    }

    #[test]
    fn scan_truncates_overlong_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        let huge = format!("Permanent error: {}\n", "x".repeat(5000));
        std::fs::write(&path, &huge).unwrap();
        let line = scan_log_for_marker(&path, 0, None).unwrap();
        assert!(line.len() <= 240);
    }

    #[test]
    fn scan_with_exporter_filter_ignores_unrelated_failure_lines() {
        // Bug-I regression lock. Two exporters in the log; one is the
        // row under test (sentry-3a0c9975), the other is unrelated
        // (signoz-31fb8e0a) and is retry-looping. With the filter set
        // to the sentry exporter id, the scan must ignore the signoz
        // failure entirely.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        let body = "\
2026-05-25T10:00:00 info  some healthy line\n\
2026-05-25T10:00:01 error Exporting failed. Will retry. {\"otelcol.component.id\":\"otlp/signoz-31fb8e0a\",\"error\":\"connection refused\"}\n\
2026-05-25T10:00:02 info  unrelated line\n\
";
        std::fs::write(&path, body).unwrap();
        assert!(
            scan_log_for_marker(&path, 0, Some("otlphttp/sentry-3a0c9975")).is_none(),
            "filter should drop the signoz failure"
        );
        // Confirm the same file would have matched without the filter
        // (sanity check: the bug is the filter, not the markers).
        assert!(scan_log_for_marker(&path, 0, None).is_some());
    }

    #[test]
    fn scan_with_exporter_filter_matches_target_failure_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        let body = "\
2026-05-25T10:00:00 info  unrelated line\n\
2026-05-25T10:00:01 error Exporting failed. {\"otelcol.component.id\":\"otlphttp/sentry-3a0c9975\",\"error\":\"Permanent error: 404\"}\n\
";
        std::fs::write(&path, body).unwrap();
        let line =
            scan_log_for_marker(&path, 0, Some("otlphttp/sentry-3a0c9975")).unwrap();
        assert!(line.contains("Permanent error"));
        assert!(line.contains("otlphttp/sentry-3a0c9975"));
    }

    #[test]
    fn tail_log_errors_returns_empty_when_log_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.log");
        assert!(tail_log_errors(&path).is_empty());
    }

    #[test]
    fn tail_log_errors_returns_empty_when_no_error_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        std::fs::write(&path, b"info\tstarted\ninfo\thealth ok\n").unwrap();
        assert!(tail_log_errors(&path).is_empty());
    }

    #[test]
    fn tail_log_errors_surfaces_error_and_marker_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        std::fs::write(
            &path,
            b"info\tstarted\nerror\texporter dial tcp: connection refused\n\
              info\tnoise\nFailed to send 5 spans to backend\n",
        )
        .unwrap();
        let lines = tail_log_errors(&path);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("connection refused"));
        assert!(lines[1].contains("Failed to send"));
    }

    #[test]
    fn tail_log_errors_caps_at_max_lines_keeping_most_recent() {
        use std::fmt::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        let mut buf = String::new();
        for i in 0..20 {
            writeln!(buf, "error\tfailure number {i}").unwrap();
        }
        std::fs::write(&path, buf.as_bytes()).unwrap();
        let lines = tail_log_errors(&path);
        assert_eq!(lines.len(), LOG_TAIL_MAX_LINES);
        // Last line in the file must be in the surfaced set.
        assert!(lines.last().unwrap().contains("number 19"));
    }

    #[test]
    fn append_log_trailer_is_noop_when_no_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        std::fs::write(&path, b"info\tquiet\n").unwrap();
        let detail = append_log_trailer("HTTP request failed".to_string(), &path);
        assert_eq!(detail, "HTTP request failed");
    }

    #[test]
    fn append_log_trailer_appends_recent_errors_under_preamble() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        std::fs::write(
            &path,
            b"info\tstartup\nerror\texporter dial tcp 127.0.0.1:4317: connection refused\n",
        )
        .unwrap();
        let detail = append_log_trailer(
            "HTTP request to http://127.0.0.1:4318/v1/traces failed: connect refused".to_string(),
            &path,
        );
        assert!(detail.starts_with("HTTP request to http://127.0.0.1:4318/v1/traces failed: connect refused"));
        assert!(detail.contains("Recent collector log:"));
        assert!(detail.contains("> error\texporter dial tcp 127.0.0.1:4317: connection refused"));
    }

    #[tokio::test]
    async fn test_export_returns_failed_when_endpoint_unreachable() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("absent.log");
        // Port 1 is always unbound on a normal machine; reqwest fails fast.
        let result = test_export_at(
            "http://127.0.0.1:1/v1/traces",
            &log,
            Duration::from_millis(800),
            None,
        )
        .await;
        // Either Failed (request error) or Timeout (under 1s budget) is
        // acceptable — both are red in the wizard.
        assert!(
            matches!(result.status, TestExportStatus::Failed | TestExportStatus::Timeout),
            "expected Failed or Timeout, got {result:?}",
        );
    }
}
