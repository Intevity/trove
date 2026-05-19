//! Tee Collector child stdout/stderr to the parent's `tracing` logger and
//! to an append-mode log file under the platform-specific state dir.
//!
//! The file is size-capped: when it crosses [`MAX_LOG_BYTES`] it's rotated
//! once to `<name>.1`, replacing any previous rotation. This is
//! deliberately simpler than a full rolling-log library — Sprint 11's
//! diagnostics tab can replace it if we need richer rotation policies.

use std::path::{Path, PathBuf};

use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;

use super::lifecycle::CollectorLogLine;

/// Roll the log file when it exceeds 10 MiB. Tuned to keep ~one boot
/// cycle of detailed output without risking unbounded disk usage.
pub const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Read lines from `reader` (typically a `ChildStdout` / `ChildStderr`)
/// and append them to `log_path`, also re-emitting them via `tracing` at
/// the supplied level. Each line is also forwarded onto `broadcast` (in
/// front of the file write, so rotation cannot drop lines en route to
/// the dashboard's logs panel). Exits when the reader returns EOF or
/// errors.
pub async fn tee_stream<R>(
    mut reader: BufReader<R>,
    log_path: PathBuf,
    level: tracing::Level,
    stream_label: &'static str,
    broadcast: broadcast::Sender<CollectorLogLine>,
) where
    R: AsyncRead + Unpin,
{
    if let Err(e) = ensure_parent_dir(&log_path).await {
        tracing::warn!(?e, ?log_path, "could not create collector log directory");
        return;
    }

    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf).await {
            Ok(0) => return, // EOF
            Ok(_) => {
                let line = buf.trim_end_matches('\n').trim_end_matches('\r');
                emit_traced(level, stream_label, line);
                // Broadcast in front of the disk write so the dashboard
                // sees lines even if rotation is in flight. send() errors
                // when no receivers are attached — that's expected (the
                // dashboard may not be mounted yet) and non-fatal.
                let _ = broadcast.send(CollectorLogLine {
                    stream: stream_label,
                    line: line.to_string(),
                });
                if let Err(e) = append_with_rotation(&log_path, line).await {
                    tracing::warn!(?e, ?log_path, "could not append to collector log");
                }
            }
            Err(e) => {
                tracing::warn!(?e, "error reading collector stream; closing tee");
                return;
            }
        }
    }
}

fn emit_traced(level: tracing::Level, stream_label: &'static str, line: &str) {
    match level {
        tracing::Level::ERROR => tracing::error!(stream = stream_label, "{line}"),
        tracing::Level::WARN => tracing::warn!(stream = stream_label, "{line}"),
        tracing::Level::INFO => tracing::info!(stream = stream_label, "{line}"),
        tracing::Level::DEBUG => tracing::debug!(stream = stream_label, "{line}"),
        tracing::Level::TRACE => tracing::trace!(stream = stream_label, "{line}"),
    }
}

async fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}

async fn append_with_rotation(path: &Path, line: &str) -> std::io::Result<()> {
    rotate_if_needed(path).await?;
    let mut f: File = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    f.write_all(line.as_bytes()).await?;
    f.write_all(b"\n").await?;
    Ok(())
}

async fn rotate_if_needed(path: &Path) -> std::io::Result<()> {
    let meta = match fs::metadata(path).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.len() < MAX_LOG_BYTES {
        return Ok(());
    }
    let rotated = rotated_path(path);
    if rotated.exists() {
        fs::remove_file(&rotated).await?;
    }
    fs::rename(path, &rotated).await?;
    Ok(())
}

fn rotated_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".1");
    PathBuf::from(s)
}

/// One exporter-error event extracted from a collector stderr line.
/// Drives the tooltip text shown when a destination's health pill is
/// red or amber.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExporterErrorLine {
    /// OTel collector component id, e.g. `otlphttp/openobserve-93eb10f1`.
    pub component_id: String,
    /// The raw `error` field text from the structured log line.
    pub error: String,
}

/// Best-effort parse of a single collector stderr line into an
/// [`ExporterErrorLine`]. Returns `None` for lines that don't look like
/// an exporter-failure record — the substring check makes this cheap
/// on the hot path so it can run inline against every broadcast line.
///
/// Lines we recognise (captured live during the 2026-05-18 release
/// pairing pass) look like:
///
/// ```text
/// <ts> info  internal/retry_sender.go:133  Exporting failed. Will retry the request after interval.  {"resource":{...},"otelcol.component.id":"otlphttp/opensearch-a7f0880b","otelcol.component.kind":"exporter","otelcol.signal":"logs","error":"failed to make an HTTP request: ..."}
/// ```
///
/// This parser is intentionally NOT a full JSON parser. It scans for
/// the three fields it cares about by string match — the collector's
/// log format is stable but the surrounding noise (timestamps, ANSI
/// colour codes from `tracing`) isn't worth fighting through `serde_json`
/// every line. Lines without an `error` field (happy-path exporter
/// records) are dropped.
#[must_use]
pub fn try_parse_exporter_error_line(line: &str) -> Option<ExporterErrorLine> {
    // The otelcol Go logger writes structured logs with a space after the
    // JSON `:` separator (`"k": "v"`), but some downstream pretty-printers
    // strip it. Accept both: do a substring check for the key (no value
    // pinning), then verify the *value* through the same lookup as
    // `extract_quoted_string_field` so the body is parsed once.
    if !line.contains("\"otelcol.component.kind\"") {
        return None;
    }
    let kind = extract_quoted_string_field(line, "\"otelcol.component.kind\"")?;
    if kind != "exporter" {
        return None;
    }
    let component_id = extract_quoted_string_field(line, "\"otelcol.component.id\"")?;
    let error = extract_quoted_string_field(line, "\"error\"")?;
    Some(ExporterErrorLine {
        component_id: component_id.to_string(),
        error: error.to_string(),
    })
}

