import { useState } from 'react';

import type { LucideIcon } from 'lucide-react';
import { Activity, Cable, Cpu, Radio, Stethoscope, Zap } from 'lucide-react';

import type {
  AppState,
  CollectorRunState,
  MetricsSnapshotWire,
  TestExportResult,
} from '@trove/shared';

import { formatCount, signalTotal } from '../../lib/format.js';
import { RECENT_SIGNAL_WINDOW_MS } from '../../lib/health.js';
import { TroveIpcError, testExport } from '../../lib/ipc.js';
import { Button, Card, CardHeader, CardTitle } from '../ui/index.js';

interface Props {
  appState: AppState;
  state: CollectorRunState | null;
  metrics: MetricsSnapshotWire | null;
}

export type DiagnosticStatus = 'green' | 'amber' | 'red';

export interface DiagnosticRow {
  id: 'sidecar' | 'backend' | 'harnesses' | 'signal';
  label: string;
  status: DiagnosticStatus;
  detail: string;
  /** `data-testid` of the dashboard section this row's "Fix" link should
   *  scroll to. `null` for rows whose fix-action is the inline button. */
  fixTargetTestid: string | null;
}

/** "Why isn't my data showing up?" diagnostics — Sprint 11.
 *
 *  Composes four checks the plan calls out: sidecar healthy, backend
 *  reachable, at least one harness enabled, recent signal observed. The
 *  first three are derived from the same IPC data already polled by the
 *  rest of the dashboard (zero extra network cost). The fourth — the
 *  backend reachability probe — is gated behind an explicit button so
 *  rendering the panel doesn't fire a synthetic OTLP payload on every
 *  mount. */
