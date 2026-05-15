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

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use super::lifecycle::CollectorState;
use super::metrics_tap::MetricsSnapshot;

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
}
