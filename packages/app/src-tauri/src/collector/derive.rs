//! Pure derivation: collector run state + metrics tap → overall health.
//!
//! This is the Rust source of truth for the tray icon color. PR 3 ships
//! a TypeScript twin (`packages/app/src/lib/health.ts`) that mirrors the
//! same truth table for the dashboard's `OverallHealthBadge`. Both are
//! pure functions of the same inputs; both have unit tests covering
//! every transition.
//!
//! ## Truth table
//!
//! | Collector state              | Metrics endpoint   | Enabled harnesses | Last signal       | Result |
//! |------------------------------|--------------------|-------------------|-------------------|--------|
//! | `Crashed` / `Failed`         | —                  | —                 | —                 | red    |
//! | `Stopped`                    | —                  | —                 | —                 | red    |
//! | `Idle` / `Starting` / `Stopping` | —              | —                 | —                 | amber  |
//! | `Running`                    | unreachable        | —                 | —                 | amber  |
//! | `Running`                    | reachable          | 0                 | —                 | green  |
//! | `Running`                    | reachable          | ≥ 1               | within 60s        | green  |
//! | `Running`                    | reachable          | ≥ 1               | older than 60s    | amber  |
//! | `Running`                    | reachable          | ≥ 1               | none yet          | amber  |
//!
//! `:8888` unreachable is deliberately mapped to **amber**, not red — a
//! user who customised their YAML to drop the telemetry block hasn't
//! broken the data path, only the introspection.

use std::collections::VecDeque;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use super::lifecycle::CollectorState;
use super::metrics_tap::{delta_clamped, MetricsSnapshot};

/// How long after the most recent receiver-counter increment we still
/// consider telemetry "live". Past this threshold the dashboard turns
/// amber to nudge the user that something stopped flowing.
pub const RECENT_SIGNAL_WINDOW: Duration = Duration::from_secs(60);

/// Overall health rendered on the tray icon and the dashboard badge.
/// Serialised lowercase so the wire format matches the TS Zod enum
/// (`"green" | "amber" | "red"`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverallHealth {
    Green,
    Amber,
    Red,
}

/// Derive overall health from the collector's run state, the latest
/// metrics snapshot (if any), the count of currently-enabled harnesses,
/// and the current monotonic time. Pure: no I/O, no clock reads. Caller
/// supplies `now` so tests are deterministic.
#[must_use]
pub fn derive_overall_health(
    state: &CollectorState,
    metrics: Option<&MetricsSnapshot>,
    enabled_harnesses: usize,
    now: Instant,
) -> OverallHealth {
    match state {
        CollectorState::Crashed { .. }
        | CollectorState::Failed { .. }
        | CollectorState::Stopped => OverallHealth::Red,

        CollectorState::Idle | CollectorState::Starting { .. } | CollectorState::Stopping => {
            OverallHealth::Amber
        }

        CollectorState::Running { .. } => match metrics {
            Some(snap) if snap.unreachable => OverallHealth::Amber,
            Some(snap) => {
                if enabled_harnesses == 0 {
                    return OverallHealth::Green;
                }
                match snap.last_signal_at {
                    Some(at) if now.saturating_duration_since(at) <= RECENT_SIGNAL_WINDOW => {
                        OverallHealth::Green
                    }
                    _ => OverallHealth::Amber,
                }
            }
            // Running but no scrape has come back yet. Treat as amber
            // (transitional) rather than green so a slow-to-warm metrics
            // endpoint isn't reported as healthy.
            None => OverallHealth::Amber,
        },
    }
}

// ----- per-backend health (the Platforms-tab status dot) -----

/// Window over which `BackendHealthSamples` aggregates sent / failed
/// deltas to decide the per-platform pill color. Matches
/// [`RECENT_SIGNAL_WINDOW`] so the global tray and per-platform pill
/// transition on the same timescale.
pub const BACKEND_HEALTH_WINDOW: Duration = RECENT_SIGNAL_WINDOW;

/// Status surfaced for a single configured destination. Mirrors the
/// 4-color `StatusDot` palette on the frontend.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendHealthStatus {
    /// No samples yet — destination was just added or no traffic has
    /// flowed through any harness since startup.
    Gray,
    /// At least one batch sent and zero failures in the last window.
    Green,
    /// Some batches sent and some failed in the same window (retries
    /// recovering, intermittent connectivity).
    Amber,
    /// Failures only — every batch in the window failed, OR samples
    /// have aged out and the most recent observation was a failure.
    Red,
}

