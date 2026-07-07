/*
 * Trove capture mock — installed via Playwright addInitScript before the app loads
 * (see `curtain capture`). It stands in for the Tauri backend: a fake
 * `__TAURI_INTERNALS__.invoke` answering every command a demoed view calls, an
 * event bus, and the two window hooks the capture recipes drive:
 *
 *   window.__troveSetState(partial)   merge demo state (recipe ctx.seed)
 *   window.__troveEmit({event,payload}) fire a mock event (recipe ctx.emit)
 *
 * This is ONE comprehensive, schema-valid seed shared by all recipes; each recipe
 * tweaks it per-clip via seed()/emit(). It is redacted demo data only — no real
 * tokens/org/tenant, identity is "Demo User <demo@trove.dev>", paths use /Users/demo.
 *
 * Real-time capture is Curtain's deliberate exception to the determinism rule; this
 * browser script is injected during a live recording, never into the render IR.
 */
/* global window */
(function installTroveMock() {
  var ISO = '2026-07-01T12:00:00+00:00';
  var HASH_A = 'a'.repeat(64);
  var HASH_B = 'b'.repeat(64);
  var PATCH = {
    managedBlockHash: HASH_A,
    fileHashAtLastWrite: HASH_B,
    format: 'json',
    lastWrittenRegionPayload: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}',
  };
  var counts = function (s, m, l) {
    return { spans: s, metricPoints: m, logRecords: l };
  };
  var harnessRow = function (id, telemetry, detectionMethod) {
    return {
      id: id,
      detected: true,
      configPath: '/Users/demo/.config/' + id + '.json',
      telemetry: telemetry,
      detectionMethod: detectionMethod,
      troveRegionPresent: telemetry === 'on',
      adapterAvailable: true,
    };
  };
  var enabledHarness = function (id) {
    return {
      id: id,
      enabled: true,
      configPath: '/Users/demo/.config/' + id + '.json',
      lastPatchedAt: ISO,
      trovePatch: PATCH,
      options: { customAttributes: {} },
    };
  };

  var seed = {
    appState: {
      schemaVersion: 11,
      backends: [
        {
          id: 'be-signoz',
          label: 'SigNoz Cloud',
          enabled: true,
          backend: {
            kind: 'signoz',
            endpoint: 'http://localhost:14318',
            ingestionKey: { service: 'trove', account: 'backend.signoz.ingestion-key' },
          },
        },
        {
          id: 'be-grafana',
          label: 'Grafana Cloud',
          enabled: true,
          backend: {
            kind: 'grafana-cloud',
            endpoint: 'http://localhost:14319',
            auth: { service: 'trove', account: 'backend.grafana.auth' },
          },
        },
      ],
      harnesses: [
        enabledHarness('claude-code'),
        enabledHarness('codex-cli'),
        enabledHarness('cursor-ide'),
      ],
      autoUpdateEnabled: false,
      launchAtStartupEnabled: true,
      identity: { enabled: true, source: 'manual', name: 'Demo User', email: 'demo@trove.dev' },
      // Populated so the Mappings-tab clips (metrics / mappings / cost-normalization /
      // best-effort-adapter) render on navigation — appState re-reads only on an IPC
      // refresh, but the initial load picks up this seed. Five Tier A catalog metrics +
      // synthesis rules for two native harnesses + hook rules for a best-effort one.
      mappings: {
        schemaVersion: 2,
        metrics: [
          {
            id: 'events',
            name: 'trove.harness.events',
            kind: 'counter',
            description: '',
            requiredAttributes: [],
            builtin: true,
          },
          {
            id: 'tokens',
            name: 'trove.harness.tokens',
            kind: 'counter',
            description: '',
            requiredAttributes: [],
            builtin: true,
          },
          {
            id: 'cost.usd',
            name: 'trove.harness.cost.usd',
            kind: 'counter',
            description: '',
            requiredAttributes: [],
            builtin: true,
          },
          {
            id: 'turn.duration',
            name: 'trove.harness.turn.duration',
            kind: 'histogram',
            description: '',
            requiredAttributes: [],
            builtin: true,
          },
          {
            id: 'errors',
            name: 'trove.harness.errors',
            kind: 'counter',
            description: '',
            requiredAttributes: [],
            builtin: true,
          },
        ],
        harnesses: [
          {
            harnessId: 'claude-code',
            enabled: true,
            costOverrides: {},
            sources: [
              {
                kind: 'synthesize-from-native',
                nativeMetric: 'gen_ai.client.operation.count',
                targetMetric: 'events',
                attributeMap: {},
                injectAttributes: {},
              },
              {
                kind: 'synthesize-from-native',
                nativeMetric: 'gen_ai.client.token.usage',
                targetMetric: 'tokens',
                attributeMap: {},
                injectAttributes: {},
              },
              {
                kind: 'synthesize-from-native',
                nativeMetric: 'gen_ai.client.cost',
                targetMetric: 'cost.usd',
                attributeMap: {},
                injectAttributes: {},
              },
              {
                kind: 'synthesize-from-native',
                nativeMetric: 'gen_ai.client.operation.duration',
                targetMetric: 'turn.duration',
                attributeMap: {},
                injectAttributes: {},
              },
            ],
          },
          {
            harnessId: 'codex-cli',
            enabled: true,
            costOverrides: {},
            sources: [
              {
                kind: 'synthesize-from-native',
                nativeMetric: 'gen_ai.client.token.usage',
                targetMetric: 'tokens',
                attributeMap: {},
                injectAttributes: {},
              },
              {
                kind: 'synthesize-from-native',
                nativeMetric: 'gen_ai.client.cost',
                targetMetric: 'cost.usd',
                attributeMap: {},
                injectAttributes: {},
              },
            ],
          },
          {
            harnessId: 'aider',
            enabled: true,
            costOverrides: {},
            sources: [
              {
                kind: 'hook-rule',
                when: 'session.end',
                emit: { metric: 'events', attributes: {} },
              },
              { kind: 'hook-rule', when: 'tool.call', emit: { metric: 'tokens', attributes: {} } },
            ],
          },
        ],
      },
      telemetryObserved: {
        'claude-code': 1751371200,
        'codex-cli': 1751371200,
        'cursor-ide': 1751371200,
      },
    },
    detectedHarnesses: [
      harnessRow('claude-code', 'on', 'config-dir'),
      harnessRow('codex-cli', 'on', 'config-dir'),
      harnessRow('cursor-ide', 'on', 'config-dir'),
      harnessRow('qwen-code', 'off', 'config-dir'),
      harnessRow('aider', 'unknown', 'path-binary'),
    ],
    collectorStatus: {
      state: { kind: 'running', pid: 4242, restarts: 0 },
      logPath: '/Users/demo/Library/Application Support/trove/collector.log',
    },
    metricsSnapshot: {
      received: counts(128, 64, 32),
      sent: counts(128, 64, 32),
      lastSignalMsAgo: 3000,
      scrapedMsAgo: 500,
      unreachable: false,
      overallHealth: 'green',
      diagObservations: { 'claude-code': counts(60, 30, 12) },
    },
    collectorLogTail: {
      lines: [
        { stream: 'stdout', line: 'collector listening on 127.0.0.1:14317' },
        { stream: 'stdout', line: 'collector listening on 127.0.0.1:14318' },
      ],
      byteOffset: 128,
    },
    backendHealth: [
      {
        backendId: 'be-signoz',
        status: 'green',
        lastSuccessAt: ISO,
        windowSent: 128,
        windowFailed: 0,
      },
      {
        backendId: 'be-grafana',
        status: 'green',
        lastSuccessAt: ISO,
        windowSent: 128,
        windowFailed: 0,
      },
    ],
    testExportResult: { status: 'ok', detail: 'received span at backend' },
  };

  var store = { state: seed, callbacks: new Map(), listeners: new Map(), nextCb: 0, nextEv: 0 };
  var asObj = function (a) {
    return a || {};
  };
  var appState = function () {
    return store.state.appState;
  };

  var handlers = {
    get_app_state: function () {
      return store.state.appState;
    },
    list_detected_harnesses: function () {
      return store.state.detectedHarnesses;
    },
    get_collector_status: function () {
      return store.state.collectorStatus;
    },
    get_metrics_snapshot: function () {
      return store.state.metricsSnapshot;
    },
    get_collector_log_tail: function () {
      return store.state.collectorLogTail;
    },
    get_backend_health: function () {
      return store.state.backendHealth || [];
    },
    test_export: function () {
      return store.state.testExportResult || { status: 'ok', detail: 'received span at backend' };
    },
    preview_patch: function () {
      return {
        configPath: '/Users/demo/.claude/settings.json',
        format: 'json',
        before: '{}',
        after: '{ "_trove": {} }',
        status: 'fresh',
      };
    },
    apply_patch: function () {
      return PATCH;
    },
    revert_patch: function () {
      return null;
    },
    resolve_conflict: function () {
      return { status: 'marked-mine', patch: PATCH };
    },
    add_backend: function () {
      return appState().backends[0];
    },
    update_backend: function (a) {
      return { id: String(a.id || 'be-1'), backend: { kind: 'signoz' } };
    },
    remove_backend: function () {
      return null;
    },
    clear_backend: function () {
      return null;
    },
    set_backend_enabled: function (a) {
      var id = String(a.id || '');
      var en = a.enabled !== false;
      store.state.appState = Object.assign({}, appState(), {
        backends: (appState().backends || []).map(function (b) {
          return b.id === id ? Object.assign({}, b, { enabled: en }) : b;
        }),
      });
      return null;
    },
    apply_mappings: function () {
      return { status: 'ok' };
    },
    reset_mappings_to_defaults: function () {
      return appState().mappings;
    },
    simulate_mapping: function () {
      return { ok: true, output: [] };
    },
    set_auto_update_enabled: function () {
      return null;
    },
    set_launch_at_startup_enabled: function () {
      return null;
    },
    check_for_updates: function () {
      return { status: 'up-to-date' };
    },
    set_identity_enabled: function () {
      return appState().identity;
    },
    set_identity_manual: function () {
      return appState().identity;
    },
    set_identity_auto: function () {
      return appState().identity;
    },
    resolve_identity_preview: function () {
      return { name: 'Demo User', email: 'demo@trove.dev' };
    },
    quit_app: function () {
      return null;
    },
    uninstall_app: function () {
      return null;
    },
    install_update: function () {
      return null;
    },
  };

  function fire(event, payload) {
    store.listeners.forEach(function (e) {
      if (e.event === event) {
        var cb = store.callbacks.get(e.handlerId);
        if (cb) cb({ event: event, id: e.handlerId, payload: payload });
      }
    });
  }

  window.__TAURI_INTERNALS__ = {
    transformCallback: function (cb) {
      var id = ++store.nextCb;
      if (cb) store.callbacks.set(id, cb);
      return id;
    },
    invoke: function (cmd, args) {
      var a = asObj(args);
      if (cmd === 'plugin:event|listen') {
        var id = ++store.nextEv;
        store.listeners.set(id, { event: String(a.event), handlerId: Number(a.handler) });
        return Promise.resolve(id);
      }
      if (cmd === 'plugin:event|unlisten') {
        store.listeners.delete(Number(a.eventId));
        return Promise.resolve(null);
      }
      var h = handlers[cmd];
      if (!h) return Promise.reject({ kind: 'internal', reason: 'unmocked: ' + cmd });
      return Promise.resolve(h(a));
    },
    convertFileSrc: function (p) {
      return p;
    },
  };
  // @tauri-apps/api event `_unlisten` calls this before invoke('plugin:event|unlisten').
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: function () {} };

  // capture ctx.seed(partial) delivers `partial` directly; ctx.emit(event,payload)
  // delivers a single { event, payload } object — match that convention here.
  window.__troveSetState = function (partial) {
    if (!partial) return;
    // appState is a nested object the app re-reads wholesale; merge one level so a
    // recipe can seed just `{ appState: { mappings } }` without dropping backends/
    // harnesses/identity. Other top-level keys (detectedHarnesses, metricsSnapshot,
    // …) are replaced wholesale — recipes pass the full value.
    if (partial.appState) {
      store.state.appState = Object.assign({}, store.state.appState, partial.appState);
      var rest = Object.assign({}, partial);
      delete rest.appState;
      Object.assign(store.state, rest);
    } else {
      Object.assign(store.state, partial);
    }
  };
  window.__troveEmit = function (a) {
    fire(a.event, a.payload);
  };
})();
