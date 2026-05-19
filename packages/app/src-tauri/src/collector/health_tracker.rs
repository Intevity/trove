//! Per-backend health tracker.
//!
//! Fuses two input streams into one `Vec<BackendHealth>` snapshot
//! published on a `watch` channel:
//!
//! - **Pull (color):** every `MetricsSnapshot` from the metrics tap
//!   carries a `per_exporter` map of cumulative sent / failed counters
//!   keyed by OTel collector component id (e.g.
//!   `otlphttp/openobserve-93eb10f1`). Each tick we resolve the
//!   component id back to a `BackendInstance.id` via
//!   [`super::codegen::backend_id_from_component_id`] and call
//!   [`BackendHealthSamples::observe`].
//! - **Push (tooltip text):** every `CollectorLogLine` broadcast by
//!   `tee_stream` runs through [`super::logs::try_parse_exporter_error_line`].
//!   On a match we attribute the error to a backend via the same
//!   resolver and call [`BackendHealthSamples::observe_error_line`].
//!
//! Both inputs converge on `samples_by_id`, guarded by an async Mutex.
//! After each update we project to a `Vec<BackendHealth>` (sorted by
//! `backend_id` for deterministic wire ordering) and `send_replace`
//! into the watch channel. Subscribers (IPC event pump) coalesce on
//! their own.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tauri::async_runtime::JoinHandle;
use tokio::sync::{watch, Mutex};

use crate::app_state::BackendInstance;
use crate::collector::codegen::backend_id_from_component_id;
use crate::collector::derive::{BackendHealth, BackendHealthSamples};
use crate::collector::lifecycle::CollectorLogLine;
use crate::collector::logs::try_parse_exporter_error_line;
use crate::collector::metrics_tap::MetricsSnapshot;

/// Closure returning the current configured backend list. The tracker
/// invokes it on every input event so newly-added or just-removed
/// backends start / stop receiving updates within one tick. lib.rs
/// constructs the closure capturing the AppHandle so it can read
/// `state.json` lazily.
pub type BackendsFetcher = Arc<dyn Fn() -> Vec<BackendInstance> + Send + Sync + 'static>;

/// Public handle held by Tauri as a managed state. Cloneable so IPC
/// commands and event pumps can share access without contention on
/// the inner Mutex.
#[derive(Clone)]
pub struct BackendHealthHandle {
    health_rx: watch::Receiver<Vec<BackendHealth>>,
}

impl BackendHealthHandle {
    /// Latest health snapshot, cloned out of the watch channel.
    #[must_use]
    pub fn latest(&self) -> Vec<BackendHealth> {
        self.health_rx.borrow().clone()
    }

    /// Subscribe to health-snapshot transitions.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<Vec<BackendHealth>> {
        self.health_rx.clone()
    }
}

/// Owns the tracker task. Drop terminates the task at the next event.
pub struct BackendHealthTracker {
    handle: BackendHealthHandle,
    _join: JoinHandle<()>,
}

impl BackendHealthTracker {
    /// Wire the tracker up to its two inputs and spawn it on the Tauri
    /// runtime. `metrics_rx` is the receiver from
    /// `MetricsTapHandle::subscribe`; `log_rx` is the receiver from
    /// `SupervisorChannels::subscribe_logs`; `backends_fn` returns the
    /// current configured backend list each time it's called.
    #[must_use]
    pub fn start(
        mut metrics_rx: watch::Receiver<Option<MetricsSnapshot>>,
        mut log_rx: tokio::sync::broadcast::Receiver<CollectorLogLine>,
        backends_fn: BackendsFetcher,
    ) -> Self {
        let (health_tx, health_rx) = watch::channel::<Vec<BackendHealth>>(Vec::new());
        let samples_by_id: Arc<Mutex<HashMap<String, BackendHealthSamples>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let join = {
            let samples_by_id = samples_by_id.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::select! {
                        biased;
                        changed = metrics_rx.changed() => {
                            if changed.is_err() { return; }
                            let snapshot_opt = metrics_rx.borrow_and_update().clone();
                            let Some(snapshot) = snapshot_opt else { continue };
                            if snapshot.unreachable { continue; }
                            let backends = (backends_fn)();
                            apply_metrics_tick(&samples_by_id, &backends, &snapshot).await;
                            publish(&samples_by_id, &backends, &health_tx).await;
                        }
                        log = log_rx.recv() => {
                            let line = match log {
                                Ok(l) => l,
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(_) => return,
                            };
                            let Some(parsed) = try_parse_exporter_error_line(&line.line) else { continue };
                            let backends = (backends_fn)();
                            let Some(backend_id) = backend_id_from_component_id(&parsed.component_id, &backends).map(str::to_string) else { continue };
                            {
                                let mut map = samples_by_id.lock().await;
                                map.entry(backend_id)
                                    .or_default()
                                    .observe_error_line(Utc::now(), parsed.error);
                            }
                            publish(&samples_by_id, &backends, &health_tx).await;
                        }
                    }
                }
            })
        };

        Self {
            handle: BackendHealthHandle { health_rx },
            _join: join,
        }
    }

    #[must_use]
    pub fn handle(&self) -> BackendHealthHandle {
        self.handle.clone()
    }
}

