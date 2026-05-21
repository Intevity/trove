//! Scrape the bundled trove-otelcol's internal Prometheus metrics and
//! publish them as a [`MetricsSnapshot`] for the dashboard + tray.
//!
//! When `service.telemetry.metrics.address: 127.0.0.1:18888` is set in
//! the active YAML (added in this same PR to every preset and the
//! smoke config), the Collector exposes its own counters on
//! `:18888/metrics` in Prometheus exposition format. Port 18888 is
//! the OTel default 8888 shifted by +10000 so we don't collide with
//! other tooling that uses 8888 (devspace, generic Prometheus setups,
//! vanilla otelcol runs). The interesting families for Sprint 6:
//!
//! - `otelcol_receiver_accepted_spans` / `_metric_points` / `_log_records`
//!   — total signals the OTLP receivers have accepted from harnesses.
//! - `otelcol_exporter_sent_spans` / `_metric_points` / `_log_records`
//!   — total signals the user-backend exporter has shipped onward.
//!
//! Each family is split across receiver/exporter and transport label
//! dimensions; we sum across all of them to produce a single family
//! total. The "last signal at" timestamp ticks every time the
//! receiver-totals strictly increase between scrapes — that's the
//! signal the dashboard's `OverallHealthBadge` consumes to flip from
//! amber → green.
//!
//! The tap publishes `Option<MetricsSnapshot>`:
//! - `None` until the first scrape attempt completes (so the UI can
//!   render a skeleton rather than zeros).
//! - `Some(snapshot)` thereafter. `unreachable: true` distinguishes a
//!   user who customised the YAML to drop the telemetry block from a
//!   genuine "zero traffic" steady state.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

/// Default endpoint the bundled YAML configs expose. Trove never binds
/// external interfaces.
pub const DEFAULT_METRICS_URL: &str = "http://127.0.0.1:18888/metrics";

/// Default scrape cadence. Aligns with the watch coalescing semantics:
/// the dashboard only needs the latest snapshot, not a packet of every
/// intermediate read.
pub const DEFAULT_SCRAPE_INTERVAL: Duration = Duration::from_secs(5);

/// Per-request timeout. Tight enough that a wedged collector doesn't
/// stall the scrape loop; loose enough to tolerate a slow first response
/// while telemetry warms up.
pub const DEFAULT_SCRAPE_TIMEOUT: Duration = Duration::from_secs(2);

/// Counts of one signal type, summed across all receiver/exporter/
/// transport label combinations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SignalCounts {
    pub spans: u64,
    pub metric_points: u64,
    pub log_records: u64,
}

impl SignalCounts {
    #[must_use]
    pub fn total(&self) -> u64 {
        self.spans
            .saturating_add(self.metric_points)
            .saturating_add(self.log_records)
    }
}

/// Per-harness outgoing counters from the diag filter pipelines. One
/// `SignalCounts` per harness suffix; populated only for harnesses with
/// a `filter/diag-<suffix>` processor in the collector YAML (every
/// enabled native-OTel emitter). The dashboard `FlowChart` subtracts
/// successive snapshots to derive a per-harness rate per signal type.
pub type DiagObservations = HashMap<String, SignalCounts>;

/// One scrape's worth of state. Wrapped in `Option` by the watch
/// publisher; the snapshot itself is always concrete.
///
/// `last_signal_at` is `None` until the first time the receiver totals
/// strictly increase between scrapes. After that it tracks the most
/// recent such increase. Persists across `unreachable: true` snapshots
/// so the dashboard doesn't flicker amber on a transient :18888 hiccup.
#[derive(Clone, Debug, Serialize)]
pub struct MetricsSnapshot {
    pub received: SignalCounts,
    pub sent: SignalCounts,
    /// Per-`filter/diag-<suffix>` outgoing counters, keyed by harness
    /// suffix (e.g. `"gemini-cli"`). Each entry counts spans, metric
    /// points, and log records the diag pipeline observed for that
    /// harness. Populated only for native-OTel emitters (those with
    /// non-empty `native_service_name_candidates`).
    #[serde(default)]
    pub diag_observations: DiagObservations,
    /// Per-exporter cumulative counters, keyed by `OTel` component id
    /// (e.g. `otlphttp/openobserve-93eb10f1`). Drives the per-platform
    /// health pill via `BackendHealthSamples::observe`.
    #[serde(default, skip)]
    pub per_exporter: HashMap<String, ExporterCounts>,
    /// Local monotonic timestamp of the most recent scrape that saw a
    /// strict increase in `received`. `None` means "no traffic seen
    /// since the tap started".
    #[serde(skip)]
    pub last_signal_at: Option<Instant>,
    /// When this snapshot was produced. Used by the UI for "scraped Ns
    /// ago" displays.
    #[serde(skip)]
    pub scraped_at: Instant,
    /// True when the most recent scrape failed to connect to the
    /// metrics endpoint. The pre-existing counts are kept (so the UI
    /// can still show "last seen" totals) but the dashboard renders a
    /// "metrics endpoint unreachable" callout and the tray turns amber.
    pub unreachable: bool,
}