/// Rolling per-destination delta samples. Updated each metrics-tap
/// scrape tick by [`BackendHealthSamples::observe`], read by
/// [`derive_backend_health`].
///
/// Held in transient memory only — not serialized to `state.json`. A
/// Trove restart starts everything at Gray and re-derives within one
/// scrape tick.
#[derive(Clone, Debug, Default)]
pub struct BackendHealthSamples {
    /// `(observed_at, sent_delta, failed_delta)` for the last
    /// `BACKEND_HEALTH_WINDOW`. Newest at the front.
    samples: VecDeque<(Instant, u64, u64)>,
    prior_sent_total: u64,
    prior_failed_total: u64,
    has_prior: bool,
    /// Wall-clock copies for the public `BackendHealth` payload — the
    /// UI needs absolute times to render "5 s ago", which monotonic
    /// `Instant` can't express.
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error_msg: Option<String>,
}

impl BackendHealthSamples {
    /// Record a new scrape observation. `current_sent_total` /
    /// `current_failed_total` are cumulative counters from the Prom
    /// scrape; this fn computes the delta vs the previous tick. The
    /// first call after construction (or after a counter reset) stores
    /// the baseline without contributing a sample.
    pub fn observe(
        &mut self,
        now: Instant,
        wall_now: DateTime<Utc>,
        current_sent_total: u64,
        current_failed_total: u64,
    ) {
        if !self.has_prior {
            self.prior_sent_total = current_sent_total;
            self.prior_failed_total = current_failed_total;
            self.has_prior = true;
            return;
        }
        let sent_delta = delta_clamped(current_sent_total, self.prior_sent_total);
        let failed_delta = delta_clamped(current_failed_total, self.prior_failed_total);
        if sent_delta > 0 {
            self.last_success_at = Some(wall_now);
        }
        if failed_delta > 0 {
            self.last_error_at = Some(wall_now);
        }
        self.samples.push_front((now, sent_delta, failed_delta));
        self.prior_sent_total = current_sent_total;
        self.prior_failed_total = current_failed_total;
        self.trim_window(now);
    }

    /// Drop samples older than `BACKEND_HEALTH_WINDOW`. Always safe to
    /// call (no-op when nothing to trim).
    pub fn trim_window(&mut self, now: Instant) {
        while let Some((t, _, _)) = self.samples.back() {
            if now.saturating_duration_since(*t) > BACKEND_HEALTH_WINDOW {
                self.samples.pop_back();
            } else {
                break;
            }
        }
    }

    /// `(sent, failed)` summed across the in-window samples.
    #[must_use]
    pub fn window_totals(&self) -> (u64, u64) {
        self.samples
            .iter()
            .fold((0u64, 0u64), |(s, f), (_, ds, df)| {
                (s.saturating_add(*ds), f.saturating_add(*df))
            })
    }

    /// Record a non-counter signal: a stderr error line attributed to
    /// this destination. Drives the tooltip text but not the pill
    /// color (counters do that — see [`derive_backend_health`]).
    pub fn observe_error_line(&mut self, wall_now: DateTime<Utc>, msg: String) {
        self.last_error_at = Some(wall_now);
        self.last_error_msg = Some(msg);
    }
}

