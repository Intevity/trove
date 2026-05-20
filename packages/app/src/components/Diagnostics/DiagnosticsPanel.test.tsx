import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  AppState,
  CollectorRunState,
  HarnessConfig,
  HarnessId,
  MetricsSnapshotWire,
  TestExportResult,
} from '@trove/shared';

import {
  DiagnosticsPanel,
  deriveBackendRow,
  derivePassiveDiagnostics,
} from './DiagnosticsPanel.js';

const invokeMock = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const RUNNING: CollectorRunState = { kind: 'running', pid: 42, restarts: 0 };

function harnessConfig(id: HarnessId, enabled: boolean): HarnessConfig {
  return {
    id,
    enabled,
    configPath: `/tmp/${id}/settings.json`,
    lastPatchedAt: '2026-05-09T00:00:00.000Z',
    trovePatch: {
      managedBlockHash: 'a'.repeat(64),
      fileHashAtLastWrite: 'b'.repeat(64),
      format: 'json',
      lastWrittenRegionPayload: '{}',
    },
    options: { customAttributes: {} },
  };
}

function appState(opts?: {
  backends?: AppState['backends'];
  harnesses?: HarnessConfig[];
}): AppState {
  return {
    schemaVersion: 9,
    backends:
      opts?.backends === undefined
        ? [
            {
              id: '11111111-1111-1111-1111-111111111111',
              backend: {
                kind: 'signoz',
                endpoint: 'ingest.us.signoz.cloud:443',
                ingestionKey: { service: 'trove', account: 'signoz-key' },
              },
            },
          ]
        : opts.backends,
    harnesses: opts?.harnesses ?? [harnessConfig('claude-code', true)],
    autoUpdateEnabled: false,
    launchAtStartupEnabled: true,
    identity: { enabled: false, source: 'auto', name: '', email: '' },
    mappings: { schemaVersion: 2, metrics: [], harnesses: [] },
    telemetryObserved: {},
  };
}

function snapshot(opts?: Partial<MetricsSnapshotWire>): MetricsSnapshotWire {
  return {
    received: { spans: 0, metricPoints: 0, logRecords: 0 },
    sent: { spans: 0, metricPoints: 0, logRecords: 0 },
    lastSignalMsAgo: 4_000,
    scrapedMsAgo: 250,
    unreachable: false,
    overallHealth: 'green',
    diagObservations: {},
    ...opts,
  };
}

