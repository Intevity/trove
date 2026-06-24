import { useCallback } from 'react';

import { BackendWizard } from './components/wizard/BackendWizard.js';
import { Dashboard } from './components/Dashboard.js';
import { StateRecoveryNotice } from './components/StateRecoveryNotice.js';
import { useAppState } from './hooks/useAppState.js';

export function App(): JSX.Element {
  const {
    appState,
    loading: appStateLoading,
    error: appStateError,
    refresh: refreshAppState,
  } = useAppState();

  const handleWizardComplete = useCallback(async () => {
    await refreshAppState();
  }, [refreshAppState]);

  // A load *failure* (e.g. state.json written by a newer Trove build) is
  // NOT a first run — the user's data is intact on disk. Render a
  // data-safe recovery notice instead of the wizard, which would imply
  // their configuration was wiped.
  if (!appStateLoading && appStateError) {
    return <StateRecoveryNotice error={appStateError} onRetry={() => void refreshAppState()} />;
  }

  // First-run: the wizard takes over until the user has configured at
  // least one platform. We only treat a *successfully loaded* state with
  // no destinations as first-run, so a failed load never flashes the
  // wizard. We wait for state.json to load so we don't flash the wizard
  // for users who already have a destination saved.
  const showWizard = !appStateLoading && appState !== null && appState.backends.length === 0;

  if (showWizard) {
    return (
      <main className="h-full overflow-y-auto text-slate-900 antialiased dark:text-slate-100">
        <div className="mx-auto flex max-w-3xl flex-col items-stretch gap-6 px-8 py-12">
          <header>
            <h1 data-testid="app-header" className="text-3xl font-semibold tracking-tight">
              Trove
            </h1>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              Pick a destination for your AI coding harness telemetry; Trove forwards every harness
              through a local collector to whichever backend you choose. You can add more platforms
              later from the Platforms tab.
            </p>
          </header>

          <BackendWizard onComplete={() => void handleWizardComplete()} />
        </div>
      </main>
    );
  }

  if (!appState) return <></>;

  return (
    <Dashboard
      appState={appState}
      // No-op deep link target — Dashboard switches its own activeTab
      // when the user wants to manage platforms; this callback exists
      // so OverviewTab can request the jump without owning the tab
      // state.
      onOpenPlatforms={() => {}}
      onAppStateRefresh={() => void refreshAppState()}
    />
  );
}
