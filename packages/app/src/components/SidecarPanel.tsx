import { useState } from 'react';

import type {
  Backend,
  CollectorRunState,
  MetricsSnapshotWire,
  TestExportResult,
} from '@trove/shared';

import { TroveIpcError, testExport } from '../lib/ipc.js';
import { SyntheticSpanHints } from './wizard/SyntheticSpanHints.js';

interface Props {
  state: CollectorRunState | null;
  metrics: MetricsSnapshotWire | null;
  /** Backend currently configured. Used only to label the post-success
   *  synthetic-span hints ("Look for this in {label}"). Null on the
   *  brief window between collector startup and backend save, in which
   *  case the hints fall back to a generic preamble. */
  backend?: Backend | null | undefined;
}

/** Collector / sidecar dashboard tile. Surfaces the supervisor's
 *  current run state, summary counts from the metrics tap, and the
 *  "Test Pipeline" affordance the e2e exercises. */
export function SidecarPanel({ state, metrics, backend }: Props): JSX.Element {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<TestExportResult | null>(null);
  const [error, setError] = useState<TroveIpcError | null>(null);

  async function handleTest(): Promise<void> {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const r = await testExport();
      setResult(r);
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      data-testid="sidecar-panel"
      className="rounded-md border border-slate-200 bg-white px-4 py-3 dark:border-slate-800 dark:bg-slate-900"
    >
      <header className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">Collector</h2>
        <span data-testid="sidecar-state" className="text-xs text-slate-600 dark:text-slate-400">
          {formatState(state)}
        </span>
      </header>

      <dl className="grid grid-cols-3 gap-3 text-xs text-slate-700 dark:text-slate-300">
        <CountTile
          label="Received"
          spans={metrics?.received.spans ?? 0}
          metricPoints={metrics?.received.metricPoints ?? 0}
          logRecords={metrics?.received.logRecords ?? 0}
          testid="counts-received"
        />
        <CountTile
          label="Sent"
          spans={metrics?.sent.spans ?? 0}
          metricPoints={metrics?.sent.metricPoints ?? 0}
          logRecords={metrics?.sent.logRecords ?? 0}
          testid="counts-sent"
        />
        <div data-testid="counts-last-signal">
          <dt className="text-slate-500">Last signal</dt>
          <dd className="font-mono text-sm">{formatLastSignal(metrics)}</dd>
        </div>
      </dl>

      <div className="mt-3 flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <button
            type="button"
            data-testid="test-pipeline-button"
            disabled={busy}
            onClick={() => void handleTest()}
            className="rounded-md border border-blue-300 bg-blue-50 px-2.5 py-1 text-xs font-medium text-blue-900 hover:bg-blue-100 disabled:cursor-not-allowed disabled:opacity-60 dark:border-blue-800 dark:bg-blue-950 dark:text-blue-100 dark:hover:bg-blue-900"
          >
            {busy ? 'Testing…' : 'Test Pipeline'}
          </button>
          {result ? (
            <span
              data-testid="test-pipeline-result"
              data-status={result.status}
              className={`text-xs ${
                result.status === 'ok'
                  ? 'text-emerald-700 dark:text-emerald-300'
                  : 'text-amber-700 dark:text-amber-300'
              }`}
            >
              {result.status === 'ok' ? '✓' : '⚠︎'} {result.detail}
            </span>
          ) : null}
          {error ? (
            <span className="text-xs text-red-700 dark:text-red-300">
              Test failed: {error.cause.kind}
            </span>
          ) : null}
        </div>
        {result?.status === 'ok' ? <SyntheticSpanHints backendKind={backend?.kind} /> : null}
      </div>
    </section>
  );
}

function CountTile(props: {
  label: string;
  spans: number;
  metricPoints: number;
  logRecords: number;
  testid: string;
}): JSX.Element {
  const { label, spans, metricPoints, logRecords, testid } = props;
  return (
    <div data-testid={testid}>
      <dt className="text-slate-500">{label}</dt>
      <dd className="font-mono text-sm">
        <span data-testid={`${testid}-spans`}>{spans}</span> spans ·{' '}
        <span data-testid={`${testid}-metrics`}>{metricPoints}</span> metrics ·{' '}
        <span data-testid={`${testid}-logs`}>{logRecords}</span> logs
      </dd>
    </div>
  );
}

function formatState(state: CollectorRunState | null): string {
  if (!state) return 'unknown';
  switch (state.kind) {
    case 'idle':
      return 'idle';
    case 'starting':
      return `starting (pid ${state.pid})`;
    case 'running':
      return `running${state.restarts > 0 ? ` (restarts: ${state.restarts})` : ''}`;
    case 'crashed':
      return `crashed (restarts: ${state.restarts})`;
    case 'stopping':
      return 'stopping';
    case 'stopped':
      return 'stopped';
    case 'failed':
      return `failed: ${state.reason}`;
  }
}

function formatLastSignal(metrics: MetricsSnapshotWire | null): string {
  if (!metrics) return '—';
  if (metrics.lastSignalMsAgo === null) return 'none yet';
  const seconds = Math.round(metrics.lastSignalMsAgo / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  return `${minutes}m ago`;
}
