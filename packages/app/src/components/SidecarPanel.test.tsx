import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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
});
