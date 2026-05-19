import { useEffect, useState } from 'react';

import { listen } from '@tauri-apps/api/event';
import {
  BackendHealth as BackendHealthSchema,
  TauriEventName,
  type BackendHealth,
} from '@trove/shared';

import { getBackendHealth } from '../lib/ipc.js';

/** Per-destination health snapshot used to render the Platforms tab
 *  status pill. Initial value comes from `get_backend_health`; live
 *  updates stream in via the `backend-health` Tauri event (debounced
 *  ~250 ms on the Rust side so flurry-of-failures doesn't thrash the
 *  UI). Returns a `Map` keyed by `backendId` for O(1) lookups inside
 *  the destinations list render path. */
export function useBackendHealth(): Map<string, BackendHealth> {
  const [byId, setById] = useState<Map<string, BackendHealth>>(() => new Map());

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const initial = await getBackendHealth();
        if (cancelled) return;
        setById(toMap(initial));
      } catch {
        // First-render fetch failure is non-fatal — the event stream
        // will populate the map within one scrape tick (~5 s).
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen<unknown>(TauriEventName.BackendHealth, (event) => {
      if (cancelled) return;
      const parsed = BackendHealthSchema.array().safeParse(event.payload);
      if (!parsed.success) return;
      setById(toMap(parsed.data));
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

  return byId;
}

function toMap(list: BackendHealth[]): Map<string, BackendHealth> {
  const next = new Map<string, BackendHealth>();
  for (const entry of list) {
    next.set(entry.backendId, entry);
  }
  return next;
}
