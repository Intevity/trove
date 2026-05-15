import { describe, expect, it } from 'vitest';

import type { CollectorRunState, MetricsSnapshotWire } from '@trove/shared';

import { RECENT_SIGNAL_WINDOW_MS, deriveOverallHealth, overallHealthLabel } from './health.js';

const RUNNING: CollectorRunState = { kind: 'running', pid: 1, restarts: 0 };

function snapshot(opts?: Partial<MetricsSnapshotWire>): MetricsSnapshotWire {
  return {
    received: { spans: 0, metricPoints: 0, logRecords: 0 },
    sent: { spans: 0, metricPoints: 0, logRecords: 0 },
    lastSignalMsAgo: null,
    scrapedMsAgo: 0,
    unreachable: false,
    overallHealth: 'green',
    diagObservations: {},
    ...opts,
  };
}

describe('deriveOverallHealth', () => {
  it('reports red for crashed', () => {
    expect(deriveOverallHealth({ kind: 'crashed', restarts: 1 }, null, 0)).toBe('red');
  });

  it('reports red for failed', () => {
    expect(deriveOverallHealth({ kind: 'failed', reason: 'spawn fail' }, null, 0)).toBe('red');
  });

  it('reports red for stopped', () => {
    expect(deriveOverallHealth({ kind: 'stopped' }, null, 0)).toBe('red');
  });

  it('reports amber for idle', () => {
    expect(deriveOverallHealth({ kind: 'idle' }, null, 0)).toBe('amber');
  });

  it('reports amber for starting', () => {
    expect(deriveOverallHealth({ kind: 'starting', pid: 1 }, null, 0)).toBe('amber');
  });

  it('reports amber for stopping', () => {
    expect(deriveOverallHealth({ kind: 'stopping' }, null, 0)).toBe('amber');
  });

  it('reports amber when running but metrics endpoint unreachable', () => {
    expect(deriveOverallHealth(RUNNING, snapshot({ unreachable: true }), 3)).toBe('amber');
  });

  it('reports amber when running but no scrape has returned yet', () => {
    expect(deriveOverallHealth(RUNNING, null, 0)).toBe('amber');
  });

  it('reports green when running with zero harnesses regardless of last signal', () => {
    expect(deriveOverallHealth(RUNNING, snapshot({ lastSignalMsAgo: null }), 0)).toBe('green');
  });

  it('reports green when running with a recent signal within the staleness window', () => {
    expect(deriveOverallHealth(RUNNING, snapshot({ lastSignalMsAgo: 5_000 }), 2)).toBe('green');
  });

  it('reports amber when the last signal is older than the staleness window', () => {
    expect(
      deriveOverallHealth(RUNNING, snapshot({ lastSignalMsAgo: RECENT_SIGNAL_WINDOW_MS + 1 }), 2),
    ).toBe('amber');
  });

  it('reports amber when running with enabled harnesses and no signal yet', () => {
    expect(deriveOverallHealth(RUNNING, snapshot({ lastSignalMsAgo: null }), 1)).toBe('amber');
  });

  it('treats a signal at the exact window boundary as green', () => {
    expect(
      deriveOverallHealth(RUNNING, snapshot({ lastSignalMsAgo: RECENT_SIGNAL_WINDOW_MS }), 1),
    ).toBe('green');
  });
});

describe('overallHealthLabel', () => {
  it('produces a distinct label per state', () => {
    expect(overallHealthLabel('green')).toBe('Healthy');
    expect(overallHealthLabel('amber')).toBe('Awaiting telemetry');
    expect(overallHealthLabel('red')).toBe('Sidecar down');
  });
});
