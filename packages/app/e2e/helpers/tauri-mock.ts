/**
 * Sprint 6 PR 3 — Playwright Tauri shim.
 *
 * The dashboard-flow e2e exercises the React UI against a mocked
 * Tauri runtime so we can assert state transitions (wizard → enable
 * harness → Test Pipeline → green badge) without launching a real
 * native build. The shim installs `window.__TAURI_INTERNALS__` with
 * just enough surface to satisfy `@tauri-apps/api/core::invoke` and
 * `@tauri-apps/api/event::listen`:
 *
 *   - `transformCallback(cb)` stores the JS callback and returns an id.
 *   - `invoke('plugin:event|listen', ...)` registers a listener entry
 *     that maps event-name → handler id.
 *   - `invoke('plugin:event|unlisten', ...)` removes it.
 *   - `invoke(<app cmd>, args)` looks up a scripted handler.
 *
 * The test drives the in-memory state machine via two helpers exposed
 * on `window`:
 *
 *   - `window.__troveSetState(partial)` — merge into the mock store.
 *   - `window.__troveEmit(eventName, payload)` — fire a Tauri event.
 *
 * No Tauri-related modules are imported here so the helper can be
 * serialised straight into Playwright's `addInitScript`. */

import type { Page } from '@playwright/test';

export interface TauriMockState {
  appState: unknown;
  detectedHarnesses: unknown[];
  collectorStatus: unknown;
  metricsSnapshot: unknown;
  collectorLogTail: unknown;
  testExportResult: unknown;
}

export const DEFAULT_MOCK_STATE: TauriMockState = {
  appState: { schemaVersion: 2, backend: null, harnesses: [] },
  detectedHarnesses: [
    {
      id: 'claude-code',
      detected: true,
      configPath: '/tmp/test/.claude/settings.json',
      telemetry: 'off',
      detectionMethod: 'config-dir',
      troveRegionPresent: false,
    },
  ],
  collectorStatus: {
    state: { kind: 'idle' },
    logPath: '/tmp/test/collector.log',
  },
  metricsSnapshot: null,
  collectorLogTail: { lines: [], byteOffset: 0 },
  testExportResult: { status: 'ok', detail: 'received span at backend' },
};

/** Install the Tauri shim into `page` before any navigation. */
export async function installTauriMock(
  page: Page,
  state: Partial<TauriMockState> = {},
): Promise<void> {
  const initial = { ...DEFAULT_MOCK_STATE, ...state };
  await page.addInitScript((seeded: TauriMockState) => {
    interface ListenerEntry {
      event: string;
      handlerId: number;
    }
    interface Store {
      state: TauriMockState;
      callbacks: Map<number, (response: unknown) => void>;
      listeners: Map<number, ListenerEntry>;
      nextCallbackId: number;
      nextEventId: number;
    }
    const store: Store = {
      state: seeded,
      callbacks: new Map(),
      listeners: new Map(),
      nextCallbackId: 0,
      nextEventId: 0,
    };

    function asObject(args: unknown): Record<string, unknown> {
      return (args ?? {}) as Record<string, unknown>;
    }

    const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
      get_app_state: () => store.state.appState,
      list_detected_harnesses: () => store.state.detectedHarnesses,
      preview_patch: () => ({
        configPath: '/tmp/test/.claude/settings.json',
        format: 'json',
        before: '{}',
        after: '{ "_trove": {} }',
        status: 'fresh',
      }),
      apply_patch: (args: Record<string, unknown>) => {
        const id = String(args['harnessId'] ?? '');
        store.state.detectedHarnesses = (store.state.detectedHarnesses ?? []).map((h: unknown) => {
          const harness = h as Record<string, unknown>;
          if (harness['id'] === id) {
            return { ...harness, troveRegionPresent: true, telemetry: 'on' };
          }
          return harness;
        });
        const appState = store.state.appState as Record<string, unknown>;
        const harnesses = (appState['harnesses'] as unknown[]) ?? [];
        store.state.appState = {
          ...appState,
          harnesses: [
            ...harnesses,
            {
              id,
              enabled: true,
              configPath: '/tmp/test/.claude/settings.json',
              lastPatchedAt: new Date().toISOString(),
              trovePatch: {
                managedBlockHash: 'a'.repeat(64),
                fileHashAtLastWrite: 'b'.repeat(64),
                format: 'json',
              },
              options: { logUserPrompts: false, customAttributes: {} },
            },
          ],
        };
        return {
          managedBlockHash: 'a'.repeat(64),
          fileHashAtLastWrite: 'b'.repeat(64),
          format: 'json',
        };
      },
      revert_patch: () => null,
      save_backend: (args: Record<string, unknown>) => {
        const draft = args['draft'] as Record<string, unknown>;
        const backend: Record<string, unknown> = { kind: draft['kind'] };
        if (draft['kind'] === 'signoz') {
          backend['region'] = draft['region'];
          backend['ingestionKey'] = {
            service: 'trove',
            account: 'backend.signoz.ingestion-key',
          };
        }
        store.state.appState = { ...(store.state.appState as object), backend };
        return backend;
      },
      clear_backend: () => null,
      test_export: () => store.state.testExportResult,
      get_collector_status: () => store.state.collectorStatus,
      get_metrics_snapshot: () => store.state.metricsSnapshot,
      get_collector_log_tail: () => store.state.collectorLogTail,
      dev_set_tray_color: () => null,
    };

    function fire(eventName: string, payload: unknown): void {
      for (const [, entry] of store.listeners) {
        if (entry.event === eventName) {
          const cb = store.callbacks.get(entry.handlerId);
          if (cb) {
            cb({ event: eventName, id: entry.handlerId, payload });
          }
        }
      }
    }

    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
      transformCallback: (cb?: (response: unknown) => void): number => {
        const id = ++store.nextCallbackId;
        if (cb) store.callbacks.set(id, cb);
        return id;
      },
      invoke: async (cmd: string, args?: unknown): Promise<unknown> => {
        if (cmd === 'plugin:event|listen') {
          const a = asObject(args);
          const eventId = ++store.nextEventId;
          store.listeners.set(eventId, {
            event: String(a['event']),
            handlerId: Number(a['handler']),
          });
          return eventId;
        }
        if (cmd === 'plugin:event|unlisten') {
          const a = asObject(args);
          store.listeners.delete(Number(a['eventId']));
          return null;
        }
        const handler = handlers[cmd];
        if (!handler) throw new Error(`unmocked Tauri command: ${cmd}`);
        return handler(asObject(args));
      },
    };

    (window as unknown as Record<string, unknown>).__troveSetState = (
      partial: Partial<TauriMockState>,
    ): void => {
      Object.assign(store.state, partial);
    };
    (window as unknown as Record<string, unknown>).__troveEmit = (
      eventName: string,
      payload: unknown,
    ): void => {
      fire(eventName, payload);
    };
  }, initial);
}
