import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { CollectorRunState, MetricsSnapshotWire } from '@trove/shared';

import { SidecarPanel } from './SidecarPanel.js';

const invokeMock = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const RUNNING: CollectorRunState = { kind: 'running', pid: 42, restarts: 0 };

function snapshot(opts?: Partial<MetricsSnapshotWire>): MetricsSnapshotWire {
  return {
    received: { spans: 7, metricPoints: 0, logRecords: 0 },
    sent: { spans: 7, metricPoints: 0, logRecords: 0 },
    lastSignalMsAgo: 4_000,
    scrapedMsAgo: 250,
    unreachable: false,
    overallHealth: 'green',
    diagObservations: {},
    ...opts,
  };
}

describe('SidecarPanel', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders running state with counts', () => {
    render(<SidecarPanel state={RUNNING} metrics={snapshot()} backends={[]} />);
    expect(screen.getByTestId('sidecar-state').textContent).toContain('running');
    expect(screen.getByTestId('counts-received-spans').textContent).toBe('7');
    expect(screen.getByTestId('counts-sent-spans').textContent).toBe('7');
  });

  it('renders zeros when no metrics snapshot is available', () => {
    render(<SidecarPanel state={RUNNING} metrics={null} backends={[]} />);
    expect(screen.getByTestId('counts-received-spans').textContent).toBe('0');
  });

  it('formats the last-signal timestamp in seconds when recent', () => {
    render(
      <SidecarPanel state={RUNNING} metrics={snapshot({ lastSignalMsAgo: 4_000 })} backends={[]} />,
    );
    expect(screen.getByTestId('counts-last-signal').textContent).toContain('4s ago');
  });

  it('renders an absolute timestamp sub-line when a signal has been observed', () => {
    render(
      <SidecarPanel state={RUNNING} metrics={snapshot({ lastSignalMsAgo: 4_000 })} backends={[]} />,
    );
    const sub = screen.getByTestId('counts-last-signal-at').textContent ?? '';
    // toLocaleTimeString output always contains a colon between hours and minutes.
    expect(sub).toMatch(/at \d/);
    expect(sub).toContain(':');
  });

  it('shows "none yet" when no signal has been observed', () => {
    render(
      <SidecarPanel state={RUNNING} metrics={snapshot({ lastSignalMsAgo: null })} backends={[]} />,
    );
    expect(screen.getByTestId('counts-last-signal').textContent).toContain('none yet');
  });

  it('formats failed state with reason', () => {
    render(
      <SidecarPanel
        state={{ kind: 'failed', reason: 'spawn fail' }}
        metrics={null}
        backends={[]}
      />,
    );
    expect(screen.getByTestId('sidecar-state').textContent).toContain('failed: spawn fail');
  });

  it('formats restarts in the running state when non-zero', () => {
    render(
      <SidecarPanel
        state={{ kind: 'running', pid: 1, restarts: 3 }}
        metrics={snapshot()}
        backends={[]}
      />,
    );
    expect(screen.getByTestId('sidecar-state').textContent).toContain('restarts: 3');
  });

  it('triggers test_export and surfaces ok result', async () => {
    invokeMock.mockResolvedValue({ status: 'ok', detail: 'received span at backend' });
    render(<SidecarPanel state={RUNNING} metrics={snapshot()} backends={[]} />);
    fireEvent.click(screen.getByTestId('test-pipeline-button'));
    await waitFor(() => {
      const result = screen.getByTestId('test-pipeline-result');
      expect(result.getAttribute('data-status')).toBe('ok');
      expect(result.textContent).toContain('received span at backend');
    });
  });

  it('renders synthetic-span hints after an ok test, labeled with the backend', async () => {
    invokeMock.mockResolvedValue({ status: 'ok', detail: 'received span' });
    render(
      <SidecarPanel
        state={RUNNING}
        metrics={snapshot()}
        backends={[
          {
            id: '11111111-1111-1111-1111-111111111111',
            enabled: true,
            backend: {
              kind: 'signoz',
              endpoint: 'https://ingest.eu.signoz.cloud:443',
              ingestionKey: { service: 'trove', account: 'signoz' },
            },
          },
        ]}
      />,
    );
    fireEvent.click(screen.getByTestId('test-pipeline-button'));
    await waitFor(() => {
      expect(screen.getByTestId('synthetic-span-hints')).toBeDefined();
    });
    expect(screen.getByTestId('synthetic-span-hints').textContent).toContain('SigNoz Cloud');
    expect(screen.getByTestId('hint-trace-id').textContent).toBe(
      '74726f7665740000c0deca5e0d5705ed',
    );
  });

  it('does not render synthetic-span hints on a failed test', async () => {
    invokeMock.mockResolvedValue({ status: 'failed', detail: 'bad' });
    render(<SidecarPanel state={RUNNING} metrics={snapshot()} backends={[]} />);
    fireEvent.click(screen.getByTestId('test-pipeline-button'));
    await waitFor(() => {
      expect(screen.getByTestId('test-pipeline-result').getAttribute('data-status')).toBe('failed');
    });
    expect(screen.queryByTestId('synthetic-span-hints')).toBeNull();
  });

  it('disables Test Pipeline while busy', async () => {
    let resolve: ((v: unknown) => void) | undefined;
    invokeMock.mockImplementation(
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
    render(<SidecarPanel state={RUNNING} metrics={snapshot()} backends={[]} />);
    const button = screen.getByTestId('test-pipeline-button');
    fireEvent.click(button);
    await waitFor(() => {
      expect((button as HTMLButtonElement).disabled).toBe(true);
    });
    expect(button.textContent).toContain('Testing');
    resolve?.({ status: 'ok', detail: 'done' });
    await waitFor(() => {
      expect((button as HTMLButtonElement).disabled).toBe(false);
    });
  });

  it('headline Received/Sent numerals sum all signal types, not just spans', () => {
    // Spans-less traffic: metrics + logs only (e.g. Sentinel forwarding to
    // SigNoz without traces). The pre-fix headline read `.spans` and so
    // rendered 0 here even though metrics/logs were flowing. Pin the
    // headline to the cross-signal total.
    const metrics = snapshot({
      received: { spans: 0, metricPoints: 175, logRecords: 149 },
      sent: { spans: 0, metricPoints: 448, logRecords: 294 },
    });
    render(<SidecarPanel state={RUNNING} metrics={metrics} backends={[]} />);

    const received = screen.getByTestId('counts-received');
    const sent = screen.getByTestId('counts-sent');

    // Headline numeral = total across traces + metrics + logs.
    expect(within(received).getByText('324')).toBeTruthy(); // 0 + 175 + 149
    expect(within(sent).getByText('742')).toBeTruthy(); // 0 + 448 + 294

    // Per-signal spans breakdown is still its own (zero) value, and the
    // metric/log counts remain visible in the subtext.
    expect(screen.getByTestId('counts-received-spans').textContent).toBe('0');
    expect(screen.getByTestId('counts-sent-spans').textContent).toBe('0');
    expect(received.textContent).toContain('175');
    expect(received.textContent).toContain('149');
    expect(sent.textContent).toContain('448');
    expect(sent.textContent).toContain('294');
  });
});