/// Pure derivation of the per-destination pill color from rolling
/// samples plus the most-recent stderr error event. Truth table:
///
/// | has_successes | has_failures | result |
/// |---------------|--------------|--------|
/// | false         | false        | Gray   |
/// | true          | false        | Green  |
/// | true          | true         | Amber  |
/// | false         | true         | Red    |
///
/// `has_successes` is `window_sent > 0`. `has_failures` is `window_failed > 0`
/// **OR** `last_error_at` is within `BACKEND_HEALTH_WINDOW` of `now` — the
/// OR branch matters because the collector's `send_failed_*` counter only
/// increments when the retry queue gives up; an exporter that's been
/// failing every batch but is still inside its backoff budget shows
/// `sent=0, failed=0` in the Prom dump even though stderr is repeatedly
/// logging `Exporting failed`. Without the stderr fallback, the pill
/// would silently stay Gray on a destination that's actually broken.
///
/// Caller is expected to have called
/// [`BackendHealthSamples::trim_window`] (or `observe`) with the
/// current monotonic time before invoking this.
#[must_use]
pub fn derive_backend_health(
    samples: &BackendHealthSamples,
    now: DateTime<Utc>,
) -> BackendHealthStatus {
    let (window_sent, window_failed) = samples.window_totals();
    let has_successes = window_sent > 0;
    let recent_error = matches!(
        samples.last_error_at,
        Some(t) if now.signed_duration_since(t).num_seconds() <= BACKEND_HEALTH_WINDOW.as_secs() as i64,
    );
    let has_failures = window_failed > 0 || recent_error;
    match (has_successes, has_failures) {
        (false, false) => BackendHealthStatus::Gray,
        (true, false) => BackendHealthStatus::Green,
        (true, true) => BackendHealthStatus::Amber,
        (false, true) => BackendHealthStatus::Red,
    }
}

/// Public per-destination health payload. Sent over IPC to the
/// frontend; deserialised by the `useBackendHealth` hook.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackendHealth {
    pub backend_id: String,
    pub status: BackendHealthStatus,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error_msg: Option<String>,
    pub window_sent: u64,
    pub window_failed: u64,
}

