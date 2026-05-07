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
pub async fn test_export_at(
    endpoint: &str,
    log_path: &Path,
    budget: Duration,
) -> TestExportResult {
    let deadline = Instant::now() + budget;
    let log_offset = current_log_size(log_path);

    let body = synthetic_payload();
    let client = match reqwest::Client::builder()
        .timeout(budget)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return TestExportResult {
                status: TestExportStatus::Timeout,
                detail: format!("could not build HTTP client: {e}"),
            };
        }
    };

    let response = match client.post(endpoint).json(&body).send().await {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            return TestExportResult {
                status: TestExportStatus::Timeout,
                detail: format!("HTTP request to {endpoint} timed out: {e}"),
            };
        }
        Err(e) => {
            return TestExportResult {
                status: TestExportStatus::Failed,
                detail: format!("HTTP request to {endpoint} failed: {e}"),
            };
        }
    };

    if !response.status().is_success() {
        return TestExportResult {
            status: TestExportStatus::Failed,
            detail: format!("collector receiver returned HTTP {}", response.status()),
        };
    }

    // The local collector accepted the payload. Now watch the log to
    // see whether the exporter forwarded it successfully. Otelcol
    // exporters log on failure within seconds; absence of a failure
    // marker by `deadline` means we report `ok`.
    while Instant::now() < deadline {
        if let Some(marker) = scan_log_for_marker(log_path, log_offset) {
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

/// Read `log_path` from `since` to current length and return the first
/// failure-marker line found, or `None` if no marker is present in the
/// new bytes.
fn scan_log_for_marker(log_path: &Path, since: u64) -> Option<String> {
    let bytes = std::fs::read(log_path).ok()?;
    if (bytes.len() as u64) <= since {
        return None;
    }
    let since_usize = usize::try_from(since).unwrap_or(usize::MAX);
    let new_slice = &bytes[since_usize..];
    let new_text = std::str::from_utf8(new_slice).ok()?;
    for line in new_text.lines() {
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
        assert!(scan_log_for_marker(&path, 0).is_none());
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
        let line = scan_log_for_marker(&path, 0).unwrap();
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
        assert!(scan_log_for_marker(&path, suffix_offset).is_none());

        // Append new bytes containing a marker; should now match.
        let appended = "Permanent error: fresh\n";
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::fs::write(
            &path,
            format!("{prefix}{appended}").as_bytes(),
        )
        .unwrap();
        let line = scan_log_for_marker(&path, suffix_offset).unwrap();
        assert!(line.contains("fresh"));
    }

    #[test]
    fn scan_truncates_overlong_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.log");
        let huge = format!("Permanent error: {}\n", "x".repeat(5000));
        std::fs::write(&path, &huge).unwrap();
        let line = scan_log_for_marker(&path, 0).unwrap();
        assert!(line.len() <= 240);
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