impl Default for MetricsSnapshot {
    fn default() -> Self {
        Self {
            received: SignalCounts::default(),
            sent: SignalCounts::default(),
            diag_observations: HashMap::new(),
            per_exporter: HashMap::new(),
            last_signal_at: None,
            scraped_at: Instant::now(),
            unreachable: false,
        }
    }
}

/// Background scraper. Owns a watch sender publishing
/// `Option<MetricsSnapshot>`; subscribers (tray, IPC commands) take a
/// receiver via [`MetricsTapHandle::subscribe`]. The handle is held by
/// `lib.rs` for the lifetime of the app so the same channel survives
/// across `reload_collector` calls.
pub struct MetricsTap;

impl MetricsTap {
    /// Spawn the scrape loop on the Tauri runtime. The supplied URL is
    /// usually [`DEFAULT_METRICS_URL`]; tests can point it at a stub
    /// HTTP server.
    #[must_use]
    pub fn start(opts: MetricsTapOptions) -> MetricsTapHandle {
        let (snapshot_tx, snapshot_rx) = watch::channel::<Option<MetricsSnapshot>>(None);
        let snapshot_tx = Arc::new(snapshot_tx);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let task_tx = snapshot_tx.clone();
        let join = tauri::async_runtime::spawn(async move {
            scrape_loop(opts, task_tx, shutdown_rx).await;
        });

        MetricsTapHandle {
            snapshot_tx,
            snapshot_rx,
            shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
            join: std::sync::Mutex::new(Some(join)),
        }
    }
}

/// Tunables for the scrape loop. Defaults match the bundled YAML.
#[derive(Clone, Debug)]
pub struct MetricsTapOptions {
    pub url: String,
    pub scrape_interval: Duration,
    pub scrape_timeout: Duration,
}

impl Default for MetricsTapOptions {
    fn default() -> Self {
        Self {
            url: DEFAULT_METRICS_URL.to_string(),
            scrape_interval: DEFAULT_SCRAPE_INTERVAL,
            scrape_timeout: DEFAULT_SCRAPE_TIMEOUT,
        }
    }
}

/// Owns the scrape task and the watch sender. Drop without `shutdown()`
/// terminates the task on the next loop iteration via the oneshot
/// channel close.
pub struct MetricsTapHandle {
    snapshot_tx: Arc<watch::Sender<Option<MetricsSnapshot>>>,
    snapshot_rx: watch::Receiver<Option<MetricsSnapshot>>,
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    join: std::sync::Mutex<Option<JoinHandle<()>>>,
}

#[allow(dead_code)]
impl MetricsTapHandle {
    /// Snapshot the latest published snapshot, if any.
    #[must_use]
    pub fn latest(&self) -> Option<MetricsSnapshot> {
        self.snapshot_rx.borrow().clone()
    }

    /// Subscribe to snapshot transitions. The receiver is seeded with
    /// the current value (`None` until the first scrape returns).
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<Option<MetricsSnapshot>> {
        self.snapshot_rx.clone()
    }

    /// Lend out the shared sender so a future scrape task (if we ever
    /// rebuild the tap during a reload) can publish into the same
    /// watch channel that existing subscribers hold receivers on.
    #[must_use]
    pub fn sender(&self) -> Arc<watch::Sender<Option<MetricsSnapshot>>> {
        self.snapshot_tx.clone()
    }