describe('derivePassiveDiagnostics', () => {
  it('marks sidecar green and signal green when running with recent telemetry', () => {
    const [sidecar, harnesses, signal] = derivePassiveDiagnostics(RUNNING, snapshot(), 1);
    expect(sidecar.status).toBe('green');
    expect(harnesses.status).toBe('green');
    expect(signal.status).toBe('green');
    expect(signal.detail).toContain('s ago');
  });

  it('marks sidecar red on crashed and includes restart count', () => {
    const [sidecar] = derivePassiveDiagnostics({ kind: 'crashed', restarts: 3 }, null, 0);
    expect(sidecar.status).toBe('red');
    expect(sidecar.detail).toContain('3 restarts');
  });

  it('marks sidecar red on failed with reason', () => {
    const [sidecar] = derivePassiveDiagnostics({ kind: 'failed', reason: 'spawn fail' }, null, 0);
    expect(sidecar.status).toBe('red');
    expect(sidecar.detail).toContain('spawn fail');
  });

  it('marks sidecar red on stopped', () => {
    const [sidecar] = derivePassiveDiagnostics({ kind: 'stopped' }, null, 0);
    expect(sidecar.status).toBe('red');
  });

  it('marks sidecar amber on starting/idle/stopping', () => {
    expect(derivePassiveDiagnostics({ kind: 'starting', pid: 1 }, null, 0)[0].status).toBe('amber');
    expect(derivePassiveDiagnostics({ kind: 'idle' }, null, 0)[0].status).toBe('amber');
    expect(derivePassiveDiagnostics({ kind: 'stopping' }, null, 0)[0].status).toBe('amber');
  });

  it('marks sidecar amber when state is unknown (null)', () => {
    const [sidecar] = derivePassiveDiagnostics(null, null, 0);
    expect(sidecar.status).toBe('amber');
    expect(sidecar.detail).toBe('state unknown');
  });

  it('marks harnesses red when none enabled', () => {
    const [, harnesses] = derivePassiveDiagnostics(RUNNING, snapshot(), 0);
    expect(harnesses.status).toBe('red');
    expect(harnesses.detail).toContain('no harnesses enabled');
    expect(harnesses.fixTargetTestid).toBe('harness-list');
  });

  it('marks signal amber and points at harnesses when enabledCount is zero', () => {
    const [, , signal] = derivePassiveDiagnostics(RUNNING, snapshot(), 0);
    expect(signal.status).toBe('amber');
    expect(signal.detail).toContain('first enabled harness');
    expect(signal.fixTargetTestid).toBe('harness-list');
  });

  it('marks signal amber when sidecar is not running', () => {
    const [, , signal] = derivePassiveDiagnostics({ kind: 'idle' }, snapshot(), 1);
    expect(signal.status).toBe('amber');
    expect(signal.detail).toBe('sidecar not running');
    expect(signal.fixTargetTestid).toBe('sidecar-panel');
  });

  it('marks signal amber when no metrics scrape yet', () => {
    const [, , signal] = derivePassiveDiagnostics(RUNNING, null, 1);
    expect(signal.status).toBe('amber');
    expect(signal.detail).toContain('first metrics scrape');
  });

  it('marks signal amber when metrics endpoint unreachable', () => {
    const [, , signal] = derivePassiveDiagnostics(RUNNING, snapshot({ unreachable: true }), 1);
    expect(signal.status).toBe('amber');
    expect(signal.detail).toContain('unreachable');
  });

  it('marks signal red when no telemetry observed yet', () => {
    const [, , signal] = derivePassiveDiagnostics(RUNNING, snapshot({ lastSignalMsAgo: null }), 1);
    expect(signal.status).toBe('red');
    expect(signal.detail).toContain('no telemetry observed yet');
  });

  it('marks signal amber when last signal is older than the recent window', () => {
    const [, , signal] = derivePassiveDiagnostics(
      RUNNING,
      snapshot({ lastSignalMsAgo: 5 * 60_000 }),
      1,
    );
    expect(signal.status).toBe('amber');
    expect(signal.detail).toContain('5m ago');
  });
});

