import { useCallback, useEffect, useState } from 'react';

import type { DetectedHarness } from '@trove/shared';

import { TroveIpcError, listDetectedHarnesses } from '../lib/ipc.js';

export interface DetectionState {
  harnesses: DetectedHarness[];
  loading: boolean;
  error: TroveIpcError | null;
  /** Re-run the detection sweep. The dashboard calls this after a
   *  user enables / disables a harness so the row's status updates. */
  refresh: () => Promise<void>;
}

/** Loads the list of detected harnesses on mount and exposes a
 *  refresh function. Stays deliberately minimal — no React Query, no
 *  global store. The wider app builds on this in later sprints. */
export function useDetectedHarnesses(): DetectionState {
  const [harnesses, setHarnesses] = useState<DetectedHarness[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<TroveIpcError | null>(null);

  const refresh = useCallback(async () => {
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

  return { harnesses, loading, error, refresh };
}
