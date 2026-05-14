//! Sprint 6 PR 1 IPC surface: collector status, metrics snapshot, log
//! tail, and the matching Tauri events.
//!
//! ### Wire format choices
//!
//! Internal types use `tokio::time::Instant` for staleness math —
//! monotonic, immune to wall-clock skew. The wire format converts
//! every `Instant` to a `u64` of milliseconds since the call landed
//! so the TS side gets a stable JSON shape. Snapshots also include
//! the current monotonic "now" baseline (`evaluatedAtMsAgo: 0`) so
//! the dashboard can render "10s ago" without a second IPC roundtrip.
//!
//! ### Event emission
//!
//! Two background tasks (started in [`spawn_event_pumps`]) subscribe
//! to the long-lived [`SupervisorChannels`] / [`MetricsTapHandle`]
//! and forward updates to the `WebView` via Tauri's `emit` channel:
//!
//! - `collector-state` — every `CollectorState` transition.
//! - `metrics-snapshot` — every metrics tap publication (~5s).
//! - `collector-log`   — per-line stdout/stderr from the supervised
//!   child, batched into 100ms windows so a startup line spike
//!   doesn't slam the IPC bridge.
//!
//! The pumps live for the lifetime of the `AppHandle`. Both subscribe
//! through the long-lived channels, so no resubscription is needed
//! across `reload_collector`.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::Instant;

use crate::collector::{
    CollectorLogLine, CollectorState, MetricsSnapshot, MetricsTapHandle, OverallHealth,
    SignalCounts, SupervisorChannels, derive_overall_health,
};

use super::IpcError;

pub const EVENT_COLLECTOR_STATE: &str = "collector-state";
pub const EVENT_METRICS_SNAPSHOT: &str = "metrics-snapshot";
pub const EVENT_COLLECTOR_LOG: &str = "collector-log";

/// JSON-friendly mirror of [`CollectorState`]. Discriminator-tagged so
/// the TS side can branch by `kind`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CollectorRunState {
    Idle,
    Starting { pid: u32 },
    Running { pid: u32, restarts: u32 },
    Crashed { restarts: u32 },
    Stopping,
    Stopped,
    Failed { reason: String },
}

impl From<&CollectorState> for CollectorRunState {
    fn from(s: &CollectorState) -> Self {
        match s {
            CollectorState::Idle => Self::Idle,
            CollectorState::Starting { pid } => Self::Starting { pid: *pid },
            CollectorState::Running { pid, restarts } => Self::Running {
                pid: *pid,
                restarts: *restarts,
            },
            CollectorState::Crashed { restarts } => Self::Crashed {
                restarts: *restarts,
            },
            CollectorState::Stopping => Self::Stopping,
            CollectorState::Stopped => Self::Stopped,
            CollectorState::Failed { reason } => Self::Failed {
                reason: reason.clone(),
            },
        }
    }
}

/// Wire payload returned by `get_collector_status`. Includes the path
/// to the on-disk log file so the React side can fetch the initial
/// tail without going through a separate accessor command.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorStatus {
    pub state: CollectorRunState,
    pub log_path: String,
}

/// Wire mirror of [`MetricsSnapshot`]. Instants become "ms ago"
/// scalars, computed at IPC handler time so TS doesn't need to know
/// about monotonic clocks.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSnapshotWire {
    pub received: SignalCountsWire,
    pub sent: SignalCountsWire,
    /// Milliseconds since the most recent receiver-counter increment.
    /// `null` when no signal has been observed yet.
    pub last_signal_ms_ago: Option<u64>,
    /// Milliseconds since the most recent successful scrape.
    pub scraped_ms_ago: u64,
    /// True when the scrape couldn't reach `:8888/metrics` (user
    /// customised the YAML to drop the telemetry block, etc.).
    pub unreachable: bool,
    /// Convenience: derived green/amber/red, identical truth table to
    /// the tray icon. Saves the dashboard from re-deriving it client-
    /// side; the TS twin still does so for unit-test parity.
    pub overall_health: OverallHealth,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalCountsWire {
    pub spans: u64,
    pub metric_points: u64,
    pub log_records: u64,
}

impl From<&SignalCounts> for SignalCountsWire {
    fn from(c: &SignalCounts) -> Self {
        Self {
            spans: c.spans,
            metric_points: c.metric_points,
            log_records: c.log_records,
        }
    }
}

/// One historical log line as returned by `get_collector_log_tail`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorLogLineWire {
    pub stream: String,
    pub line: String,
}

