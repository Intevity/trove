//! Tier 3 OTLP/HTTP emitter.
//!
//! Sprint 9 PR 1 extracts the wizard's reqwest-based POST primitive into
//! a shared module so the Tier 3 adapters (Cline, Aider, Copilot CLI) can
//! deliver their parsed events to the supervised local collector at
//! `127.0.0.1:4318` without each adapter rebuilding the same client.
//!
//! Three helpers, one per OTLP signal kind, all targeting the loopback
//! collector. They hard-code the loopback host because Trove's safety
//! contract forbids egress beyond it — every payload routes through the
//! user's collector, which is the only component that knows the user's
//! backend credentials.
//!
//! The helpers are fire-and-forget by intent: a 2s default timeout means
//! a hung collector never blocks an adapter's watcher loop. Tier 3 log
//! parsers swallow `OtlpEmitError` after a `tracing::warn!` and continue
//! — losing one event is preferable to the watcher dying.

use std::time::Duration;

use serde_json::Value;

/// Default total wall-clock budget for a single OTLP POST. Tighter than
/// the wizard's `test_export` budget (5s) — Tier 3 is in the hot path of
/// an adapter's watcher loop, where blocking a few seconds delays every
/// subsequent event.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// Where the supervised collector binds. Trove never points its emitters
/// anywhere else; the user's backend credentials live in the collector,
/// not in the app.
pub const COLLECTOR_BASE: &str = "http://127.0.0.1:4318";

/// What can go wrong when posting an OTLP/HTTP/JSON payload.
#[derive(Debug, thiserror::Error)]
pub enum OtlpEmitError {
    /// `reqwest::Client::builder().build()` failed. Practically only
    /// happens when the OS prevents TLS init — extremely rare on the
    /// platforms Trove ships to.
    #[error("could not build HTTP client: {0}")]
    Client(reqwest::Error),
    /// The HTTP request itself failed: connection refused, timeout, DNS,
    /// etc. The collector being down is the most common cause; the
    /// caller should treat it as recoverable and try the next event.
    #[error("HTTP request to {endpoint} failed: {source}")]
    Transport {
        endpoint: String,
        #[source]
        source: reqwest::Error,
    },
    /// The collector returned a non-2xx status. Body is captured so the
    /// caller can log a snippet without re-fetching.
    #[error("collector returned HTTP {status} for {endpoint}: {body}")]
    NonOk {
        endpoint: String,
        status: u16,
        body: String,
    },
}

/// POST `payload` to the local collector's `/v1/traces` endpoint.
pub async fn post_traces_json(payload: &Value) -> Result<(), OtlpEmitError> {
    post_signal(SignalKind::Traces, payload, COLLECTOR_BASE, DEFAULT_TIMEOUT).await
}

/// POST `payload` to the local collector's `/v1/logs` endpoint.
pub async fn post_logs_json(payload: &Value) -> Result<(), OtlpEmitError> {
    post_signal(SignalKind::Logs, payload, COLLECTOR_BASE, DEFAULT_TIMEOUT).await
}

/// POST `payload` to the local collector's `/v1/metrics` endpoint.
pub async fn post_metrics_json(payload: &Value) -> Result<(), OtlpEmitError> {
    post_signal(SignalKind::Metrics, payload, COLLECTOR_BASE, DEFAULT_TIMEOUT).await
}

/// Test-friendly variant of [`post_traces_json`] / friends. Lets tests
/// point at a stub OTLP receiver on a tempdir-scoped port and override
/// the timeout. Production code calls the three signal-kind helpers
/// above, which lock `base` to the loopback constant.
pub async fn post_signal(
    kind: SignalKind,
    payload: &Value,
    base: &str,
    timeout: Duration,
) -> Result<(), OtlpEmitError> {
    let endpoint = format!("{base}{}", kind.path());
    post_at_url(&endpoint, payload, timeout).await
}

/// Lowest-level primitive: POST `payload` as OTLP/HTTP/JSON to `url`.
/// The wizard's `test_export` calls this directly with its own full URL
/// (already includes the `/v1/traces` suffix); Tier 3 adapters go
/// through the [`post_signal`] / [`post_traces_json`] / [`post_logs_json`] /
/// [`post_metrics_json`] sugar that builds the URL from the loopback
/// base + signal kind.
pub async fn post_at_url(
    url: &str,
    payload: &Value,
    timeout: Duration,
) -> Result<(), OtlpEmitError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(OtlpEmitError::Client)?;

    let response = client
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|source| OtlpEmitError::Transport {
            endpoint: url.to_string(),
            source,
        })?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable response body>".to_string());
    let snippet: String = body.chars().take(240).collect();
    Err(OtlpEmitError::NonOk {
        endpoint: url.to_string(),
        status: status.as_u16(),
        body: snippet,
    })
}

/// OTLP/HTTP signal kinds Trove emits. The path variant is the URL path
/// segment `OTel` collectors listen on by convention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalKind {
    Traces,
    Logs,
    Metrics,
}

