import { useState } from 'react';

import type {
  AppState,
  CollectorRunState,
  MetricsSnapshotWire,
  TestExportResult,
} from '@trove/shared';

import { RECENT_SIGNAL_WINDOW_MS } from '../../lib/health.js';
import { TroveIpcError, testExport } from '../../lib/ipc.js';

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
  const backendRow = deriveBackendRow(appState, backendResult, busy);

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
    <section
      data-testid="diagnostics-panel"
      className="rounded-md border border-slate-200 bg-white px-4 py-3 dark:border-slate-800 dark:bg-slate-900"
    >
      <header className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">Diagnostics</h2>
        <button
          type="button"
          data-testid="diagnostics-backend-check-button"
          disabled={busy}
          onClick={() => void handleRunBackendCheck()}
          className="rounded-md border border-blue-300 bg-blue-50 px-2.5 py-1 text-xs font-medium text-blue-900 hover:bg-blue-100 disabled:cursor-not-allowed disabled:opacity-60 dark:border-blue-800 dark:bg-blue-950 dark:text-blue-100 dark:hover:bg-blue-900"
        >
          {busy ? 'Checking…' : 'Run backend check'}
        </button>
      </header>

      <ul className="flex flex-col gap-1.5">
        {passiveRows.map((row) => (
          <DiagnosticsRow key={row.id} row={row} />
        ))}
        <DiagnosticsRow row={backendRow} />
      </ul>

      {error ? (
        <p
          data-testid="diagnostics-backend-check-error"
          className="mt-2 text-xs text-red-700 dark:text-red-300"
        >
          Backend check failed: {error.cause.kind}
        </p>
      ) : null}
    </section>
  );
}

const ROW_STYLES: Record<DiagnosticStatus, string> = {
  green:
    'border-emerald-200 bg-emerald-50 text-emerald-900 dark:border-emerald-800 dark:bg-emerald-950 dark:text-emerald-200',
  amber:
    'border-amber-200 bg-amber-50 text-amber-900 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-200',
  red: 'border-red-200 bg-red-50 text-red-900 dark:border-red-800 dark:bg-red-950 dark:text-red-200',
};

const ROW_DOT: Record<DiagnosticStatus, string> = {
  green: 'bg-emerald-500',
  amber: 'bg-amber-500',
  red: 'bg-red-500',
};

function DiagnosticsRow({ row }: { row: DiagnosticRow }): JSX.Element {
  return (
    <li
      data-testid={`diagnostics-row-${row.id}`}
      data-status={row.status}
      className={`flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs ${ROW_STYLES[row.status]}`}
    >
      <span
        aria-hidden
        className={`inline-block h-2.5 w-2.5 rounded-full ${ROW_DOT[row.status]}`}
      />
      <span className="font-medium">{row.label}</span>
      <span className="opacity-80">— {row.detail}</span>
      {row.fixTargetTestid && row.status !== 'green' ? (
        <button
          type="button"
          data-testid={`diagnostics-fix-${row.id}`}
          onClick={() => scrollToTestid(row.fixTargetTestid!)}
          className="ml-auto text-xs font-medium underline-offset-2 hover:underline"
        >
          Fix
        </button>
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
      detail: 'no harnesses enabled — Trove has nothing to forward',
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
      detail: 'no telemetry observed yet — try invoking an enabled harness',
      fixTargetTestid: 'harness-list',
    };
  }
  if (metrics.lastSignalMsAgo > RECENT_SIGNAL_WINDOW_MS) {
    const minutes = Math.max(1, Math.round(metrics.lastSignalMsAgo / 60_000));
    return {
      id: 'signal',
      label: 'Recent signal',
      status: 'amber',
      detail: `last signal ${minutes}m ago — quiet for over a minute`,
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

/** Pure derivation for the backend row — exported for testing. */
export function deriveBackendRow(
  appState: AppState,
  result: TestExportResult | null,
  busy: boolean,
): DiagnosticRow {
  const fix = 'backend-banner';
  if (!appState.backend) {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'red',
      detail: 'no backend configured — finish the wizard',
      fixTargetTestid: fix,
    };
  }
  if (busy) {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'amber',
      detail: 'checking…',
      fixTargetTestid: null,
    };
  }
  if (!result) {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'amber',
      detail: `${appState.backend.kind} configured — run check to verify`,
      fixTargetTestid: null,
    };
  }
  if (result.status === 'ok') {
    return {
      id: 'backend',
      label: 'Backend',
      status: 'green',
      detail: result.detail,
      fixTargetTestid: null,
    };
  }
  return {
    id: 'backend',
    label: 'Backend',
    status: 'red',
    detail: `${result.status}: ${result.detail}`,
    fixTargetTestid: fix,
  };
}
