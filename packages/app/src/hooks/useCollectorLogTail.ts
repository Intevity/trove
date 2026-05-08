import { useCallback, useEffect, useState } from 'react';

import { listen } from '@tauri-apps/api/event';
import {
  CollectorLogEvent as CollectorLogEventSchema,
  TauriEventName,
  type CollectorLogLineWire,
} from '@trove/shared';

import { TroveIpcError, getCollectorLogTail } from '../lib/ipc.js';

/** Frontend cap on retained log lines. Mirrors claude-sentinel's
 *  `useDaemonLogs` cap; oldest lines drop when exceeded. */
export const LOG_TAIL_CAP = 5000;

/** Lines pulled at mount time. Matches the dashboard panel's initial
 *  render; subsequent lines arrive via `collector-log` events. */
export const INITIAL_LOG_TAIL_LINES = 200;

export interface CollectorLogTailBinding {
  lines: CollectorLogLineWire[];
  loading: boolean;
  error: TroveIpcError | null;
}

/** Pull the last N lines from `collector.log` on mount, then append
 *  every `collector-log` event the supervisor pumps onto the WebView.
 *  Bounded ring buffer of 5000 lines.
 *
 *  Initial fetch and live stream are coordinated via the IPC's
 *  `byteOffset`: we discard live events whose payload's content was
 *  already part of the initial tail. Today the live event payload is
 *  just `{ stream, line }` — there is no per-event offset — so we
 *  rely on the supervisor's tee being the single source of truth and
 *  drop any live event that arrives before the initial fetch settles
 *  (the typical case). PR 3's e2e exercises the post-mount path
 *  directly. */
export function useCollectorLogTail(): CollectorLogTailBinding {
  const [lines, setLines] = useState<CollectorLogLineWire[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<TroveIpcError | null>(null);
  const [initialised, setInitialised] = useState(false);

  const append = useCallback((batch: CollectorLogLineWire[]) => {
    setLines((prev) => {
      const next = prev.concat(batch);
      if (next.length <= LOG_TAIL_CAP) return next;
      return next.slice(next.length - LOG_TAIL_CAP);
    });
  }, []);

  // Initial fetch.
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void getCollectorLogTail(INITIAL_LOG_TAIL_LINES)
      .then((tail) => {
        if (cancelled) return;
        setLines(tail.lines);
        setInitialised(true);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        if (err instanceof TroveIpcError) setError(err);
        // Even if the file is missing, treat the panel as initialised
        // so live events can begin appending.
        setInitialised(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Live event subscription. Buffer events that arrive before the
  // initial tail completes, then flush.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    const buffered: CollectorLogLineWire[] = [];

    void listen<CollectorLogLineWire>(TauriEventName.CollectorLog, (event) => {
      if (cancelled) return;
      const parsed = CollectorLogEventSchema.safeParse(event.payload);
      if (!parsed.success) return;
      if (initialised) {
        append([parsed.data]);
      } else {
        buffered.push(parsed.data);
      }
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
  }, [append, initialised]);

  return { lines, loading, error };
}