    /// Stop the scrape loop and await its exit.
    pub async fn shutdown(&self) {
        let tx = {
            let mut guard = self
                .shutdown_tx
                .lock()
                .expect("metrics tap shutdown mutex poisoned");
            guard.take()
        };
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
        let join = {
            let mut guard = self
                .join
                .lock()
                .expect("metrics tap join mutex poisoned");
            guard.take()
        };
        if let Some(join) = join {
            let _ = join.await;
        }
    }
}

async fn scrape_loop(
    opts: MetricsTapOptions,
    sender: Arc<watch::Sender<Option<MetricsSnapshot>>>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let client = build_client(opts.scrape_timeout);
    let mut prev_received_total: u64 = 0;
    let mut last_signal_at: Option<Instant> = None;

    loop {
        let now = Instant::now();
        let scrape_result = scrape_once(&client, &opts.url).await;
        match scrape_result {
            ScrapeOutcome::Ok {
                received,
                sent,
                diag_observations,
                per_exporter,
            } => {
                let total_now = received.total();
                if total_now > prev_received_total {
                    last_signal_at = Some(now);
                }
                prev_received_total = total_now;
                sender.send_replace(Some(MetricsSnapshot {
                    received,
                    sent,
                    diag_observations,
                    per_exporter,
                    last_signal_at,
                    scraped_at: now,
                    unreachable: false,
                }));
            }
            ScrapeOutcome::Unreachable => {
                let prev = sender.borrow().clone();
                let (received, sent, diag_observations, per_exporter) = prev
                    .map(|s| (s.received, s.sent, s.diag_observations, s.per_exporter))
                    .unwrap_or_default();
                sender.send_replace(Some(MetricsSnapshot {
                    received,
                    sent,
                    diag_observations,
                    per_exporter,
                    last_signal_at,
                    scraped_at: now,
                    unreachable: true,
                }));
            }
        }

        tokio::select! {
            () = tokio::time::sleep(opts.scrape_interval) => {}
            _ = &mut shutdown_rx => return,
        }
    }
}

enum ScrapeOutcome {
    Ok {
        received: SignalCounts,
        sent: SignalCounts,
        diag_observations: DiagObservations,
        per_exporter: HashMap<String, ExporterCounts>,
    },
    Unreachable,
}

async fn scrape_once(client: &reqwest::Client, url: &str) -> ScrapeOutcome {
    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::trace!(error = %e, url, "metrics endpoint unreachable");
            return ScrapeOutcome::Unreachable;
        }
    };
    if !response.status().is_success() {
        tracing::trace!(status = %response.status(), url, "metrics endpoint non-200");
        return ScrapeOutcome::Unreachable;
    }
    let body = match response.text().await {
        Ok(t) => t,
        Err(e) => {
            tracing::trace!(error = %e, "metrics endpoint body read failed");
            return ScrapeOutcome::Unreachable;
        }
    };
    let (received, sent) = parse_signal_counts(&body);
    let diag_observations = parse_diag_observations(&body);
    let per_exporter = parse_per_exporter_counts(&body);
    ScrapeOutcome::Ok {
        received,
        sent,
        diag_observations,
        per_exporter,
    }
}