impl SignalKind {
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Traces => "/v1/traces",
            Self::Logs => "/v1/logs",
            Self::Metrics => "/v1/metrics",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    /// Tiny stub OTLP/HTTP receiver. Returns `(base, captured)` where
    /// `captured` accumulates one entry per accepted POST: `(path, body)`.
    /// The receiver replies with `status_code`; pass 200 for the happy
    /// path or 4xx/5xx to exercise the `NonOk` path.
    async fn start_stub(
        status_code: u16,
    ) -> (String, Arc<tokio::sync::Mutex<Vec<(String, String)>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let captured: Arc<tokio::sync::Mutex<Vec<(String, String)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let captured = captured_clone.clone();
                tokio::spawn(async move {
                    let (read_half, mut write_half) = socket.split();
                    let mut reader = BufReader::new(read_half);
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).await.is_err() {
                        return;
                    }
                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_string();

                    let mut content_length: usize = 0;
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).await.unwrap_or(0) == 0 {
                            break;
                        }
                        if header == "\r\n" || header == "\n" {
                            break;
                        }
                        let lower = header.to_ascii_lowercase();
                        if let Some(rest) = lower.strip_prefix("content-length:") {
                            content_length = rest.trim().parse().unwrap_or(0);
                        }
                    }

                    let mut body_buf = vec![0u8; content_length];
                    let _ = reader.read_exact(&mut body_buf).await;
                    let body = String::from_utf8_lossy(&body_buf).into_owned();

                    captured.lock().await.push((path, body));

                    let reason = match status_code {
                        200 => "OK",
                        201 => "Created",
                        400 => "Bad Request",
                        500 => "Internal Server Error",
                        _ => "Status",
                    };
                    let response = format!(
                        "HTTP/1.1 {status_code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    let _ = write_half.write_all(response.as_bytes()).await;
                });
            }
        });

        (format!("http://{addr}"), captured)
    }

    #[test]
    fn signal_kind_paths_are_otlp_http_canonical() {
        assert_eq!(SignalKind::Traces.path(), "/v1/traces");
        assert_eq!(SignalKind::Logs.path(), "/v1/logs");
        assert_eq!(SignalKind::Metrics.path(), "/v1/metrics");
    }

    #[test]
    fn collector_base_is_loopback_only() {
        // Safety: Trove never points emitters anywhere but the supervised
        // collector. Hard-code the assertion so a future refactor can't
        // silently change the egress destination.
        assert_eq!(COLLECTOR_BASE, "http://127.0.0.1:4318");
    }

    #[tokio::test]
    async fn post_signal_traces_succeeds_against_stub() {
        let (base, captured) = start_stub(200).await;
        let payload = serde_json::json!({"resourceSpans": []});
        post_signal(SignalKind::Traces, &payload, &base, DEFAULT_TIMEOUT)
            .await
            .unwrap();
        let entries = captured.lock().await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "/v1/traces");
        assert!(entries[0].1.contains("resourceSpans"));
    }

    #[tokio::test]
    async fn post_signal_logs_routes_to_v1_logs_path() {
        let (base, captured) = start_stub(200).await;
        let payload = serde_json::json!({"resourceLogs": []});
        post_signal(SignalKind::Logs, &payload, &base, DEFAULT_TIMEOUT)
            .await
            .unwrap();
        let entries = captured.lock().await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "/v1/logs");
    }

    #[tokio::test]
    async fn post_signal_metrics_routes_to_v1_metrics_path() {
        let (base, captured) = start_stub(200).await;
        let payload = serde_json::json!({"resourceMetrics": []});
        post_signal(SignalKind::Metrics, &payload, &base, DEFAULT_TIMEOUT)
            .await
            .unwrap();
        let entries = captured.lock().await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "/v1/metrics");
    }

    #[tokio::test]
    async fn non_2xx_response_returns_non_ok_error() {
        let (base, _captured) = start_stub(400).await;
        let payload = serde_json::json!({});
        let err = post_signal(SignalKind::Traces, &payload, &base, DEFAULT_TIMEOUT)
            .await
            .unwrap_err();
        match err {
            OtlpEmitError::NonOk { status, .. } => assert_eq!(status, 400),
            other => panic!("expected NonOk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unreachable_endpoint_returns_transport_error() {
        // Port 1 is reserved/unbound on a normal machine; reqwest fails
        // fast with a connection error.
        let payload = serde_json::json!({});
        let err = post_signal(
            SignalKind::Traces,
            &payload,
            "http://127.0.0.1:1",
            Duration::from_millis(800),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, OtlpEmitError::Transport { .. }));
    }

    #[tokio::test]
    async fn concurrent_emits_each_land_intact() {
        let (base, captured) = start_stub(200).await;
        let p1 = serde_json::json!({"resourceLogs": [{"a": 1}]});
        let p2 = serde_json::json!({"resourceLogs": [{"b": 2}]});
        let r1 = post_signal(SignalKind::Logs, &p1, &base, DEFAULT_TIMEOUT);
        let r2 = post_signal(SignalKind::Logs, &p2, &base, DEFAULT_TIMEOUT);
        let (a, b) = tokio::join!(r1, r2);
        a.unwrap();
        b.unwrap();
        let entries = captured.lock().await;
        assert_eq!(entries.len(), 2);
        for (path, _) in entries.iter() {
            assert_eq!(path, "/v1/logs");
        }
    }
}
