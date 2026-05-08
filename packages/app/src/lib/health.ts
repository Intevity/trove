import type { CollectorRunState, MetricsSnapshotWire, OverallHealth } from '@trove/shared';

/**
 * Window inside which the most recent receiver-counter increment
 * counts as "live" telemetry. Past this threshold the dashboard turns
 * amber. Locked-step with the Rust constant in
 * `packages/app/src-tauri/src/collector/derive.rs` (`RECENT_SIGNAL_WINDOW`).
 */
export const RECENT_SIGNAL_WINDOW_MS = 60 * 1000;

/**
 * Pure derivation: collector run state + latest metrics snapshot →
 * overall green/amber/red. TS twin of Rust `derive_overall_health`;
 * fed identical inputs, both must agree.
 *
 * Truth table (mirrors `derive.rs`):
 *
 * | Collector state              | Metrics endpoint   | Enabled harnesses | Last signal       | Result |
 * |------------------------------|--------------------|-------------------|-------------------|--------|
 * | crashed / failed / stopped   | —                  | —                 | —                 | red    |
 * | idle / starting / stopping   | —                  | —                 | —                 | amber  |
 * | running                      | unreachable        | —                 | —                 | amber  |
 * | running                      | reachable          | 0                 | —                 | green  |
 * | running                      | reachable          | ≥ 1               | within 60 s       | green  |
 * | running                      | reachable          | ≥ 1               | older than 60 s   | amber  |
 * | running                      | reachable          | ≥ 1               | none yet          | amber  |
 * | running                      | no scrape yet      | —                 | —                 | amber  |
 *
 * `:8888` unreachable maps to amber (not red): a user-customised
 * collector YAML that drops the telemetry block hasn't broken the
 * data path, only Trove's introspection of it.
 */
export function deriveOverallHealth(
  state: CollectorRunState,
  metrics: MetricsSnapshotWire | null | undefined,
  enabledHarnessCount: number,
): OverallHealth {
  switch (state.kind) {
    case 'crashed':
    case 'failed':
    case 'stopped':
      return 'red';
    case 'idle':
    case 'starting':
    case 'stopping':
      return 'amber';
    case 'running':
      // No scrape has come back yet. Render amber — slow-to-warm
      // metrics endpoints shouldn't be reported as healthy.
      if (!metrics) return 'amber';
      if (metrics.unreachable) return 'amber';
      if (enabledHarnessCount === 0) return 'green';
      if (metrics.lastSignalMsAgo === null) return 'amber';
      return metrics.lastSignalMsAgo <= RECENT_SIGNAL_WINDOW_MS ? 'green' : 'amber';
  }
}

/** User-facing label per state, used by the OverallHealthBadge and the
 *  status text in the tray menu (which has its own Rust copy in
 *  `tray.rs::format_status_text`). Keeping the strings in TS lets the
 *  dashboard render them while keeping the tray copy stable across
 *  language. */
export function overallHealthLabel(health: OverallHealth): string {
  switch (health) {
    case 'green':
      return 'Healthy';
    case 'amber':
      return 'Awaiting telemetry';
    case 'red':
      return 'Sidecar down';
  }
}
