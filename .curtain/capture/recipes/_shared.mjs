/*
 * Shared builders for Trove capture recipes. Recipes that flip harness state at
 * runtime must c.seed the END-state before the click (the mock re-reads on the
 * click's refresh; it does not mutate itself). The Mappings-tab clips need no seed
 * — the catalog + rules live in the shared init.js seed, loaded at startup.
 *
 * All shapes are schema-valid against packages/shared/src/schemas.ts.
 */

const PATCH = {
  managedBlockHash: 'a'.repeat(64),
  fileHashAtLastWrite: 'b'.repeat(64),
  format: 'json',
  lastWrittenRegionPayload: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}',
};

/** An enabled harness config (AppState.harnesses entry). */
export const enabledHarness = (id) => ({
  id,
  enabled: true,
  configPath: '/Users/demo/.config/' + id + '.json',
  lastPatchedAt: '2026-07-01T12:00:00+00:00',
  trovePatch: PATCH,
  options: { customAttributes: {} },
});

/** A detected-harness row (list_detected_harnesses entry). */
export const detectedRow = (id, telemetry, detectionMethod, extra = {}) => ({
  id,
  detected: true,
  configPath: '/Users/demo/.config/' + id + '.json',
  telemetry,
  detectionMethod,
  troveRegionPresent: telemetry === 'on',
  adapterAvailable: true,
  ...extra,
});

/** The standard detected set; `over.qwen` overrides qwen-code's telemetry ('off' default). */
export const baseDetected = (over = {}) => [
  detectedRow('claude-code', 'on', 'config-dir'),
  detectedRow('codex-cli', 'on', 'config-dir'),
  detectedRow('cursor-ide', 'on', 'config-dir'),
  detectedRow('qwen-code', over.qwen || 'off', 'config-dir'),
  detectedRow('aider', 'unknown', 'path-binary'),
];

/** The three base enabled harness configs, plus any extra ids you pass. */
export const baseEnabled = (extra = []) => [
  enabledHarness('claude-code'),
  enabledHarness('codex-cli'),
  enabledHarness('cursor-ide'),
  ...extra.map(enabledHarness),
];

/** A rising metrics snapshot (received === sent) — emit a few to animate counters + flow. */
export const risingSnapshot = (n) => {
  const received = { spans: 128 + n * 12, metricPoints: 64 + n * 6, logRecords: 32 + n * 3 };
  return {
    received,
    sent: received,
    lastSignalMsAgo: 400,
    scrapedMsAgo: 300,
    unreachable: false,
    overallHealth: 'green',
    diagObservations: {
      'claude-code': { spans: 60 + n * 6, metricPoints: 30 + n * 3, logRecords: 12 + n },
    },
  };
};