/// Extract per-`filter/diag-<suffix>` outgoing counts from the
/// Prometheus exposition body. Returns a map keyed by the suffix
/// (e.g. `"gemini-cli"`) holding per-signal-type counts. Empty when
/// no diag pipeline is configured.
#[must_use]
pub fn parse_diag_observations(body: &str) -> DiagObservations {
    let mut out: DiagObservations = HashMap::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = split_metric_line(line) else {
            continue;
        };
        let name_no_total = name.strip_suffix("_total").unwrap_or(name);
        if name_no_total != "otelcol_processor_outgoing_items" {
            continue;
        }
        // Pull the `processor` and `otel_signal` labels out of the raw
        // line. Count rows whose processor starts with "filter/diag-",
        // routing the value into the matching SignalCounts field.
        let Some(brace_open) = raw.find('{') else { continue };
        let Some(brace_close_rel) = raw[brace_open..].find('}') else { continue };
        let labels = &raw[brace_open + 1..brace_open + brace_close_rel];
        let mut processor_label: Option<&str> = None;
        let mut signal_label: Option<&str> = None;
        for kv in labels.split(',') {
            let kv = kv.trim();
            if let Some(rest) = kv.strip_prefix("processor=\"") {
                processor_label = rest.strip_suffix('"');
            } else if let Some(rest) = kv.strip_prefix("otel_signal=\"") {
                signal_label = rest.strip_suffix('"');
            }
        }
        let Some(processor) = processor_label else { continue };
        let Some(suffix) = processor.strip_prefix("filter/diag-") else {
            continue;
        };
        let Some(signal) = signal_label else { continue };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = if value < 0.0 { 0u64 } else { value.floor() as u64 };
        let entry = out.entry(suffix.to_string()).or_default();
        match signal {
            "traces" => entry.spans = entry.spans.saturating_add(count),
            "metrics" => entry.metric_points = entry.metric_points.saturating_add(count),
            "logs" => entry.log_records = entry.log_records.saturating_add(count),
            _ => {}
        }
    }
    out
}

fn build_client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .no_proxy()
        .build()
        .expect("reqwest client construction is infallible for this configuration")
}

/// Parse a Prometheus exposition body and sum the otelcol receiver /
/// exporter counters across every label-dimension permutation.
///
/// Counters are floats per the Prometheus format spec but represent
/// integer counts in practice; we floor to u64 for the IPC payload.
#[must_use]
pub fn parse_signal_counts(body: &str) -> (SignalCounts, SignalCounts) {
    let mut received = SignalCounts::default();
    let mut sent = SignalCounts::default();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = split_metric_line(line) else {
            continue;
        };
        // Counter values are non-negative integers in practice; floor
        // and saturate to a u64. `as u64` is the explicit conversion;
        // both possible-truncation and sign-loss are intentional for
        // the float→counter mapping the OTel collector emits.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = if value < 0.0 { 0u64 } else { value.floor() as u64 };
        // OTel collector v0.151+ exposes counters with the standard
        // Prometheus `_total` suffix (e.g.
        // `otelcol_receiver_accepted_metric_points_total`). Older
        // collector builds used the bare name. Strip the suffix so we
        // match both conventions without listing every variant.
        let name = name.strip_suffix("_total").unwrap_or(name);
        match name {
            "otelcol_receiver_accepted_spans" => {
                received.spans = received.spans.saturating_add(count);
            }
            "otelcol_receiver_accepted_metric_points" => {
                received.metric_points = received.metric_points.saturating_add(count);
            }
            "otelcol_receiver_accepted_log_records" => {
                received.log_records = received.log_records.saturating_add(count);
            }
            "otelcol_exporter_sent_spans" => {
                sent.spans = sent.spans.saturating_add(count);
            }
            "otelcol_exporter_sent_metric_points" => {
                sent.metric_points = sent.metric_points.saturating_add(count);
            }
            "otelcol_exporter_sent_log_records" => {
                sent.log_records = sent.log_records.saturating_add(count);
            }
            _ => {}
        }
    }
    (received, sent)
}

/// Cumulative success / failure counters for a single `OTel` collector
/// exporter, summed across the three signal types. Used as the input to
/// the per-backend health pill: window deltas drive the color.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExporterCounts {
    pub sent_total: u64,
    pub failed_total: u64,
}

