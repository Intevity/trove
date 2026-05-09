import { useCallback } from 'react';

import { BackendWizard } from './components/wizard/BackendWizard.js';
import { Dashboard } from './components/Dashboard.js';
import { useAppState } from './hooks/useAppState.js';
import { clearBackend } from './lib/ipc.js';

export function App(): JSX.Element {
  const { appState, loading: appStateLoading, refresh: refreshAppState } = useAppState();

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
      <div className="mx-auto flex min-h-screen max-w-3xl flex-col items-stretch gap-6 px-8 py-12">
        <header>
          <h1 data-testid="app-header" className="text-3xl font-semibold tracking-tight">
            Trove
          </h1>
          <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
            {showWizard
              ? 'Pick a destination for your AI coding harness telemetry — Trove forwards every harness through a local collector to whichever backend you choose.'
              : 'Live status for the local collector and detected AI coding harnesses.'}
          </p>
        </header>

        {showWizard ? (
          <BackendWizard onComplete={() => void handleWizardComplete()} />
        ) : appState ? (
          <Dashboard
            appState={appState}
            onChangeBackend={() => void handleChangeBackend()}
            onAppStateRefresh={() => void refreshAppState()}
          />
        ) : null}
      </div>
    </main>
  );
}