describe('deriveBackendRow', () => {
  it('reports red when backend is not configured', () => {
    const row = deriveBackendRow(appState({ backends: [] }), null, false, null);
    expect(row.status).toBe('red');
    expect(row.detail).toContain('no platform configured');
    expect(row.fixTargetTestid).toBe('configure-platform-nudge');
  });

  it('reports amber while a check is in flight', () => {
    const row = deriveBackendRow(appState(), null, true, null);
    expect(row.status).toBe('amber');
    expect(row.detail).toBe('checking…');
  });

  it('reports amber when backend is configured but no data has flowed', () => {
    const row = deriveBackendRow(appState(), null, false, null);
    expect(row.status).toBe('amber');
    expect(row.detail).toContain('signoz configured');
    expect(row.detail).toContain('awaiting first export');
  });

  it('auto-greens when the exporter has delivered records', () => {
    // No manual check ever ran — derive from the live counters alone.
    const metrics = snapshot({
      sent: { spans: 0, metricPoints: 7, logRecords: 5 },
    });
    const row = deriveBackendRow(appState(), null, false, metrics);
    expect(row.status).toBe('green');
    expect(row.detail).toContain('exporting');
    expect(row.detail).toContain('12 records sent');
  });

  it('auto-green wins over a stale failed manual result when data is now flowing', () => {
    const metrics = snapshot({ sent: { spans: 0, metricPoints: 3, logRecords: 0 } });
    const row = deriveBackendRow(
      appState(),
      { status: 'failed', detail: 'old 401' },
      false,
      metrics,
    );
    expect(row.status).toBe('green');
    expect(row.detail).toContain('exporting');
  });

  it('falls back to manual ok result when no live exports yet', () => {
    const metrics = snapshot({ sent: { spans: 0, metricPoints: 0, logRecords: 0 } });
    const row = deriveBackendRow(appState(), { status: 'ok', detail: 'received' }, false, metrics);
    expect(row.status).toBe('green');
    expect(row.detail).toBe('received');
  });

  it('reports red when the test export failed and no live data is flowing', () => {
    const row = deriveBackendRow(appState(), { status: 'failed', detail: '401' }, false, null);
    expect(row.status).toBe('red');
    expect(row.detail).toContain('failed');
    expect(row.fixTargetTestid).toBe('configure-platform-nudge');
  });

  it('reports red when the test export timed out and no live data is flowing', () => {
    const row = deriveBackendRow(appState(), { status: 'timeout', detail: '5s' }, false, null);
    expect(row.status).toBe('red');
    expect(row.detail).toContain('timeout');
  });

  it('does not auto-green when the telemetry endpoint is unreachable', () => {
    // Counters might be stale; can't trust them. Surface the manual
    // path instead.
    const metrics = snapshot({
      sent: { spans: 0, metricPoints: 5, logRecords: 5 },
      unreachable: true,
    });
    const row = deriveBackendRow(appState(), null, false, metrics);
    expect(row.status).toBe('amber');
  });

  it('uses singular "record" wording when only one has been sent', () => {
    const metrics = snapshot({ sent: { spans: 1, metricPoints: 0, logRecords: 0 } });
    const row = deriveBackendRow(appState(), null, false, metrics);
    expect(row.detail).toContain('1 record sent');
    expect(row.detail).not.toContain('records');
  });
});

