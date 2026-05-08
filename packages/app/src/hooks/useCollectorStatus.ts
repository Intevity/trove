import { useCallback, useEffect, useState } from 'react';

import { listen } from '@tauri-apps/api/event';
import {
  CollectorRunState as CollectorRunStateSchema,
  TauriEventName,
  type CollectorRunState,
  type CollectorStatus,
} from '@trove/shared';

import { TroveIpcError, getCollectorStatus } from '../lib/ipc.js';

export interface CollectorStatusBinding {
  status: CollectorStatus | null;
  loading: boolean;
  error: TroveIpcError | null;
  refresh: () => Promise<void>;
}

/** Snapshot the collector supervisor's current run state and stream
 *  live transitions in via the `collector-state` Tauri event. The
 *  initial fetch returns the persisted log path too — the dashboard's
 *  logs panel uses it as a stable identifier even if the path
 *  changes across reloads. */
export function useCollectorStatus(): CollectorStatusBinding {
  const [status, setStatus] = useState<CollectorStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<TroveIpcError | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const fresh = await getCollectorStatus();
      setStatus(fresh);
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
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen<CollectorRunState>(TauriEventName.CollectorState, (event) => {
      if (cancelled) return;
      // Validate at the boundary so a Rust-side rename surfaces here
      // rather than as a silent type lie.
      const parsed = CollectorRunStateSchema.safeParse(event.payload);
      if (!parsed.success) return;
      setStatus((prev) =>
        prev ? { ...prev, state: parsed.data } : { state: parsed.data, logPath: '' },
      );
    }).then((un) => {
      if (cancelled) {
        un();
      } else {
        unlisten = un;
      }
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  return { status, loading, error, refresh };
}
