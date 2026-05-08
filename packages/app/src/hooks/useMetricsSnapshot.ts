import { useCallback, useEffect, useState } from 'react';

import { listen } from '@tauri-apps/api/event';
import {
  MetricsSnapshotWire as MetricsSnapshotWireSchema,
  TauriEventName,
  type MetricsSnapshotWire,
} from '@trove/shared';

import { TroveIpcError, getMetricsSnapshot } from '../lib/ipc.js';

export interface MetricsSnapshotBinding {
  /** `null` until the first scrape completes; subsequently the
   *  latest snapshot. The UI distinguishes "no scrape yet" (skeleton)
   *  from `unreachable: true` (amber callout). */
  snapshot: MetricsSnapshotWire | null;
  loading: boolean;
  error: TroveIpcError | null;
  refresh: () => Promise<void>;
}

/** Subscribe to live `metrics-snapshot` events with an initial fetch
 *  via `get_metrics_snapshot`. The Rust side debounces the metrics
 *  tap to a 5 s cadence — the dashboard rerenders no more often than
 *  that, regardless of how chatty the receiving side is. */
export function useMetricsSnapshot(): MetricsSnapshotBinding {
  const [snapshot, setSnapshot] = useState<MetricsSnapshotWire | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<TroveIpcError | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const fresh = await getMetricsSnapshot();
      setSnapshot(fresh);
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
    void listen<MetricsSnapshotWire>(TauriEventName.MetricsSnapshot, (event) => {
      if (cancelled) return;
      const parsed = MetricsSnapshotWireSchema.safeParse(event.payload);
      if (!parsed.success) return;
      setSnapshot(parsed.data);
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

  return { snapshot, loading, error, refresh };
}