/// Parse the collector's Prometheus dump and return cumulative
/// `(sent, send_failed)` counters keyed by `exporter` label (the `OTel`
/// collector component id, e.g. `otlphttp/openobserve-93eb10f1`).
///
/// Sums across `spans` / `metric_points` / `log_records` — callers
/// downstream (the health-derive function) only care whether *anything*
/// is flowing for a given destination, not which signal type. Skips
/// lines that don't match the two metric families.
///
/// Tolerates both the bare counter names and the v0.151+ `_total`
/// suffix variants, mirroring [`parse_signal_counts`].
pub fn parse_per_exporter_counts(body: &str) -> HashMap<String, ExporterCounts> {
    let mut out: HashMap<String, ExporterCounts> = HashMap::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Quick reject: we only care about the exporter counter families.
        // The full match happens below after label extraction.
        if !line.starts_with("otelcol_exporter_sent_")
            && !line.starts_with("otelcol_exporter_send_failed_")
        {
            continue;
        }
        let Some(brace) = line.find('{') else {
            // Per-exporter counters always have an `exporter=` label; an
            // unlabelled form would have nothing to attribute to.
            continue;
        };
        let Some(close_rel) = line[brace..].find('}') else {
            continue;
        };
        let close = brace + close_rel;
        let name = line[..brace].trim();
        let labels = &line[brace + 1..close];
        let Some(exporter) = extract_exporter_label(labels) else {
            continue;
        };
        // Parse the value after the `}`.
        let after = close + 1;
        let value_start = match line[after..].find(|c: char| !c.is_whitespace()) {
            Some(off) => after + off,
            None => continue,
        };
        let value_end = line[value_start..]
            .find(char::is_whitespace)
            .map_or(line.len(), |off| value_start + off);
        let Ok(value) = line[value_start..value_end].parse::<f64>() else {
            continue;
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = if value < 0.0 { 0u64 } else { value.floor() as u64 };

        let bare_name = name.strip_suffix("_total").unwrap_or(name);
        let entry = out.entry(exporter.to_string()).or_default();
        match bare_name {
            "otelcol_exporter_sent_spans"
            | "otelcol_exporter_sent_metric_points"
            | "otelcol_exporter_sent_log_records" => {
                entry.sent_total = entry.sent_total.saturating_add(count);
            }
            "otelcol_exporter_send_failed_spans"
            | "otelcol_exporter_send_failed_metric_points"
            | "otelcol_exporter_send_failed_log_records" => {
                entry.failed_total = entry.failed_total.saturating_add(count);
            }
            _ => {}
        }
    }
    out
}

