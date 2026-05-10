import { useCallback, useState } from 'react';

import { presetMetadataFor } from '@trove/collector-presets';
import type { AppState, Backend, HarnessId } from '@trove/shared';

import { useCollectorStatus } from '../hooks/useCollectorStatus.js';
import { useDetectedHarnesses } from '../hooks/useDetectedHarnesses.js';
import { useMetricsSnapshot } from '../hooks/useMetricsSnapshot.js';
import { deriveOverallHealth } from '../lib/health.js';
import { TroveIpcError, revertPatch, setAutoUpdateEnabled } from '../lib/ipc.js';
import { DiagnosticsPanel } from './Diagnostics/DiagnosticsPanel.js';
import { HarnessList } from './HarnessList.js';
import { LogsPanel } from './LogsPanel.js';
import { OverallHealthBadge } from './OverallHealthBadge.js';
import { PatchPreviewModal } from './PatchPreviewModal.js';
import { AutoUpdate } from './Settings/AutoUpdate.js';
import { SidecarPanel } from './SidecarPanel.js';

interface Props {
  appState: AppState;
  onChangeBackend: () => void;
  /** Refresh the parent's `appState` after a write (e.g. the
   *  AutoUpdate toggle persists `autoUpdateEnabled` then asks the
   *  parent to re-fetch). */
  onAppStateRefresh: () => void | Promise<void>;
}

/** Sprint 6 PR 3 dashboard. Replaces the post-wizard App body.
 *  Sections:
 *  - `OverallHealthBadge` — green/amber/red mirroring the tray.
 *  - `BackendBanner` — currently-selected backend + Change link.
 *  - `SidecarPanel` — collector state, counts, Test Pipeline.
 *  - `HarnessList` — existing detected-harnesses list.
 *  - `LogsPanel` — live tail of collector.log. */
export function Dashboard({ appState, onChangeBackend, onAppStateRefresh }: Props): JSX.Element {
  const { status } = useCollectorStatus();
  const { snapshot } = useMetricsSnapshot();
  const { harnesses, loading, error, refresh } = useDetectedHarnesses();

  const [previewing, setPreviewing] = useState<HarnessId | null>(null);
  const [busyIds, setBusyIds] = useState<Set<HarnessId>>(() => new Set());
  const [revertError, setRevertError] = useState<TroveIpcError | null>(null);

  const enabledHarnessCount = appState.harnesses.filter((h) => h.enabled).length;
  const health = deriveOverallHealth(
    status?.state ?? { kind: 'idle' },
    snapshot,
    enabledHarnessCount,
  );
  const detail = badgeDetail(status, snapshot);

  const handleEnable = useCallback((id: HarnessId) => {
    setRevertError(null);
    setPreviewing(id);
  }, []);

  const handleDisable = useCallback(
    async (id: HarnessId) => {
      setRevertError(null);
      setBusyIds((prev) => {
        const next = new Set(prev);
        next.add(id);
        return next;
      });
      try {
        await revertPatch(id);
        await refresh();
      } catch (err) {
        if (err instanceof TroveIpcError) setRevertError(err);
      } finally {
        setBusyIds((prev) => {
          const next = new Set(prev);
          next.delete(id);
          return next;
        });
      }
    },
    [refresh],
  );

  const handleApplied = useCallback(async () => {
    setPreviewing(null);
    await refresh();
  }, [refresh]);

  return (
    <div data-testid="dashboard" className="flex flex-col gap-4">
      <OverallHealthBadge health={health} detail={detail} />

      <DiagnosticsPanel appState={appState} state={status?.state ?? null} metrics={snapshot} />

      {appState.backend ? (
        <BackendBanner backend={appState.backend} onChange={onChangeBackend} />
      ) : null}

      <SidecarPanel state={status?.state ?? null} metrics={snapshot} />

      {error ? (
        <p
          className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
          data-testid="harness-list-error"
        >
          Detection failed: {error.cause.kind}
        </p>
      ) : (
        <HarnessList
          harnesses={harnesses}
          loading={loading}
          onEnable={handleEnable}
          onDisable={(id) => void handleDisable(id)}
          busyIds={busyIds}
        />
      )}

      {revertError ? (
        <p
          className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
          data-testid="revert-error"
        >
          Revert failed: {revertError.cause.kind}
        </p>
      ) : null}

      <LogsPanel />

      <AutoUpdate
        enabled={appState.autoUpdateEnabled}
        onToggle={async (next) => {
          await setAutoUpdateEnabled(next);
          await onAppStateRefresh();
        }}
      />

      {previewing ? (
        <PatchPreviewModal
          harnessId={previewing}
          onClose={() => setPreviewing(null)}
          onApplied={() => void handleApplied()}
        />
      ) : null}
    </div>
  );
}

function badgeDetail(
  status: ReturnType<typeof useCollectorStatus>['status'],
  snapshot: ReturnType<typeof useMetricsSnapshot>['snapshot'],
): string | undefined {
  if (!status) return undefined;
  if (snapshot && snapshot.unreachable) return 'metrics endpoint unreachable';
  if (status.state.kind === 'failed') return status.state.reason;
  if (status.state.kind === 'crashed') return `${status.state.restarts} restart(s)`;
  return undefined;
}

function BackendBanner({
  backend,
  onChange,
}: {
  backend: Backend;
  onChange: () => void;
}): JSX.Element {
  const meta = presetMetadataFor(backend.kind);
  return (
    <p
      data-testid="backend-banner"
      className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-100 px-3 py-2 text-sm text-slate-800 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-200"
    >
      <span>
        Forwarding to <span className="font-medium">{meta.label}</span>
        <BackendDetail backend={backend} />
      </span>
      <button
        type="button"
        onClick={onChange}
        data-testid="backend-banner-change"
        className="text-xs text-blue-700 hover:text-blue-900 dark:text-blue-300 dark:hover:text-blue-100"
      >
        Change
      </button>
    </p>
  );
}

function BackendDetail({ backend }: { backend: Backend }): JSX.Element | null {
  switch (backend.kind) {
    case 'signoz':
      return <span className="text-slate-500"> ({backend.region})</span>;
    case 'honeycomb':
      return <span className="text-slate-500"> ({backend.dataset})</span>;
    case 'datadog':
      return <span className="text-slate-500"> ({backend.site})</span>;
    case 'grafana-cloud':
    case 'otlp-generic':
    case 'otelcol-passthrough':
      return null;
  }
}