/// Response shape for `get_collector_log_tail`. `byte_offset` is the
/// position of the cursor at the moment the read returned; the React
/// side filters live `collector-log` events to lines that arrived
/// after this offset to avoid double-display. Live events also carry
/// a `byteOffset` field so the matchup is exact.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorLogTailResponse {
    pub lines: Vec<CollectorLogLineWire>,
    pub byte_offset: u64,
}

/// Live-event payload for `collector-log`. The `byteOffset` here is
/// the file offset just after the line was appended, so a tail fetch
/// can be threaded with the live stream by ignoring events whose
/// offset ≤ the tail's reported offset.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorLogEvent {
    pub stream: String,
    pub line: String,
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn get_collector_status(app: tauri::AppHandle) -> Result<CollectorStatus, IpcError> {
    let channels = app.state::<SupervisorChannels>();
    let state = channels.subscribe_state().borrow().clone();
    let log_path = crate::collector_log_path(&app)
        .map_err(|e| IpcError::Internal {
            reason: format!("could not resolve collector log path: {e}"),
        })?;
    Ok(CollectorStatus {
        state: (&state).into(),
        log_path: log_path.to_string_lossy().into_owned(),
    })
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn get_metrics_snapshot(
    app: tauri::AppHandle,
) -> Result<Option<MetricsSnapshotWire>, IpcError> {
    let channels = app.state::<SupervisorChannels>();
    let metrics = app.state::<MetricsTapHandle>();
    let collector_state = channels.subscribe_state().borrow().clone();
    let snapshot = metrics.latest();
    Ok(snapshot.map(|s| {
        let now = Instant::now();
        snapshot_to_wire(&s, &collector_state, now)
    }))
}

/// Sprint 6 PR 2 dev hatch — bypass `derive_overall_health` and force
/// the tray to a specific colour. Used by Playwright in PR 3 (parity
/// vs the dashboard's `OverallHealthBadge`) and by manual macOS smoke
/// verification (dev-tools
/// `__TAURI_INTERNALS__.invoke('dev_set_tray_color', { color: 'green' })`).
/// Compiled in every build so `tauri::generate_handler!` never sees a
/// missing item, but the body is a no-op in release builds — the
/// command is effectively unreachable from a packaged app because
/// devtools is debug-only.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn dev_set_tray_color(app: tauri::AppHandle, color: OverallHealth) -> Result<(), IpcError> {
    #[cfg(debug_assertions)]
    {
        crate::tray::force_color(&app, color);
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = app;
        let _ = color;
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn get_collector_log_tail(
    app: tauri::AppHandle,
    lines: u32,
) -> Result<CollectorLogTailResponse, IpcError> {
    let log_path = crate::collector_log_path(&app)
        .map_err(|e| IpcError::Internal {
            reason: format!("could not resolve collector log path: {e}"),
        })?;
    tauri::async_runtime::block_on(read_log_tail(log_path, lines as usize))
}

async fn read_log_tail(
    path: PathBuf,
    line_budget: usize,
) -> Result<CollectorLogTailResponse, IpcError> {
    use tokio::fs::File;
    let file = match File::open(&path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CollectorLogTailResponse {
                lines: Vec::new(),
                byte_offset: 0,
            });
        }
        Err(e) => {
            return Err(IpcError::Io {
                path: path.display().to_string(),
                reason: e.to_string(),
            });
        }
    };
    let reader = BufReader::new(file);
    let mut lines_acc: Vec<CollectorLogLineWire> = Vec::new();
    let mut byte_offset: u64 = 0;
    let mut iter = reader.lines();
    while let Some(line) = iter.next_line().await.map_err(|e| IpcError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })? {
        // +1 for the newline trimmed by next_line.
        byte_offset = byte_offset.saturating_add(line.len() as u64 + 1);
        lines_acc.push(CollectorLogLineWire {
            stream: "log".into(),
            line,
        });
        if lines_acc.len() > line_budget {
            // Slide the window: keep only the last `line_budget` lines.
            let drop = lines_acc.len() - line_budget;
            lines_acc.drain(0..drop);
        }
    }
    Ok(CollectorLogTailResponse {
        lines: lines_acc,
        byte_offset,
    })
}

