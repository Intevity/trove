import { Zap } from 'lucide-react';
import { useState } from 'react';

import type {
  BackendInstance,
  CollectorRunState,
  MetricsSnapshotWire,
  TestExportResult,
} from '@trove/shared';

import { TroveIpcError, testExport } from '../lib/ipc.js';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  StatTile,
  StatusDot,
  type DotStatus,
} from './ui/index.js';
import { SyntheticSpanHints } from './wizard/SyntheticSpanHints.js';

interface Props {
  state: CollectorRunState | null;
  metrics: MetricsSnapshotWire | null;
  /** Configured forwarding destinations. Used only by the synthetic-span
   *  hints ("Look for this in {label}") after a successful Test Pipeline
   *  run — the hint targets the first configured backend since the
   *  payload is broadcast to every destination. Empty list falls back
   *  to a generic preamble. */
  backends: BackendInstance[];
}

/** Collector / sidecar dashboard tile. Surfaces the supervisor's
 *  current run state, summary counts from the metrics tap, and the
 *  "Test Pipeline" affordance the e2e exercises. */
export function SidecarPanel({ state, metrics, backends }: Props): JSX.Element {
  const primaryBackendKind = backends[0]?.backend.kind;
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
    <Card testid="sidecar-panel">
      <CardHeader>
        <CardTitle>Collector</CardTitle>
        <div className="flex items-center gap-2">
          <span className="flex items-center gap-1.5">
            <StatusDot status={dotForState(state)} size="sm" />
            <span
              data-testid="sidecar-state"
              className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark"
            >
              {formatState(state)}
            </span>
          </span>
          <Button
            variant="secondary"
            size="sm"
            testid="test-pipeline-button"
            disabled={busy}
            onClick={() => void handleTest()}
          >
            <Zap size={12} aria-hidden />
            {busy ? 'Testing…' : 'Test Pipeline'}
          </Button>
        </div>
      </CardHeader>

      <div className="grid grid-cols-3 gap-2">
        <StatTile testid="counts-received" label="Received" value={metrics?.received.spans ?? 0}>
          <span data-testid="counts-received-spans">{metrics?.received.spans ?? 0}</span> spans ·{' '}
          <span data-testid="counts-received-metrics">{metrics?.received.metricPoints ?? 0}</span>{' '}
          metrics ·{' '}
          <span data-testid="counts-received-logs">{metrics?.received.logRecords ?? 0}</span> logs
        </StatTile>
        <StatTile testid="counts-sent" label="Sent" value={metrics?.sent.spans ?? 0}>
          <span data-testid="counts-sent-spans">{metrics?.sent.spans ?? 0}</span> spans ·{' '}
          <span data-testid="counts-sent-metrics">{metrics?.sent.metricPoints ?? 0}</span> metrics ·{' '}
          <span data-testid="counts-sent-logs">{metrics?.sent.logRecords ?? 0}</span> logs
        </StatTile>
        <StatTile
          testid="counts-last-signal"
          label="Last signal"
          value={formatLastSignal(metrics)}
        />
      </div>

      {result || error ? (
        <div className="mt-2 flex items-center gap-2">
          {result ? (
            <span
              data-testid="test-pipeline-result"
              data-status={result.status}
              className={`text-[12px] ${
                result.status === 'ok' ? 'text-ios-green' : 'text-ios-orange'
              }`}
            >
              {result.status === 'ok' ? '✓' : '⚠︎'} {result.detail}
            </span>
          ) : null}
          {error ? (
            <span className="text-[12px] text-ios-red">Test failed: {error.cause.kind}</span>
          ) : null}
        </div>
      ) : null}
      {result?.status === 'ok' ? (
        <div className="mt-2">
          <SyntheticSpanHints backendKind={primaryBackendKind} />
        </div>
      ) : null}
    </Card>
  );
}

/** Map collector run-state to the small status dot in the card header.
 *  Presentational only — the same kind→tone collapse the diagnostics
 *  row does, but kept local because this is a different surface. */
function dotForState(state: CollectorRunState | null): DotStatus {
  if (!state) return 'gray';
  switch (state.kind) {
    case 'running':
      return 'green';
    case 'starting':
    case 'stopping':
    case 'idle':
      return 'amber';
    case 'crashed':
    case 'failed':
    case 'stopped':
      return 'red';
  }
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
