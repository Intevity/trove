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
  /** Sprint 8 — when set, the apply_patch handler throws this value
   *  exactly once (then clears it). Used by the conflict-flow e2e to
   *  surface a region-conflict-detected error from a single click. */
  applyPatchError?: unknown;
  /** Sprint 8 — scripted return for the resolve_conflict handler. */
  resolveConflictOutcome?: unknown;
}

export const DEFAULT_MOCK_STATE: TauriMockState = {
  appState: {
    schemaVersion: 8,
    backends: [],
    harnesses: [],
    autoUpdateEnabled: false,
    launchAtStartupEnabled: true,
    identity: { enabled: false, source: 'auto', name: '', email: '' },
    mappings: { schemaVersion: 2, metrics: [], harnesses: [] },
    telemetryObserved: {},
  },
  detectedHarnesses: [
    {
      id: 'claude-code',
      detected: true,
      configPath: '/tmp/test/.claude/settings.json',
      telemetry: 'off',
      detectionMethod: 'config-dir',
      troveRegionPresent: false,
      adapterAvailable: true,
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
        // Sprint 8 — single-shot conflict injection. If the test seeded
        // `applyPatchError`, throw it now and clear it so the next
        // apply (post-resolution) takes the success path.
        if (store.state.applyPatchError !== undefined) {
          const err = store.state.applyPatchError;
          store.state.applyPatchError = undefined;
          // eslint-disable-next-line @typescript-eslint/no-throw-literal
          throw err;
        }
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
                lastWrittenRegionPayload:
                  '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}',
              },
              options: { customAttributes: {} },
            },
          ],
        };
        return {
          managedBlockHash: 'a'.repeat(64),
          fileHashAtLastWrite: 'b'.repeat(64),
          format: 'json',
          lastWrittenRegionPayload:
            '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}',
        };
      },
      revert_patch: () => null,
      resolve_conflict: () =>
        store.state.resolveConflictOutcome ?? {
          status: 'marked-mine',
          patch: {
            managedBlockHash: 'a'.repeat(64),
            fileHashAtLastWrite: 'b'.repeat(64),
            format: 'json',
            lastWrittenRegionPayload:
              '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://attacker.example.com"}}',
          },
        },
      add_backend: (args: Record<string, unknown>) => {
        const draft = args['draft'] as Record<string, unknown>;
        const label = args['label'] as string | null | undefined;
        const backend: Record<string, unknown> = { kind: draft['kind'] };
        if (draft['kind'] === 'signoz') {
          backend['endpoint'] = draft['endpoint'];
          backend['ingestionKey'] = {
            service: 'trove',
            account: 'backend.signoz.ingestion-key',
          };
        }
        const instance: Record<string, unknown> = {
          id: '11111111-1111-1111-1111-111111111111',
          backend,
        };
        if (label) instance['label'] = label;
        const appState = store.state.appState as Record<string, unknown>;
        const backends = (appState['backends'] as unknown[]) ?? [];
        store.state.appState = { ...appState, backends: [...backends, instance] };
        return instance;
      },
      update_backend: (args: Record<string, unknown>) => {
        const id = String(args['id'] ?? '');
        const draft = args['draft'] as Record<string, unknown>;
        const label = args['label'] as string | null | undefined;
        const backend: Record<string, unknown> = { kind: draft['kind'] };
        if (draft['kind'] === 'signoz') {
          backend['endpoint'] = draft['endpoint'];
          backend['ingestionKey'] = {
            service: 'trove',
            account: 'backend.signoz.ingestion-key',
          };
        }
        const instance: Record<string, unknown> = { id, backend };
        if (label) instance['label'] = label;
        const appState = store.state.appState as Record<string, unknown>;
        const backends = (appState['backends'] as unknown[]) ?? [];
        store.state.appState = {
          ...appState,
          backends: backends.map((b) =>
            (b as Record<string, unknown>)['id'] === id ? instance : b,
          ),
        };
        return instance;
      },
      remove_backend: (args: Record<string, unknown>) => {
        const id = String(args['id'] ?? '');
        const appState = store.state.appState as Record<string, unknown>;
        const backends = (appState['backends'] as unknown[]) ?? [];
        store.state.appState = {
          ...appState,
          backends: backends.filter((b) => (b as Record<string, unknown>)['id'] !== id),
        };
        return null;
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