/// Pull the value of the `exporter="..."` label from a Prometheus label
/// block. Returns `None` if the label is absent or unquoted. Tolerant of
/// leading/trailing whitespace and other adjacent labels.
fn extract_exporter_label(labels: &str) -> Option<&str> {
    // Look for `exporter=` preceded by `{`, `,`, or start-of-string —
    // avoids matching a hypothetical `myexporter=...` label.
    let mut search_from = 0;
    while let Some(idx) = labels[search_from..].find("exporter=") {
        let abs = search_from + idx;
        let preceded_ok = abs == 0
            || matches!(labels.as_bytes().get(abs - 1), Some(b',' | b' ' | b'\t'));
        if !preceded_ok {
            search_from = abs + "exporter=".len();
            continue;
        }
        let after_eq = abs + "exporter=".len();
        let rest = labels.get(after_eq..)?;
        // Must be a quoted string.
        let rest = rest.strip_prefix('"')?;
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    None
}

/// Saturating subtraction used to compute per-tick deltas across a
/// scrape window. The collector restarts drop counters to zero; a naive
/// `current - prior` would underflow. Returns `current` whenever
/// `prior > current` (treat as a fresh start).
#[must_use]
pub fn delta_clamped(current: u64, prior: u64) -> u64 {
    if current >= prior {
        current - prior
    } else {
        current
    }
}

/// Split a Prometheus metric line into `(name, value)`. Handles both
/// labelled (`name{a="b"} 12`) and unlabelled (`name 12`) shapes;
/// ignores anything else (HELP/TYPE comments, blank lines).
fn split_metric_line(line: &str) -> Option<(&str, f64)> {
    // Find the start of the value: either after `}` (labelled) or the
    // first whitespace (unlabelled).
    let (name_end, value_start) = if let Some(brace) = line.find('{') {
        let close = line[brace..].find('}')? + brace;
        let after = close + 1;
        let value_start = line[after..]
            .find(|c: char| !c.is_whitespace())
            .map(|off| after + off)?;
        (brace, value_start)
    } else {
        let space = line.find(char::is_whitespace)?;
        let value_start = line[space..]
            .find(|c: char| !c.is_whitespace())
            .map(|off| space + off)?;
        (space, value_start)
    };
    let name = line[..name_end].trim();
    if name.is_empty() {
        return None;
    }
    // Trim trailing optional timestamp.
    let value_end = line[value_start..]
        .find(char::is_whitespace)
        .map_or(line.len(), |off| value_start + off);
    let value: f64 = line[value_start..value_end].parse().ok()?;
    Some((name, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unlabelled_counters() {
        let body = "\
# HELP otelcol_receiver_accepted_spans Number of spans accepted\n\
# TYPE otelcol_receiver_accepted_spans counter\n\
otelcol_receiver_accepted_spans 42\n\
";
        let (received, sent) = parse_signal_counts(body);
        assert_eq!(received.spans, 42);
        assert_eq!(sent.spans, 0);
    }

    #[test]
    fn sums_across_label_dimensions() {
        let body = "\
otelcol_receiver_accepted_spans{receiver=\"otlp\",transport=\"grpc\"} 10\n\
otelcol_receiver_accepted_spans{receiver=\"otlp\",transport=\"http\"} 7\n\
otelcol_exporter_sent_spans{exporter=\"otlp/signoz\"} 17\n\
";
        let (received, sent) = parse_signal_counts(body);
        assert_eq!(received.spans, 17);
        assert_eq!(sent.spans, 17);
    }

    #[test]
    fn parses_metric_points_and_log_records_separately() {
        let body = "\
otelcol_receiver_accepted_metric_points{receiver=\"otlp\"} 4\n\
otelcol_receiver_accepted_log_records{receiver=\"otlp\"} 9\n\
otelcol_exporter_sent_metric_points{exporter=\"otlp/signoz\"} 4\n\
otelcol_exporter_sent_log_records{exporter=\"otlp/signoz\"} 9\n\
";
        let (received, sent) = parse_signal_counts(body);
        assert_eq!(received.spans, 0);
        assert_eq!(received.metric_points, 4);
        assert_eq!(received.log_records, 9);
        assert_eq!(sent.metric_points, 4);
        assert_eq!(sent.log_records, 9);
    }

    #[test]
    fn parses_counter_names_with_total_suffix() {
        // OTel collector v0.151+ exposes counters with the standard
        // Prometheus `_total` suffix. The parser must match both the
        // suffixed form and the bare form so the dashboard's "Recent
        // signal" indicator works across collector versions.
        let body = "\
otelcol_receiver_accepted_metric_points_total{receiver=\"otlp\",transport=\"http\"} 11\n\
otelcol_receiver_accepted_log_records_total{receiver=\"otlp\",transport=\"http\"} 8\n\
otelcol_receiver_accepted_spans_total{receiver=\"otlp\",transport=\"http\"} 1\n\
otelcol_exporter_sent_metric_points_total{exporter=\"otlp/signoz\"} 11\n\
otelcol_exporter_sent_log_records_total{exporter=\"otlp/signoz\"} 8\n\
otelcol_exporter_sent_spans_total{exporter=\"otlp/signoz\"} 1\n\
";
        let (received, sent) = parse_signal_counts(body);
        assert_eq!(received.metric_points, 11);
        assert_eq!(received.log_records, 8);
        assert_eq!(received.spans, 1);
        assert_eq!(sent.metric_points, 11);
        assert_eq!(sent.log_records, 8);
        assert_eq!(sent.spans, 1);
    }

    #[test]
    fn ignores_unrelated_metric_families() {
        let body = "\
process_cpu_seconds_total 1.23\n\
otelcol_processor_batch_send_size_count{processor=\"batch\"} 99\n\
otelcol_receiver_accepted_spans{receiver=\"otlp\"} 5\n\
";
        let (received, _sent) = parse_signal_counts(body);
        assert_eq!(received.spans, 5);
    }

    #[test]
    fn handles_float_values_by_flooring() {
        let body = "\
otelcol_receiver_accepted_spans 12.0\n\
otelcol_exporter_sent_spans 12.7\n\
";
        let (received, sent) = parse_signal_counts(body);
        assert_eq!(received.spans, 12);
        assert_eq!(sent.spans, 12);
    }

    #[test]
    fn skips_help_and_type_comments() {
        let body = "\
# HELP otelcol_receiver_accepted_spans help text\n\
# TYPE otelcol_receiver_accepted_spans counter\n\
otelcol_receiver_accepted_spans 1\n\
";
        let (received, _) = parse_signal_counts(body);
        assert_eq!(received.spans, 1);
    }

    #[test]
    fn signal_counts_total_sums_components() {
        let counts = SignalCounts {
            spans: 1,
            metric_points: 2,
            log_records: 3,
        };
        assert_eq!(counts.total(), 6);
    }

    #[test]
    fn handles_blank_and_garbage_lines() {
        let body = "\n  \nthis-is-not-a-metric\notelcol_receiver_accepted_spans 7\n";
        let (received, _) = parse_signal_counts(body);
        assert_eq!(received.spans, 7);
    }

    #[test]
    fn handles_optional_trailing_timestamp() {
        // Prometheus exposition allows `name value timestamp_ms`.
        let body = "otelcol_receiver_accepted_spans 5 1700000000000\n";
        let (received, _) = parse_signal_counts(body);
        assert_eq!(received.spans, 5);
    }

    #[test]
    fn snapshot_serializes_cleanly_without_runtime_only_fields() {
        let snap = MetricsSnapshot {
            received: SignalCounts {
                spans: 1,
                metric_points: 2,
                log_records: 3,
            },
            sent: SignalCounts::default(),
            diag_observations: HashMap::new(),
            per_exporter: HashMap::new(),
            last_signal_at: None,
            scraped_at: Instant::now(),
            unreachable: false,
        };
        let json = serde_json::to_string(&snap).unwrap();
        // `last_signal_at` and `scraped_at` are #[serde(skip)] because
        // tokio::time::Instant has no stable wire representation.
        assert!(!json.contains("last_signal_at"));
        assert!(!json.contains("scraped_at"));
        assert!(json.contains("\"received\""));
        assert!(json.contains("\"unreachable\":false"));
        assert!(json.contains("\"diag_observations\""));
    }

    #[test]
    fn parses_diag_filter_outgoing_per_signal_counters() {
        // Sums otelcol_processor_outgoing_items_total{processor=filter/diag-*}
        // by (harness suffix, signal), routing each `otel_signal` value into
        // the matching SignalCounts field. Ignores non-diag processors.
        let body = "\
otelcol_processor_outgoing_items_total{otel_signal=\"logs\",processor=\"filter/diag-claude-desktop\"} 4\n\
otelcol_processor_outgoing_items_total{otel_signal=\"metrics\",processor=\"filter/diag-claude-desktop\"} 99\n\
otelcol_processor_outgoing_items_total{otel_signal=\"traces\",processor=\"filter/diag-claude-desktop\"} 11\n\
otelcol_processor_outgoing_items_total{otel_signal=\"logs\",processor=\"transform/harness-tag\"} 17\n\
otelcol_processor_outgoing_items_total{otel_signal=\"logs\",processor=\"filter/diag-some-other\"} 2\n\
";
        let map = parse_diag_observations(body);
        let cd = map.get("claude-desktop").expect("claude-desktop counts");
        assert_eq!(cd.log_records, 4);
        assert_eq!(cd.metric_points, 99);
        assert_eq!(cd.spans, 11);
        let some = map.get("some-other").expect("some-other counts");
        assert_eq!(some.log_records, 2);
        assert_eq!(some.metric_points, 0);
        assert_eq!(some.spans, 0);
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn diag_observations_empty_when_no_filter_pipeline_configured() {
        let body = "otelcol_receiver_accepted_spans 5\n";
        assert!(parse_diag_observations(body).is_empty());
    }

    #[test]
    fn per_exporter_counts_attribute_to_their_exporter_label() {
        let body = "\
otelcol_exporter_sent_log_records{exporter=\"otlphttp/openobserve-93eb10f1\"} 42\n\
otelcol_exporter_sent_metric_points{exporter=\"otlphttp/openobserve-93eb10f1\"} 8\n\
otelcol_exporter_sent_spans{exporter=\"otlp/signoz-31fb8e0a\"} 17\n\
otelcol_exporter_send_failed_log_records{exporter=\"otlphttp/opensearch-a7f0880b\"} 5\n\
otelcol_exporter_send_failed_log_records{exporter=\"otlphttp/clickstack-5c3fbe03\"} 2\n\
";
        let map = parse_per_exporter_counts(body);
        let oo = map
            .get("otlphttp/openobserve-93eb10f1")
            .expect("openobserve counts");
        assert_eq!(oo.sent_total, 50);
        assert_eq!(oo.failed_total, 0);
        let sn = map.get("otlp/signoz-31fb8e0a").expect("signoz counts");
        assert_eq!(sn.sent_total, 17);
        assert_eq!(sn.failed_total, 0);
        let os = map.get("otlphttp/opensearch-a7f0880b").expect("opensearch");
        assert_eq!(os.sent_total, 0);
        assert_eq!(os.failed_total, 5);
        let cs = map.get("otlphttp/clickstack-5c3fbe03").expect("clickstack");
        assert_eq!(cs.failed_total, 2);
    }

    #[test]
    fn per_exporter_counts_handle_total_suffix() {
        // v0.151+ collector exposes the `_total` Prom-standard suffix.
        let body = "\
otelcol_exporter_sent_log_records_total{exporter=\"otlphttp/openobserve-93eb10f1\"} 4\n\
otelcol_exporter_send_failed_metric_points_total{exporter=\"otlp/signoz-31fb8e0a\"} 3\n\
";
        let map = parse_per_exporter_counts(body);
        assert_eq!(
            map.get("otlphttp/openobserve-93eb10f1")
                .copied()
                .unwrap_or_default()
                .sent_total,
            4,
        );
        assert_eq!(
            map.get("otlp/signoz-31fb8e0a")
                .copied()
                .unwrap_or_default()
                .failed_total,
            3,
        );
    }

    #[test]
    fn per_exporter_counts_ignore_unlabelled_lines() {
        // Defensive: an unlabelled exporter counter has no attributable
        // destination, so it must be dropped (the aggregate parser
        // `parse_signal_counts` already counts it for the global view).
        let body = "otelcol_exporter_sent_log_records 9\n";
        assert!(parse_per_exporter_counts(body).is_empty());
    }

    #[test]
    fn per_exporter_counts_ignore_non_exporter_families() {
        let body = "\
otelcol_receiver_accepted_log_records{receiver=\"otlp\"} 12\n\
otelcol_processor_outgoing_items{processor=\"batch\"} 7\n\
otelcol_exporter_sent_log_records{exporter=\"otlphttp/openobserve-93eb10f1\"} 3\n\
";
        let map = parse_per_exporter_counts(body);
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get("otlphttp/openobserve-93eb10f1")
                .copied()
                .unwrap_or_default()
                .sent_total,
            3,
        );
    }

    #[test]
    fn delta_clamped_handles_collector_restart() {
        // Normal forward progress.
        assert_eq!(delta_clamped(10, 7), 3);
        // No change.
        assert_eq!(delta_clamped(7, 7), 0);
        // Counter reset (collector restart) — must clamp to `current`,
        // never underflow into a huge positive number.
        assert_eq!(delta_clamped(2, 100), 2);
        // Both zero.
        assert_eq!(delta_clamped(0, 0), 0);
    }

    #[test]
    fn extract_exporter_label_handles_position_variants() {
        // First label.
        assert_eq!(
            extract_exporter_label("exporter=\"otlp/signoz-31fb8e0a\""),
            Some("otlp/signoz-31fb8e0a"),
        );
        // Middle label.
        assert_eq!(
            extract_exporter_label("otel_signal=\"logs\",exporter=\"otlp/x\",extra=\"y\""),
            Some("otlp/x"),
        );
        // Trailing label.
        assert_eq!(
            extract_exporter_label("otel_signal=\"logs\",exporter=\"otlp/x\""),
            Some("otlp/x"),
        );
        // Adversarial prefix that should NOT match `exporter=`.
        assert_eq!(extract_exporter_label("myexporter=\"foo\""), None);
        // Absent.
        assert_eq!(extract_exporter_label("receiver=\"otlp\""), None);
    }
}