/// Pull the value of a `"key": "…"` JSON field out of `haystack`,
/// given the quote-wrapped key (e.g. `"\"error\""`). Tolerates both
/// `"key":"v"` and `"key": "v"` (the otelcol Go logger emits the
/// latter). Handles backslash-escaped quotes inside the value.
/// Returns `None` if the field is absent or malformed.
fn extract_quoted_string_field<'a>(haystack: &'a str, quoted_key: &str) -> Option<&'a str> {
    let start = haystack.find(quoted_key)?;
    let after = start + quoted_key.len();
    let rest = haystack.get(after..)?;
    // Skip whitespace, the `:`, more whitespace, then the opening quote.
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    // Scan to the closing unescaped quote.
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            return rest.get(..i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim line shape captured during the 2026-05-18 Fix 1
    /// validation run — connection-refused on the opensearch exporter
    /// while the rest of the fan-out was healthy. Confirms the parser
    /// pulls both the component id and the error text out cleanly.
    #[test]
    fn parses_real_retry_sender_line() {
        let line = "2026-05-18T20:41:13.946-0400\tinfo\tinternal/retry_sender.go:133\tExporting failed. Will retry the request after interval.\t{\"resource\":{\"service.instance.id\":\"82fa073f\"},\"otelcol.component.id\":\"otlphttp/opensearch-a7f0880b\",\"otelcol.component.kind\":\"exporter\",\"otelcol.signal\":\"logs\",\"error\":\"failed to make an HTTP request: Post \\\"http://localhost:14326/v1/logs\\\": dial tcp [::1]:14326: connect: connection refused\",\"interval\":\"42.654289956s\"}";
        let parsed = try_parse_exporter_error_line(line).expect("should parse");
        assert_eq!(parsed.component_id, "otlphttp/opensearch-a7f0880b");
        assert!(
            parsed.error.starts_with("failed to make an HTTP request: Post "),
            "got error: {}",
            parsed.error,
        );
        assert!(parsed.error.contains("connection refused"));
    }

    #[test]
    fn ignores_lines_without_error_field() {
        // Real "Starting exporter" line — component kind matches but
        // there's no `error` field. Must return None.
        let line = "2026-05-18T20:37:52.295273Z info  builders/builders.go:40  \"otlp\" alias is deprecated; use \"otlp_grpc\" instead\t{\"otelcol.component.id\":\"otlp/signoz-31fb8e0a\",\"otelcol.component.kind\":\"exporter\",\"otelcol.signal\":\"traces\"}";
        assert_eq!(try_parse_exporter_error_line(line), None);
    }

    #[test]
    fn ignores_non_exporter_component_lines() {
        // A receiver line — different component.kind. Must return None.
        let line = "{\"otelcol.component.id\":\"otlp\",\"otelcol.component.kind\":\"receiver\",\"error\":\"binding failed\"}";
        assert_eq!(try_parse_exporter_error_line(line), None);
    }

    #[test]
    fn ignores_completely_unrelated_lines() {
        let line = "Hello from Trove dev console";
        assert_eq!(try_parse_exporter_error_line(line), None);
    }

    #[test]
    fn handles_minimal_well_formed_line() {
        let line = "{\"otelcol.component.id\":\"otlphttp/openobserve-93eb10f1\",\"otelcol.component.kind\":\"exporter\",\"error\":\"401 Unauthorized\"}";
        let parsed = try_parse_exporter_error_line(line).expect("should parse");
        assert_eq!(parsed.component_id, "otlphttp/openobserve-93eb10f1");
        assert_eq!(parsed.error, "401 Unauthorized");
    }

    #[test]
    fn extract_quoted_string_handles_escaped_quotes() {
        let s = r#"{"error":"got status \"500\" from upstream"}"#;
        let v = extract_quoted_string_field(s, "\"error\"").unwrap();
        assert_eq!(v, r#"got status \"500\" from upstream"#);
    }

    /// The Go otelcol logger writes `"key": "value"` with a space after
    /// the colon. The earlier substring check pinning `"key":"value"`
    /// silently rejected every real stderr line in dev validation,
    /// leaving destinations stuck at Gray instead of Red. Verifying the
    /// space-tolerant path stays intact going forward.
    #[test]
    fn parses_space_separated_json_otelcol_dev_format() {
        let line = r#"2026-05-18T20:08:06.121-0400 info internal/retry_sender.go:133 Exporting failed. Will retry the request after interval. {"resource": {"service.instance.id": "abc"}, "otelcol.component.id": "otlphttp/opensearch-a7f0880b", "otelcol.component.kind": "exporter", "otelcol.signal": "logs", "error": "failed to make an HTTP request: Post \"http://localhost:14326/v1/logs\": dial tcp [::1]:14326: connect: connection refused", "interval": "42.65s"}"#;
        let parsed = try_parse_exporter_error_line(line).expect("space-separated form must parse");
        assert_eq!(parsed.component_id, "otlphttp/opensearch-a7f0880b");
        assert!(parsed.error.contains("connection refused"));
    }
}
