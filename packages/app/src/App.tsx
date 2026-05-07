import { useCallback, useState } from 'react';

import type { HarnessId } from '@trove/shared';

import { HarnessList } from './components/HarnessList.js';
import { PatchPreviewModal } from './components/PatchPreviewModal.js';
import { useDetectedHarnesses } from './hooks/useDetectedHarnesses.js';
import { TroveIpcError, revertPatch } from './lib/ipc.js';

export function App(): JSX.Element {
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

  return (
    <main className="min-h-screen bg-slate-50 text-slate-900 antialiased dark:bg-slate-950 dark:text-slate-100">
      <div className="mx-auto flex min-h-screen max-w-2xl flex-col items-stretch gap-6 px-8 py-12">
        <header>
          <h1 data-testid="app-header" className="text-3xl font-semibold tracking-tight">
            Trove
          </h1>
          <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
            Detected AI coding harnesses on this machine. Patching Claude Code and Gemini CLI ships
            in Sprint 3 PRs 2 and 3; the remaining adapters land in Sprint 4 and beyond.
          </p>
        </header>

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
      </div>
    </main>
  );
}
