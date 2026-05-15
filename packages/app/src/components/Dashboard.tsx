import { useCallback, useState } from 'react';

import type { AppState, HarnessId } from '@trove/shared';

import { useCollectorStatus } from '../hooks/useCollectorStatus.js';
import { useDetectedHarnesses } from '../hooks/useDetectedHarnesses.js';
import { useMetricsSnapshot } from '../hooks/useMetricsSnapshot.js';
import { POST_ENABLE_ADVISORIES } from '../lib/harnessAdvisories.js';
import { deriveOverallHealth } from '../lib/health.js';
import { TroveIpcError, revertPatch } from '../lib/ipc.js';
import { AppHeader, type TabId } from './AppHeader.js';
import { Footer } from './Footer.js';
import { PatchPreviewModal } from './PatchPreviewModal.js';
import { Toast } from './Toast.js';
import { HarnessesTab } from './tabs/HarnessesTab.js';
import { LogsTab } from './tabs/LogsTab.js';
import { MappingsTab } from './tabs/MappingsTab.js';
import { OverviewTab } from './tabs/OverviewTab.js';
import { PlatformsTab } from './tabs/PlatformsTab.js';
import { SettingsTab } from './tabs/SettingsTab.js';

interface Props {
  appState: AppState;
  /** Jump straight to the Platforms tab — Overview surfaces this when
   *  zero platforms are configured so the user has an obvious next
   *  step. */
  onOpenPlatforms: () => void;
  /** Refresh the parent's `appState` after a write (e.g. the
   *  AutoUpdate toggle persists `autoUpdateEnabled` then asks the
   *  parent to re-fetch). */
  onAppStateRefresh: () => void | Promise<void>;
}

/** Post-wizard dashboard. Owns the tray-app shell (header / tab nav /
 *  scrolling pane / footer) and routes between four tabs:
 *  - Overview — health, diagnostics, backend, sidecar/collector
 *  - Harnesses — detected-harness list with enable/disable
 *  - Logs — live collector log tail (fills available height)
 *  - Settings — auto-update, identity tagging */
export function Dashboard({ appState, onOpenPlatforms, onAppStateRefresh }: Props): JSX.Element {
  const { status } = useCollectorStatus();
  const { snapshot } = useMetricsSnapshot();
  const { harnesses, loading, error, refresh } = useDetectedHarnesses();

  const [activeTab, setActiveTab] = useState<TabId>('overview');
  const [previewing, setPreviewing] = useState<HarnessId | null>(null);
  const [busyIds, setBusyIds] = useState<Set<HarnessId>>(() => new Set());
  const [revertError, setRevertError] = useState<TroveIpcError | null>(null);
  // Post-enable advisory toast (e.g. "restart Cursor"). The PatchPreviewModal
  // doesn't know which harness it patched, so we capture the id from
  // `previewing` at the moment `onApplied` fires and stash it here.
  const [toast, setToast] = useState<string | null>(null);

  const enabledHarnessCount = appState.harnesses.filter((h) => h.enabled).length;
  const state = status?.state ?? null;
  const metrics = snapshot ?? null;
  const health = deriveOverallHealth(state ?? { kind: 'idle' }, metrics, enabledHarnessCount);

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

  const handleApplied = useCallback(
    async (id: HarnessId) => {
      setPreviewing(null);
      const advisory = POST_ENABLE_ADVISORIES[id];
      if (advisory) setToast(advisory);
      await refresh();
    },
    [refresh],
  );

  return (
    <div data-testid="dashboard" className="flex h-full flex-col bg-canvas dark:bg-canvas-dark">
      <AppHeader health={health} activeTab={activeTab} onTabChange={setActiveTab} />

      <main className="flex-1 min-h-0 overflow-auto">
        {activeTab === 'overview' && (
          <OverviewTab
            appState={appState}
            state={state}
            metrics={metrics}
            onOpenPlatforms={() => {
              onOpenPlatforms();
              setActiveTab('platforms');
            }}
          />
        )}
        {activeTab === 'harnesses' && (
          <HarnessesTab
            harnesses={harnesses}
            loading={loading}
            detectionError={error}
            busyIds={busyIds}
            revertError={revertError}
            onEnable={handleEnable}
            onDisable={(id) => void handleDisable(id)}
            onRefresh={() => void refresh()}
          />
        )}
        {activeTab === 'platforms' && (
          <PlatformsTab appState={appState} onAppStateRefresh={onAppStateRefresh} />
        )}
        {activeTab === 'mappings' && (
          <MappingsTab appState={appState} onAppStateRefresh={onAppStateRefresh} />
        )}
        {activeTab === 'logs' && <LogsTab />}
        {activeTab === 'settings' && (
          <SettingsTab appState={appState} onAppStateRefresh={onAppStateRefresh} />
        )}
      </main>

      <Footer />

      {previewing ? (
        <PatchPreviewModal
          harnessId={previewing}
          onClose={() => setPreviewing(null)}
          onApplied={() => void handleApplied(previewing)}
        />
      ) : null}

      {toast ? <Toast message={toast} onDismiss={() => setToast(null)} /> : null}
    </div>
  );
}
