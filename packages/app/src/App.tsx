import { useCallback, useState } from 'react';

import type { Backend, HarnessId } from '@trove/shared';
import { presetMetadataFor } from '@trove/collector-presets';

import { BackendWizard } from './components/wizard/BackendWizard.js';
import { HarnessList } from './components/HarnessList.js';
import { PatchPreviewModal } from './components/PatchPreviewModal.js';
import { useAppState } from './hooks/useAppState.js';
import { useDetectedHarnesses } from './hooks/useDetectedHarnesses.js';
import { TroveIpcError, clearBackend, revertPatch } from './lib/ipc.js';

export function App(): JSX.Element {
  const { appState, loading: appStateLoading, refresh: refreshAppState } = useAppState();
  const { harnesses, loading, error, refresh } = useDetectedHarnesses();
  const [previewing, setPreviewing] = useState<HarnessId | null>(null);
  const [busyIds, setBusyIds] = useState<Set<HarnessId>>(() => new Set());
  const [revertError, setRevertError] = useState<TroveIpcError | null>(null);

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

  const handleWizardComplete = useCallback(async () => {
    await refreshAppState();
  }, [refreshAppState]);

  const handleChangeBackend = useCallback(async () => {
    await clearBackend();
    await refreshAppState();
  }, [refreshAppState]);

  // First-run: the wizard takes over until a backend is saved. We wait
  // for state.json to load so we don't flash the wizard for users with
  // an existing backend.
  const showWizard = !appStateLoading && (appState === null || appState.backend === null);

  return (
    <main className="min-h-screen bg-slate-50 text-slate-900 antialiased dark:bg-slate-950 dark:text-slate-100">
      <div className="mx-auto flex min-h-screen max-w-2xl flex-col items-stretch gap-6 px-8 py-12">
        <header>
          <h1 data-testid="app-header" className="text-3xl font-semibold tracking-tight">
            Trove
          </h1>
          <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
            {showWizard
              ? 'Pick a destination for your AI coding harness telemetry — Trove forwards every harness through a local collector to whichever backend you choose.'
              : 'Detected AI coding harnesses on this machine.'}
          </p>
        </header>

        {showWizard ? (
          <BackendWizard onComplete={() => void handleWizardComplete()} />
        ) : (
          <>
            {appState?.backend ? (
              <BackendBanner
                backend={appState.backend}
                onChange={() => void handleChangeBackend()}
              />
            ) : null}

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

            {previewing ? (
              <PatchPreviewModal
                harnessId={previewing}
                onClose={() => setPreviewing(null)}
                onApplied={() => void handleApplied()}
              />
            ) : null}
          </>
        )}
      </div>
    </main>
  );
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
