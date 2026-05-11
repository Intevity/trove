import { useCallback, useEffect, useRef, useState } from 'react';

import type { DetectedHarness } from '@trove/shared';

import { TroveIpcError, listDetectedHarnesses } from '../lib/ipc.js';

export interface DetectionState {
  harnesses: DetectedHarness[];
  loading: boolean;
  error: TroveIpcError | null;
  /** Re-run the detection sweep. The dashboard calls this after a
   *  user enables / disables a harness, or when the user clicks the
   *  list's Refresh button. */
  refresh: () => Promise<void>;
}

export interface UseDetectedHarnessesOptions {
  /** Re-scan whenever the window regains focus. Defaults to true.
   *  Lets tests opt out of the global listener and gives advanced
   *  consumers a way to disable the focus behaviour without forking. */
  refreshOnFocus?: boolean;
  /** Minimum interval between two focus-driven refreshes, in ms. The
   *  user clicking Refresh manually always runs immediately — only the
   *  focus listener coalesces, so rapid alt-tabs don't fan out into a
   *  burst of Tauri IPC calls. Defaults to 2_000. */
  focusCoalesceMs?: number;
}

/** Loads the list of detected harnesses on mount and exposes a
 *  refresh function. Stays deliberately minimal — no React Query, no
 *  global store. The wider app builds on this in later sprints.
 *
 *  Focus refresh covers the "user installed a harness while Trove was
 *  open" case. Most installs happen in a terminal; the user then
 *  switches back to the Trove window. The focus event triggers a
 *  re-scan so the row flips to detected without an explicit click. */
export function useDetectedHarnesses(options: UseDetectedHarnessesOptions = {}): DetectionState {
  const { refreshOnFocus = true, focusCoalesceMs = 2_000 } = options;
  const [harnesses, setHarnesses] = useState<DetectedHarness[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<TroveIpcError | null>(null);
  const lastRefreshAt = useRef<number>(0);

  const refresh = useCallback(async () => {
    lastRefreshAt.current = Date.now();
    setLoading(true);
    setError(null);
    try {
      const rows = await listDetectedHarnesses();
      setHarnesses(rows);
    } catch (err) {
      setError(err instanceof TroveIpcError ? err : null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!refreshOnFocus) return;
    if (typeof window === 'undefined') return;
    const handleFocus = (): void => {
      const elapsed = Date.now() - lastRefreshAt.current;
      if (elapsed < focusCoalesceMs) return;
      void refresh();
    };
    window.addEventListener('focus', handleFocus);
    return () => {
      window.removeEventListener('focus', handleFocus);
    };
  }, [refresh, refreshOnFocus, focusCoalesceMs]);

  return { harnesses, loading, error, refresh };
}