/// Convert an internal `MetricsSnapshot` to the wire format. Keeping
/// this separate from [`MetricsSnapshot`] itself lets the storage type
/// stay monotonic-clock based for staleness math while the IPC surface
/// gets the JSON-friendly "ms ago" deltas the dashboard wants.
#[must_use]
pub fn snapshot_to_wire(
    snap: &MetricsSnapshot,
    collector_state: &CollectorState,
    now: Instant,
) -> MetricsSnapshotWire {
    let last_signal_ms_ago = snap.last_signal_at.map(|at| {
        u64::try_from(now.saturating_duration_since(at).as_millis())
            .unwrap_or(u64::MAX)
    });
    let scraped_ms_ago = u64::try_from(now.saturating_duration_since(snap.scraped_at).as_millis())
        .unwrap_or(u64::MAX);
    let overall_health = derive_overall_health(collector_state, Some(snap), 0, now);
    MetricsSnapshotWire {
        received: (&snap.received).into(),
        sent: (&snap.sent).into(),
        last_signal_ms_ago,
        scraped_ms_ago,
        unreachable: snap.unreachable,
        overall_health,
    }
}

/// Spawn the background event pumps that forward state/metrics/log
/// updates to the `WebView`. Idempotent in spirit — call once during
/// app `setup` after the `SupervisorChannels` and `MetricsTapHandle`
/// have been registered.
pub fn spawn_event_pumps<R: Runtime>(app: &AppHandle<R>) {
    let app_state = app.clone();
    let app_metrics = app.clone();
    let app_logs = app.clone();

    // Collector state pump.
    tauri::async_runtime::spawn(async move {
        let channels = app_state.state::<SupervisorChannels>().inner().clone();
        let mut rx = channels.subscribe_state();
        // Send the seed so a freshly-mounted dashboard has data right
        // away rather than waiting for the first transition.
        let initial: CollectorRunState = (&*rx.borrow()).into();
        let _ = app_state.emit(EVENT_COLLECTOR_STATE, initial);
        loop {
            if rx.changed().await.is_err() {
                return;
            }
            let payload: CollectorRunState = (&*rx.borrow()).into();
            let _ = app_state.emit(EVENT_COLLECTOR_STATE, payload);
        }
    });

    // Metrics snapshot pump.
    tauri::async_runtime::spawn(async move {
        let metrics = app_metrics.state::<MetricsTapHandle>().inner().sender();
        let mut rx = metrics.subscribe();
        // Skip the initial `None`; emit once a real snapshot lands.
        loop {
            if rx.changed().await.is_err() {
                return;
            }
            let snapshot = rx.borrow().clone();
            let Some(snap) = snapshot else { continue };
            let collector_state = app_metrics
                .state::<SupervisorChannels>()
                .subscribe_state()
                .borrow()
                .clone();
            let payload = snapshot_to_wire(&snap, &collector_state, Instant::now());
            let _ = app_metrics.emit(EVENT_METRICS_SNAPSHOT, payload);
        }
    });

    // Collector log pump. Coalesces lines arriving within ~100ms into a
    // single emit batch so a startup line spike doesn't fan out into
    // dozens of WebView events.
    tauri::async_runtime::spawn(async move {
        let channels = app_logs.state::<SupervisorChannels>().inner().clone();
        let mut rx = channels.subscribe_logs();
        loop {
            // Block until at least one line is available.
            let first = match rx.recv().await {
                Ok(line) => line,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            };
            // Drain anything the broadcast buffered behind the first line
            // within a short window so the dashboard sees grouped output.
            let mut batch: Vec<CollectorLogLine> = vec![first];
            let deadline = Instant::now() + Duration::from_millis(100);
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match tokio::time::timeout(remaining, rx.recv()).await {
                    Ok(Ok(line)) => batch.push(line),
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return,
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                        // Lost some lines to the broadcast buffer; the
                        // disk log still has them. Continue draining
                        // whatever's currently buffered.
                    }
                    Err(_) => break,
                }
            }
            for line in batch {
                let _ = app_logs.emit(
                    EVENT_COLLECTOR_LOG,
                    CollectorLogEvent {
                        stream: line.stream.to_string(),
                        line: line.line,
                    },
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn collector_run_state_serializes_idle() {
        let s: CollectorRunState = (&CollectorState::Idle).into();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"idle\""));
    }

    #[test]
    fn collector_run_state_serializes_running_with_fields() {
        let s: CollectorRunState = (&CollectorState::Running { pid: 42, restarts: 1 }).into();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"running\""));
        assert!(json.contains("\"pid\":42"));
        assert!(json.contains("\"restarts\":1"));
    }

    #[test]
    fn snapshot_to_wire_computes_delta_seconds() {
        let now = Instant::now();
        let snap = MetricsSnapshot {
            received: SignalCounts {
                spans: 1,
                metric_points: 0,
                log_records: 0,
            },
            sent: SignalCounts::default(),
            diag_log_records: std::collections::HashMap::new(),
            last_signal_at: Some(now - Duration::from_millis(2_500)),
            scraped_at: now - Duration::from_millis(500),
            unreachable: false,
        };
        let wire = snapshot_to_wire(&snap, &CollectorState::Running { pid: 1, restarts: 0 }, now);
        assert_eq!(wire.last_signal_ms_ago, Some(2_500));
        assert_eq!(wire.scraped_ms_ago, 500);
        assert_eq!(wire.received.spans, 1);
        assert!(!wire.unreachable);
    }

    #[test]
    fn snapshot_to_wire_handles_no_signal_yet() {
        let now = Instant::now();
        let snap = MetricsSnapshot {
            received: SignalCounts::default(),
            sent: SignalCounts::default(),
            diag_log_records: std::collections::HashMap::new(),
            last_signal_at: None,
            scraped_at: now,
            unreachable: false,
        };
        let wire = snapshot_to_wire(&snap, &CollectorState::Running { pid: 1, restarts: 0 }, now);
        assert_eq!(wire.last_signal_ms_ago, None);
        // 0 enabled harnesses + reachable + Running ⇒ green.
        assert!(matches!(wire.overall_health, OverallHealth::Green));
    }

    #[test]
    fn snapshot_to_wire_marks_unreachable_amber() {
        let now = Instant::now();
        let snap = MetricsSnapshot {
            received: SignalCounts::default(),
            sent: SignalCounts::default(),
            diag_log_records: std::collections::HashMap::new(),
            last_signal_at: None,
            scraped_at: now,
            unreachable: true,
        };
        let wire = snapshot_to_wire(&snap, &CollectorState::Running { pid: 1, restarts: 0 }, now);
        assert!(wire.unreachable);
        assert!(matches!(wire.overall_health, OverallHealth::Amber));
    }

    #[test]
    fn collector_run_state_serializes_each_variant() {
        let cases = [
            CollectorState::Idle,
            CollectorState::Starting { pid: 1 },
            CollectorState::Running { pid: 1, restarts: 0 },
            CollectorState::Crashed { restarts: 1 },
            CollectorState::Stopping,
            CollectorState::Stopped,
            CollectorState::Failed { reason: "spawn".into() },
        ];
        for c in cases {
            let wire: CollectorRunState = (&c).into();
            // Serialization should never fail and always include "kind".
            let json = serde_json::to_string(&wire).unwrap();
            assert!(json.contains("\"kind\":"));
        }
    }

    #[test]
    fn signal_counts_wire_round_trips_via_from() {
        let counts = SignalCounts {
            spans: 11,
            metric_points: 22,
            log_records: 33,
        };
        let wire: SignalCountsWire = (&counts).into();
        assert_eq!(wire.spans, 11);
        assert_eq!(wire.metric_points, 22);
        assert_eq!(wire.log_records, 33);
    }

    #[tokio::test]
    async fn read_log_tail_returns_empty_when_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist.log");
        let resp = read_log_tail(missing, 100).await.unwrap();
        assert!(resp.lines.is_empty());
        assert_eq!(resp.byte_offset, 0);
    }

    #[tokio::test]
    async fn read_log_tail_returns_all_lines_under_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("collector.log");
        let body = "line one\nline two\nline three\n";
        tokio::fs::write(&path, body).await.unwrap();
        let resp = read_log_tail(path, 100).await.unwrap();
        assert_eq!(resp.lines.len(), 3);
        assert_eq!(resp.lines[0].line, "line one");
        assert_eq!(resp.lines[2].line, "line three");
        assert_eq!(resp.byte_offset, body.len() as u64);
    }

    #[tokio::test]
    async fn read_log_tail_keeps_only_the_last_n_lines_when_over_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("collector.log");
        // Hardcoded fixture — clippy::format_collect (1.95+) flags
        // map(format!).collect() and fold/write! is overkill for ten lines.
        let body =
            "line 0\nline 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\n";
        tokio::fs::write(&path, body).await.unwrap();
        let resp = read_log_tail(path, 3).await.unwrap();
        assert_eq!(resp.lines.len(), 3);
        // Last three lines retained; earlier ones dropped.
        assert_eq!(resp.lines[0].line, "line 7");
        assert_eq!(resp.lines[1].line, "line 8");
        assert_eq!(resp.lines[2].line, "line 9");
        // Byte offset is the full file length (we walked the whole file
        // even though the window only kept the last N).
        assert_eq!(resp.byte_offset, body.len() as u64);
    }

    #[tokio::test]
    async fn read_log_tail_handles_zero_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("collector.log");
        tokio::fs::write(&path, "x\ny\nz\n").await.unwrap();
        let resp = read_log_tail(path, 0).await.unwrap();
        assert!(resp.lines.is_empty());
        assert_eq!(resp.byte_offset, 6);
    }
}