/// Fold a fresh `MetricsSnapshot` into `samples_by_id`. Iterates
/// `per_exporter` (current scrape), resolves each component id to a
/// backend, and calls `observe` on that backend's samples. Backends
/// without a current `per_exporter` entry retain their prior samples
/// — they'll naturally trim to Gray as the window rolls forward.
async fn apply_metrics_tick(
    samples_by_id: &Mutex<HashMap<String, BackendHealthSamples>>,
    backends: &[BackendInstance],
    snapshot: &MetricsSnapshot,
) {
    let now = snapshot.scraped_at;
    let wall_now = Utc::now();
    let mut map = samples_by_id.lock().await;
    // First, trim every backend's window forward — even those without
    // a per_exporter entry this tick. Otherwise a destination that
    // briefly went silent would freeze at its last known counts forever.
    for samples in map.values_mut() {
        samples.trim_window(now);
    }
    for (component_id, counts) in &snapshot.per_exporter {
        let Some(backend_id) = backend_id_from_component_id(component_id, backends) else {
            continue;
        };
        let entry = map.entry(backend_id.to_string()).or_default();
        entry.observe(now, wall_now, counts.sent_total, counts.failed_total);
    }
    // Drop samples for backends that no longer exist in state.json
    // (user removed the destination). Otherwise stale entries linger
    // in the published snapshot.
    let live_ids: std::collections::HashSet<&str> =
        backends.iter().map(|b| b.id.as_str()).collect();
    map.retain(|id, _| live_ids.contains(id.as_str()));
}

