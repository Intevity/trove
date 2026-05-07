import { useCallback, useEffect, useState } from 'react';

import type { AppState } from '@trove/shared';

import { TroveIpcError, getAppState } from '../lib/ipc.js';

export interface AppStateBinding {
  appState: AppState | null;
  loading: boolean;
  error: TroveIpcError | null;
  /** Re-read state.json. Called after the wizard saves a backend so
   *  App.tsx can swap from the wizard view to the dashboard. */
  refresh: () => Promise<void>;
}

/** Load the persisted `AppState` (state.json) on mount. Mirrors the
 *  `useDetectedHarnesses` shape — same loading/error pattern, same
 *  refresh idiom. The wizard mounts when `appState.backend === null`. */
export function useAppState(): AppStateBinding {
  const [appState, setAppState] = useState<AppState | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<TroveIpcError | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const state = await getAppState();
      setAppState(state);
    } catch (err) {
      setError(err instanceof TroveIpcError ? err : null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { appState, loading, error, refresh };
}