describe('DiagnosticsPanel', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders all four diagnostic rows on mount', () => {
    render(<DiagnosticsPanel appState={appState()} state={RUNNING} metrics={snapshot()} />);
    expect(screen.getByTestId('diagnostics-row-sidecar')).toBeTruthy();
    expect(screen.getByTestId('diagnostics-row-harnesses')).toBeTruthy();
    expect(screen.getByTestId('diagnostics-row-signal')).toBeTruthy();
    expect(screen.getByTestId('diagnostics-row-backend')).toBeTruthy();
  });

  it('renders the healthy state across all rows when everything is green', () => {
    render(<DiagnosticsPanel appState={appState()} state={RUNNING} metrics={snapshot()} />);
    expect(screen.getByTestId('diagnostics-row-sidecar').getAttribute('data-status')).toBe('green');
    expect(screen.getByTestId('diagnostics-row-harnesses').getAttribute('data-status')).toBe(
      'green',
    );
    expect(screen.getByTestId('diagnostics-row-signal').getAttribute('data-status')).toBe('green');
  });

  it('shows red sidecar row with a Fix link when the sidecar has crashed', () => {
    render(
      <DiagnosticsPanel
        appState={appState()}
        state={{ kind: 'crashed', restarts: 2 }}
        metrics={null}
      />,
    );
    const row = screen.getByTestId('diagnostics-row-sidecar');
    expect(row.getAttribute('data-status')).toBe('red');
    expect(screen.getByTestId('diagnostics-fix-sidecar')).toBeTruthy();
  });

  it('shows red harnesses row when no harness is enabled', () => {
    const harnesses = [harnessConfig('claude-code', false), harnessConfig('gemini-cli', false)];
    render(
      <DiagnosticsPanel appState={appState({ harnesses })} state={RUNNING} metrics={snapshot()} />,
    );
    expect(screen.getByTestId('diagnostics-row-harnesses').getAttribute('data-status')).toBe('red');
  });

  it('hides the Fix link for green rows', () => {
    render(<DiagnosticsPanel appState={appState()} state={RUNNING} metrics={snapshot()} />);
    expect(screen.queryByTestId('diagnostics-fix-sidecar')).toBeNull();
    expect(screen.queryByTestId('diagnostics-fix-harnesses')).toBeNull();
  });

  it('runs test_export when the backend-check button is clicked and shows ok result', async () => {
    const result: TestExportResult = { status: 'ok', detail: 'received span' };
    invokeMock.mockResolvedValue(result);
    render(<DiagnosticsPanel appState={appState()} state={RUNNING} metrics={snapshot()} />);
    fireEvent.click(screen.getByTestId('diagnostics-backend-check-button'));
    await waitFor(() => {
      const row = screen.getByTestId('diagnostics-row-backend');
      expect(row.getAttribute('data-status')).toBe('green');
      expect(row.textContent).toContain('received span');
    });
    expect(invokeMock).toHaveBeenCalledWith('test_export', undefined);
  });

  it('shows red backend row when the test export reports failed', async () => {
    invokeMock.mockResolvedValue({ status: 'failed', detail: '401 unauthorized' });
    render(<DiagnosticsPanel appState={appState()} state={RUNNING} metrics={snapshot()} />);
    fireEvent.click(screen.getByTestId('diagnostics-backend-check-button'));
    await waitFor(() => {
      const row = screen.getByTestId('diagnostics-row-backend');
      expect(row.getAttribute('data-status')).toBe('red');
      expect(row.textContent).toContain('401 unauthorized');
    });
    // Red row shows a Fix link pointing at the backend banner.
    expect(screen.getByTestId('diagnostics-fix-backend')).toBeTruthy();
  });

  it('disables the backend-check button while in flight', async () => {
    let resolve: ((v: TestExportResult) => void) | undefined;
    invokeMock.mockImplementation(
      () =>
        new Promise<TestExportResult>((r) => {
          resolve = r;
        }),
    );
    render(<DiagnosticsPanel appState={appState()} state={RUNNING} metrics={snapshot()} />);
    const button = screen.getByTestId('diagnostics-backend-check-button') as HTMLButtonElement;
    fireEvent.click(button);
    await waitFor(() => {
      expect(button.disabled).toBe(true);
    });
    expect(button.textContent).toContain('Checking');
    resolve?.({ status: 'ok', detail: 'done' });
    await waitFor(() => {
      expect(button.disabled).toBe(false);
    });
  });

  it('surfaces an IPC error inline beneath the rows', async () => {
    invokeMock.mockRejectedValue({
      kind: 'io',
      path: '/var/run/trove.sock',
      reason: 'EHOSTUNREACH',
    });
    render(<DiagnosticsPanel appState={appState()} state={RUNNING} metrics={snapshot()} />);
    fireEvent.click(screen.getByTestId('diagnostics-backend-check-button'));
    await waitFor(() => {
      const err = screen.getByTestId('diagnostics-backend-check-error');
      expect(err.textContent).toContain('io');
    });
  });

  it('Fix link calls scrollIntoView on the targeted dashboard section', () => {
    const target = document.createElement('div');
    target.setAttribute('data-testid', 'sidecar-panel');
    const scrollSpy = vi.fn();
    (target as unknown as { scrollIntoView: typeof scrollSpy }).scrollIntoView = scrollSpy;
    document.body.appendChild(target);

    render(
      <DiagnosticsPanel
        appState={appState()}
        state={{ kind: 'crashed', restarts: 1 }}
        metrics={null}
      />,
    );
    fireEvent.click(screen.getByTestId('diagnostics-fix-sidecar'));
    expect(scrollSpy).toHaveBeenCalledWith({ behavior: 'smooth', block: 'center' });

    document.body.removeChild(target);
  });
});