/// Project `samples_by_id` to a `Vec<BackendHealth>` covering every
/// currently-configured backend (even those without any samples — they
/// appear as Gray). Sorted by `backend_id` so subscribers don't see
/// spurious reorderings.
async fn publish(
    samples_by_id: &Mutex<HashMap<String, BackendHealthSamples>>,
    backends: &[BackendInstance],
    tx: &watch::Sender<Vec<BackendHealth>>,
) {
    let map = samples_by_id.lock().await;
    let now = Utc::now();
    let mut out: Vec<BackendHealth> = backends
        .iter()
        .map(|b| {
            map.get(&b.id).map_or_else(
                || BackendHealth::from_samples(b.id.clone(), &BackendHealthSamples::default(), now),
                |s| BackendHealth::from_samples(b.id.clone(), s, now),
            )
        })
        .collect();
    out.sort_by(|a, b| a.backend_id.cmp(&b.backend_id));
    tx.send_replace(out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Instant;

    use crate::app_state::{Backend, SecretRef};
    use crate::collector::derive::BackendHealthStatus;
    use crate::collector::metrics_tap::ExporterCounts;

    fn signoz(id: &str) -> BackendInstance {
        BackendInstance {
            id: id.to_string(),
            label: None,
            backend: Backend::Signoz {
                endpoint: "localhost:14317".into(),
                ingestion_key: SecretRef::for_account(format!("k.{id}")),
            },
        }
    }

    fn snapshot_with(per_exporter: HashMap<String, ExporterCounts>) -> MetricsSnapshot {
        MetricsSnapshot {
            per_exporter,
            ..MetricsSnapshot::default()
        }
    }

    /// First scrape establishes the baseline; one backend, no sample yet.
    /// Then a second scrape with strict-positive sent counts produces a
    /// Green payload for that backend.
    #[tokio::test]
    async fn metrics_tick_walks_backend_from_gray_to_green() {
        let backends = vec![signoz("31fb8e0a-0d50-4636-ab1a-868a4428a092")];
        let samples = Mutex::new(HashMap::new());
        let (tx, mut rx) = watch::channel::<Vec<BackendHealth>>(Vec::new());

        // Baseline tick.
        let mut per = HashMap::new();
        per.insert(
            "otlp/signoz-31fb8e0a".to_string(),
            ExporterCounts { sent_total: 100, failed_total: 0 },
        );
        let snap_1 = snapshot_with(per);
        apply_metrics_tick(&samples, &backends, &snap_1).await;
        publish(&samples, &backends, &tx).await;
        rx.changed().await.unwrap();
        assert_eq!(rx.borrow()[0].status, BackendHealthStatus::Gray);

        // Second tick — counter advanced.
        let mut per = HashMap::new();
        per.insert(
            "otlp/signoz-31fb8e0a".to_string(),
            ExporterCounts { sent_total: 142, failed_total: 0 },
        );
        let mut snap_2 = snapshot_with(per);
        // scraped_at must be strictly later — Instant::now() advances
        // naturally between calls, but make it explicit so the delta
        // logic sees forward progress.
        snap_2.scraped_at = Instant::now() + std::time::Duration::from_secs(5);
        apply_metrics_tick(&samples, &backends, &snap_2).await;
        publish(&samples, &backends, &tx).await;
        rx.changed().await.unwrap();
        assert_eq!(rx.borrow()[0].status, BackendHealthStatus::Green);
        assert_eq!(rx.borrow()[0].window_sent, 42);
    }

    /// A destination removed from state.json must be evicted from the
    /// next published snapshot. Otherwise stale Gray rows linger
    /// indefinitely in the UI.
    #[tokio::test]
    async fn removed_backends_are_evicted_on_next_tick() {
        let signoz_a = signoz("aaaaaaaa-0000-0000-0000-000000000000");
        let signoz_b = signoz("bbbbbbbb-0000-0000-0000-000000000000");
        let samples = Mutex::new(HashMap::new());
        let (tx, mut rx) = watch::channel::<Vec<BackendHealth>>(Vec::new());

        // Both configured initially.
        let mut per = HashMap::new();
        per.insert(
            "otlp/signoz-aaaaaaaa".into(),
            ExporterCounts { sent_total: 1, failed_total: 0 },
        );
        per.insert(
            "otlp/signoz-bbbbbbbb".into(),
            ExporterCounts { sent_total: 1, failed_total: 0 },
        );
        apply_metrics_tick(
            &samples,
            &[signoz_a.clone(), signoz_b.clone()],
            &snapshot_with(per),
        )
        .await;
        publish(&samples, &[signoz_a.clone(), signoz_b.clone()], &tx).await;
        rx.changed().await.unwrap();
        assert_eq!(rx.borrow().len(), 2);

        // User removes signoz_b — next tick passes only signoz_a.
        apply_metrics_tick(&samples, &[signoz_a.clone()], &snapshot_with(HashMap::new())).await;
        publish(&samples, &[signoz_a.clone()], &tx).await;
        rx.changed().await.unwrap();
        let payload = rx.borrow();
        assert_eq!(payload.len(), 1);
        assert_eq!(payload[0].backend_id, signoz_a.id);
    }

    #[tokio::test]
    async fn published_snapshot_is_sorted_by_backend_id() {
        let a = signoz("11111111-0000-0000-0000-000000000000");
        let b = signoz("22222222-0000-0000-0000-000000000000");
        let c = signoz("33333333-0000-0000-0000-000000000000");
        let samples = Mutex::new(HashMap::new());
        let (tx, mut rx) = watch::channel::<Vec<BackendHealth>>(Vec::new());

        // Pass backends in non-sorted order.
        publish(&samples, &[c.clone(), a.clone(), b.clone()], &tx).await;
        rx.changed().await.unwrap();
        let ids: Vec<String> = rx.borrow().iter().map(|h| h.backend_id.clone()).collect();
        assert_eq!(ids, vec![a.id, b.id, c.id]);
    }
}