impl BackendHealth {
    /// Project a `BackendHealthSamples` view into the public payload.
    /// `now` is wall-clock; needed by `derive_backend_health` to decide
    /// whether the most-recent stderr error is still within window.
    #[must_use]
    pub fn from_samples(
        backend_id: String,
        samples: &BackendHealthSamples,
        now: DateTime<Utc>,
    ) -> Self {
        let (window_sent, window_failed) = samples.window_totals();
        Self {
            backend_id,
            status: derive_backend_health(samples, now),
            last_success_at: samples.last_success_at,
            last_error_at: samples.last_error_at,
            last_error_msg: samples.last_error_msg.clone(),
            window_sent,
            window_failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::metrics_tap::SignalCounts;
    use super::*;

    fn snapshot_with(last_signal_at: Option<Instant>, unreachable: bool) -> MetricsSnapshot {
        MetricsSnapshot {
            received: SignalCounts::default(),
            sent: SignalCounts::default(),
            diag_observations: std::collections::HashMap::new(),
            per_exporter: std::collections::HashMap::new(),
            last_signal_at,
            scraped_at: Instant::now(),
            unreachable,
        }
    }

    #[test]
    fn crashed_is_red() {
        assert_eq!(
            derive_overall_health(
                &CollectorState::Crashed { restarts: 1 },
                None,
                0,
                Instant::now(),
            ),
            OverallHealth::Red
        );
    }

    #[test]
    fn failed_is_red() {
        assert_eq!(
            derive_overall_health(
                &CollectorState::Failed { reason: "spawn failed".into() },
                None,
                0,
                Instant::now(),
            ),
            OverallHealth::Red
        );
    }

    #[test]
    fn stopped_is_red() {
        assert_eq!(
            derive_overall_health(&CollectorState::Stopped, None, 0, Instant::now()),
            OverallHealth::Red
        );
    }

    #[test]
    fn idle_is_amber() {
        assert_eq!(
            derive_overall_health(&CollectorState::Idle, None, 0, Instant::now()),
            OverallHealth::Amber
        );
    }

    #[test]
    fn starting_is_amber() {
        assert_eq!(
            derive_overall_health(
                &CollectorState::Starting { pid: 42 },
                None,
                0,
                Instant::now(),
            ),
            OverallHealth::Amber
        );
    }

    #[test]
    fn stopping_is_amber() {
        assert_eq!(
            derive_overall_health(&CollectorState::Stopping, None, 0, Instant::now()),
            OverallHealth::Amber
        );
    }

    #[test]
    fn running_with_no_metrics_yet_is_amber() {
        assert_eq!(
            derive_overall_health(
                &CollectorState::Running { pid: 1, restarts: 0 },
                None,
                0,
                Instant::now(),
            ),
            OverallHealth::Amber
        );
    }

    #[test]
    fn running_with_metrics_endpoint_unreachable_is_amber() {
        let snap = snapshot_with(None, true);
        assert_eq!(
            derive_overall_health(
                &CollectorState::Running { pid: 1, restarts: 0 },
                Some(&snap),
                3,
                Instant::now(),
            ),
            OverallHealth::Amber
        );
    }

    #[test]
    fn running_with_zero_harnesses_is_green_even_without_signal() {
        let snap = snapshot_with(None, false);
        assert_eq!(
            derive_overall_health(
                &CollectorState::Running { pid: 1, restarts: 0 },
                Some(&snap),
                0,
                Instant::now(),
            ),
            OverallHealth::Green
        );
    }

    #[test]
    fn running_with_recent_signal_is_green() {
        let now = Instant::now();
        let snap = snapshot_with(Some(now - Duration::from_secs(5)), false);
        assert_eq!(
            derive_overall_health(
                &CollectorState::Running { pid: 1, restarts: 0 },
                Some(&snap),
                2,
                now,
            ),
            OverallHealth::Green
        );
    }

    #[test]
    fn running_with_stale_signal_is_amber() {
        let now = Instant::now();
        let snap = snapshot_with(Some(now - Duration::from_secs(120)), false);
        assert_eq!(
            derive_overall_health(
                &CollectorState::Running { pid: 1, restarts: 0 },
                Some(&snap),
                2,
                now,
            ),
            OverallHealth::Amber
        );
    }

    #[test]
    fn running_with_no_signal_yet_and_enabled_harnesses_is_amber() {
        let snap = snapshot_with(None, false);
        assert_eq!(
            derive_overall_health(
                &CollectorState::Running { pid: 1, restarts: 0 },
                Some(&snap),
                1,
                Instant::now(),
            ),
            OverallHealth::Amber
        );
    }

    #[test]
    fn signal_at_exactly_window_boundary_is_green() {
        let now = Instant::now();
        let snap = snapshot_with(Some(now - RECENT_SIGNAL_WINDOW), false);
        assert_eq!(
            derive_overall_health(
                &CollectorState::Running { pid: 1, restarts: 0 },
                Some(&snap),
                1,
                now,
            ),
            OverallHealth::Green,
        );
    }

    #[test]
    fn overall_health_serializes_to_lowercase_string() {
        assert_eq!(
            serde_json::to_string(&OverallHealth::Green).unwrap(),
            "\"green\"",
        );
        assert_eq!(
            serde_json::to_string(&OverallHealth::Amber).unwrap(),
            "\"amber\"",
        );
        assert_eq!(
            serde_json::to_string(&OverallHealth::Red).unwrap(),
            "\"red\"",
        );
    }

    // ----- BackendHealthSamples / derive_backend_health -----

    fn observe_at(samples: &mut BackendHealthSamples, offset: Duration, sent: u64, failed: u64) {
        let now = Instant::now() + offset;
        let wall = Utc::now() + chrono::Duration::from_std(offset).unwrap();
        samples.observe(now, wall, sent, failed);
    }

    #[test]
    fn fresh_samples_are_gray() {
        let samples = BackendHealthSamples::default();
        assert_eq!(derive_backend_health(&samples, Utc::now()), BackendHealthStatus::Gray);
    }

    #[test]
    fn first_observation_is_a_baseline_and_emits_nothing() {
        let mut samples = BackendHealthSamples::default();
        observe_at(&mut samples, Duration::ZERO, 100, 0);
        // The first scrape just records the cumulative baseline — no
        // delta sample yet, so the pill stays Gray.
        assert_eq!(derive_backend_health(&samples, Utc::now()), BackendHealthStatus::Gray);
        assert_eq!(samples.window_totals(), (0, 0));
    }

    #[test]
    fn successful_send_after_baseline_turns_green() {
        let mut samples = BackendHealthSamples::default();
        observe_at(&mut samples, Duration::ZERO, 100, 0);
        observe_at(&mut samples, Duration::from_secs(5), 142, 0);
        assert_eq!(derive_backend_health(&samples, Utc::now()), BackendHealthStatus::Green);
        let (sent, failed) = samples.window_totals();
        assert_eq!(sent, 42);
        assert_eq!(failed, 0);
    }

    #[test]
    fn all_failures_in_window_turns_red() {
        let mut samples = BackendHealthSamples::default();
        observe_at(&mut samples, Duration::ZERO, 0, 0);
        observe_at(&mut samples, Duration::from_secs(5), 0, 7);
        observe_at(&mut samples, Duration::from_secs(10), 0, 11);
        assert_eq!(derive_backend_health(&samples, Utc::now()), BackendHealthStatus::Red);
        // Deltas: 7 (cumulative 0→7) + 4 (cumulative 7→11) = 11.
        assert_eq!(samples.window_totals(), (0, 11));
    }

    #[test]
    fn mixed_success_and_failure_turns_amber() {
        let mut samples = BackendHealthSamples::default();
        observe_at(&mut samples, Duration::ZERO, 0, 0);
        observe_at(&mut samples, Duration::from_secs(5), 12, 3);
        assert_eq!(derive_backend_health(&samples, Utc::now()), BackendHealthStatus::Amber);
    }

    #[test]
    fn samples_outside_window_are_dropped_and_state_falls_back_to_gray() {
        let mut samples = BackendHealthSamples::default();
        observe_at(&mut samples, Duration::ZERO, 0, 0);
        observe_at(&mut samples, Duration::from_secs(5), 42, 0);
        assert_eq!(derive_backend_health(&samples, Utc::now()), BackendHealthStatus::Green);
        // Now advance past the window with no new observations — trim
        // should evict the only sample and the pill should revert to
        // Gray, NOT stay Green.
        let after_window =
            Instant::now() + Duration::from_secs(5) + BACKEND_HEALTH_WINDOW + Duration::from_secs(1);
        samples.trim_window(after_window);
        assert_eq!(derive_backend_health(&samples, Utc::now()), BackendHealthStatus::Gray);
        assert_eq!(samples.window_totals(), (0, 0));
    }

    #[test]
    fn counter_reset_is_treated_as_a_fresh_baseline_not_a_huge_delta() {
        let mut samples = BackendHealthSamples::default();
        observe_at(&mut samples, Duration::ZERO, 1_000, 0);
        observe_at(&mut samples, Duration::from_secs(5), 1_050, 0);
        // Collector restarts — counters drop to 0 — observe a small
        // value below the prior cumulative total.
        observe_at(&mut samples, Duration::from_secs(10), 4, 0);
        // The reset tick's delta should clamp to `current` (4), not
        // wrap around to a massive positive number.
        let (sent, _) = samples.window_totals();
        // The two real samples were 50 + 4 = 54.
        assert_eq!(sent, 54);
    }

    #[test]
    fn observe_error_line_flips_pill_to_red_when_no_success_signal() {
        // OTel collector exporters in retry-limbo don't increment
        // send_failed_* counters — only the stderr stream surfaces the
        // failure. Without the stderr-driven Red branch, a dead
        // destination would show Gray indistinguishable from "never
        // used", which is the exact bug the user hit in dev validation.
        let mut samples = BackendHealthSamples::default();
        samples.observe_error_line(Utc::now(), "connection refused".into());
        assert_eq!(
            derive_backend_health(&samples, Utc::now()),
            BackendHealthStatus::Red,
        );
        assert_eq!(
            samples.last_error_msg.as_deref(),
            Some("connection refused"),
        );
    }

    #[test]
    fn stale_stderr_error_outside_window_does_not_keep_pill_red() {
        // A failure from 5 minutes ago should NOT pin the pill red
        // forever — the window logic applies to stderr signal too.
        let mut samples = BackendHealthSamples::default();
        let five_min_ago = Utc::now() - chrono::Duration::seconds(5 * 60);
        samples.observe_error_line(five_min_ago, "ancient error".into());
        assert_eq!(
            derive_backend_health(&samples, Utc::now()),
            BackendHealthStatus::Gray,
        );
    }

    #[test]
    fn backend_health_payload_serializes_to_camel_case() {
        let mut samples = BackendHealthSamples::default();
        observe_at(&mut samples, Duration::ZERO, 0, 0);
        observe_at(&mut samples, Duration::from_secs(5), 10, 0);
        let payload = BackendHealth::from_samples("abc-123".into(), &samples, Utc::now());
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["backendId"], "abc-123");
        assert_eq!(json["status"], "green");
        assert_eq!(json["windowSent"], 10);
        assert_eq!(json["windowFailed"], 0);
    }
}