export function DiagnosticsPanel({ appState, state, metrics }: Props): JSX.Element {
  const [busy, setBusy] = useState(false);
  const [backendResult, setBackendResult] = useState<TestExportResult | null>(null);
  const [error, setError] = useState<TroveIpcError | null>(null);

  const enabledHarnessCount = appState.harnesses.filter((h) => h.enabled).length;
  const passiveRows = derivePassiveDiagnostics(state, metrics, enabledHarnessCount);
  const backendRow = deriveBackendRow(appState, backendResult, busy, metrics);

  async function handleRunBackendCheck(): Promise<void> {
    setBusy(true);
    setError(null);
    setBackendResult(null);
    try {
      const r = await testExport();
      setBackendResult(r);
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card testid="diagnostics-panel">
      <CardHeader>
        <CardTitle className="flex items-center gap-1.5 whitespace-nowrap">
          <Activity size={14} strokeWidth={2.4} className="text-brand" aria-hidden="true" />
          Diagnostics
        </CardTitle>
        <Button
          variant="secondary"
          size="sm"
          testid="diagnostics-backend-check-button"
          disabled={busy}
          onClick={() => void handleRunBackendCheck()}
        >
          <Stethoscope size={12} aria-hidden />
          {busy ? 'Checking…' : 'Run backend check'}
        </Button>
      </CardHeader>

      <ul className="divide-y divide-hairline rounded-card border border-hairline dark:divide-hairline-dark dark:border-hairline-dark">
        {passiveRows.map((row) => (
          <DiagnosticsRow key={row.id} row={row} />
        ))}
        <DiagnosticsRow row={backendRow} />
      </ul>

      {error ? (
        <p data-testid="diagnostics-backend-check-error" className="mt-2 text-[12px] text-ios-red">
          Backend check failed: {error.cause.kind}
        </p>
      ) : null}
    </Card>
  );
}

const ROW_ICON: Record<DiagnosticRow['id'], LucideIcon> = {
  sidecar: Cpu,
  backend: Cable,
  harnesses: Zap,
  signal: Radio,
};

const ROW_TINT: Record<DiagnosticStatus, string> = {
  green: '',
  amber: 'bg-ios-orange/[0.04] dark:bg-ios-orange/[0.06]',
  red: 'bg-ios-red/[0.05] dark:bg-ios-red/[0.07]',
};

function StatusIconChip({
  icon: Icon,
  status,
}: {
  icon: LucideIcon;
  status: DiagnosticStatus;
}): JSX.Element {
  const tone = {
    green: 'bg-ios-green/[0.14] text-ios-green',
    amber: 'bg-ios-orange/[0.14] text-ios-orange',
    red: 'bg-ios-red/[0.14] text-ios-red',
  }[status];
  return (
    <span
      aria-hidden="true"
      className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-md ${tone}`}
    >
      <Icon size={13} strokeWidth={2.4} />
    </span>
  );
}

function DiagnosticsRow({ row }: { row: DiagnosticRow }): JSX.Element {
  const Icon = ROW_ICON[row.id];
  return (
    <li
      data-testid={`diagnostics-row-${row.id}`}
      data-status={row.status}
      className={`flex items-center gap-3 px-3.5 py-2.5 transition-colors first:rounded-t-card last:rounded-b-card ${ROW_TINT[row.status]}`}
    >
      <StatusIconChip icon={Icon} status={row.status} />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="text-[13px] font-medium leading-tight text-fg-primary dark:text-fg-primary-dark">
          {row.label}
        </span>
        <span className="mt-0.5 text-[12px] leading-snug text-fg-secondary dark:text-fg-secondary-dark">
          {row.detail}
        </span>
      </div>
      {row.fixTargetTestid && row.status !== 'green' ? (
        <Button
          variant="ghost"
          size="sm"
          className="ml-auto self-center"
          testid={`diagnostics-fix-${row.id}`}
          onClick={() => scrollToTestid(row.fixTargetTestid!)}
        >
          Fix
        </Button>
      ) : null}
    </li>
  );
}

function scrollToTestid(testid: string): void {
  const el = document.querySelector(`[data-testid="${testid}"]`);
  if (el && typeof (el as HTMLElement).scrollIntoView === 'function') {
    (el as HTMLElement).scrollIntoView({ behavior: 'smooth', block: 'center' });
  }
}

/** Pure derivation for the three passive checks — exported for testing.
 *  Returns a fixed 3-tuple in order: [sidecar, harnesses, signal]. */
export function derivePassiveDiagnostics(
  state: CollectorRunState | null,
  metrics: MetricsSnapshotWire | null,
  enabledHarnessCount: number,
): [DiagnosticRow, DiagnosticRow, DiagnosticRow] {
  return [
    deriveSidecarRow(state),
    deriveHarnessesRow(enabledHarnessCount),
    deriveSignalRow(state, metrics, enabledHarnessCount),
  ];
}

function deriveSidecarRow(state: CollectorRunState | null): DiagnosticRow {
  const fix = 'sidecar-panel';
  if (!state) {
    return {
      id: 'sidecar',
      label: 'Sidecar',
      status: 'amber',
      detail: 'state unknown',
      fixTargetTestid: fix,
    };
  }
  switch (state.kind) {
    case 'running':
      return {
        id: 'sidecar',
        label: 'Sidecar',
        status: 'green',
        detail: 'running',
        fixTargetTestid: fix,
      };
    case 'starting':
      return {
        id: 'sidecar',
        label: 'Sidecar',
        status: 'amber',
        detail: 'starting up',
        fixTargetTestid: fix,
      };
    case 'idle':
      return {
        id: 'sidecar',
        label: 'Sidecar',
        status: 'amber',
        detail: 'idle (no harness enabled)',
        fixTargetTestid: fix,
      };
    case 'stopping':
      return {
        id: 'sidecar',
        label: 'Sidecar',
        status: 'amber',
        detail: 'stopping',
        fixTargetTestid: fix,
      };
    case 'crashed':
      return {
        id: 'sidecar',
        label: 'Sidecar',
        status: 'red',
        detail: `crashed (${state.restarts} restart${state.restarts === 1 ? '' : 's'})`,
        fixTargetTestid: fix,
      };
    case 'failed':
      return {
        id: 'sidecar',
        label: 'Sidecar',
        status: 'red',
        detail: `failed: ${state.reason}`,
        fixTargetTestid: fix,
      };
    case 'stopped':
      return {
        id: 'sidecar',
        label: 'Sidecar',
        status: 'red',
        detail: 'stopped',
        fixTargetTestid: fix,
      };
  }
}

function deriveHarnessesRow(enabledHarnessCount: number): DiagnosticRow {
  const fix = 'harness-list';
  if (enabledHarnessCount === 0) {
    return {
      id: 'harnesses',
      label: 'Harnesses',
      status: 'red',
      detail: 'no harnesses enabled; Trove has nothing to forward',
      fixTargetTestid: fix,
    };
  }
  return {
    id: 'harnesses',
    label: 'Harnesses',
    status: 'green',
    detail: `${enabledHarnessCount} enabled`,
    fixTargetTestid: fix,
  };
}

function deriveSignalRow(
  state: CollectorRunState | null,
  metrics: MetricsSnapshotWire | null,
  enabledHarnessCount: number,
): DiagnosticRow {
  if (enabledHarnessCount === 0) {
    return {
      id: 'signal',
      label: 'Recent signal',
      status: 'amber',
      detail: 'waiting on first enabled harness',
      fixTargetTestid: 'harness-list',
    };
  }
  if (!state || state.kind !== 'running') {
    return {
      id: 'signal',
      label: 'Recent signal',
      status: 'amber',
      detail: 'sidecar not running',
      fixTargetTestid: 'sidecar-panel',
    };
  }
  if (!metrics) {
    return {
      id: 'signal',
      label: 'Recent signal',
      status: 'amber',
      detail: 'awaiting first metrics scrape',
      fixTargetTestid: null,
    };
  }
  if (metrics.unreachable) {
    return {
      id: 'signal',
      label: 'Recent signal',
      status: 'amber',
      detail: 'collector telemetry endpoint unreachable',
      fixTargetTestid: 'sidecar-panel',
    };
  }
  if (metrics.lastSignalMsAgo === null) {
    return {
      id: 'signal',
      label: 'Recent signal',
      status: 'red',
      detail: 'no telemetry observed yet; try invoking an enabled harness',
      fixTargetTestid: 'harness-list',
    };
  }
  if (metrics.lastSignalMsAgo > RECENT_SIGNAL_WINDOW_MS) {
    const minutes = Math.max(1, Math.round(metrics.lastSignalMsAgo / 60_000));
    return {
      id: 'signal',
      label: 'Recent signal',
      status: 'amber',
      detail: `last signal ${minutes}m ago; quiet for over a minute`,
      fixTargetTestid: 'harness-list',
    };
  }
  const seconds = Math.max(1, Math.round(metrics.lastSignalMsAgo / 1000));
  return {
    id: 'signal',
    label: 'Recent signal',
    status: 'green',
    detail: `last signal ${seconds}s ago`,
    fixTargetTestid: null,
  };
}

/** Pure derivation for the backend row — exported for testing.
 *
 *  Backend health is derived passively from the collector's exporter
 *  counters when possible: any `otelcol_exporter_sent_*` increment
 *  proves the backend accepted at least one record (the OTel
 *  exporter counts SENT, not ATTEMPTED — see metrics_tap.rs comments).
 *  We fall back to the manual `Run backend check` result when no live
 *  data has flowed yet, so a fresh install can still confirm its
 *  credentials without waiting on real harness activity. */
export function deriveBackendRow(
  appState: AppState,
  result: TestExportResult | null,
  busy: boolean,
  metrics: MetricsSnapshotWire | null,
): DiagnosticRow {
  const fix = 'configure-platform-nudge';
  if (appState.backends.length === 0) {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'red',
      detail: 'no platform configured; open Platforms to add one',
      fixTargetTestid: fix,
    };
  }
  const primaryLabel = backendsLabel(appState.backends);
  if (busy) {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'amber',
      detail: 'checking…',
      fixTargetTestid: null,
    };
  }
  const sentTotal = signalTotal(metrics?.sent);
  // Auto-green when the exporter has successfully delivered ≥1 record
  // since the collector started. This supersedes a stale failed
  // result, because live successful exports are stronger evidence
  // than an old manual check. The `unreachable` guard skips this when
  // we can't read the telemetry endpoint at all.
  if (metrics && !metrics.unreachable && sentTotal > 0) {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'green',
      detail: `${primaryLabel}; exporting (${formatCount(sentTotal)} record${sentTotal === 1 ? '' : 's'} sent)`,
      fixTargetTestid: null,
    };
  }
  if (result?.status === 'ok') {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'green',
      detail: result.detail,
      fixTargetTestid: null,
    };
  }
  if (result) {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'red',
      detail: `${result.status}: ${result.detail}`,
      fixTargetTestid: fix,
    };
  }
  return {
    id: 'backend',
    label: 'Backend',
    status: 'amber',
    detail: `${primaryLabel} configured; awaiting first export to verify`,
    fixTargetTestid: null,
  };
}

/** Describe the configured forwarding list in one line. Single
 *  destinations get their kind verbatim; multi-platform configurations
 *  collapse into "{first.kind} + N more" so the diagnostic stays
 *  compact. */
function backendsLabel(backends: AppState['backends']): string {
  if (backends.length === 0) return 'none';
  const first = backends[0]!.backend.kind;
  if (backends.length === 1) return first;
  return `${first} + ${backends.length - 1} more`;
}
