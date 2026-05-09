import { z } from 'zod';

/** Discriminated union of supported AI coding harness identifiers (MVP set). */
export const HarnessId = z.enum([
  'claude-code',
  'gemini-cli',
  'codex-cli',
  'qwen-code',
  'opencode',
  'cursor-ide',
  'cursor-cli',
  'cline',
  'aider',
  'copilot-cli',
]);
export type HarnessId = z.infer<typeof HarnessId>;

/** Opaque keychain handle. The actual secret never leaves the OS keychain. */
export const SecretRef = z.object({
  service: z.string().min(1),
  account: z.string().min(1),
});
export type SecretRef = z.infer<typeof SecretRef>;

/** Backend destination kinds. Sprint 5 adds full per-kind credential schemas. */
export const Backend = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('signoz'),
    region: z.string().min(1),
    ingestionKey: SecretRef,
  }),
  z.object({
    kind: z.literal('honeycomb'),
    team: SecretRef,
    dataset: z.string().min(1),
  }),
  z.object({
    kind: z.literal('grafana-cloud'),
    endpoint: z.string().url(),
    auth: SecretRef,
  }),
  z.object({
    kind: z.literal('datadog'),
    site: z.string().min(1),
    apiKey: SecretRef,
  }),
  z.object({
    kind: z.literal('otlp-generic'),
    endpoint: z.string().url(),
    protocol: z.enum(['grpc', 'http']),
    headers: z.record(z.string(), SecretRef),
  }),
  z.object({
    kind: z.literal('otelcol-passthrough'),
    endpoint: z.string().url(),
  }),
]);
export type Backend = z.infer<typeof Backend>;

/** Wire-format draft of {@link Backend} with raw secret values inline.
 *  Used **only** for the `save_backend` IPC payload — never persisted.
 *  The Rust handler stores each secret in the OS keychain, replaces it
 *  with a {@link SecretRef}, and writes the resulting `Backend` to
 *  `state.json`. Defining the raw-secret schema explicitly (rather than
 *  reusing `Backend` with a generic) keeps the IPC boundary
 *  unmistakable: any call site that constructs `BackendDraft` is by
 *  definition holding a raw secret in memory.
 *  Mirrors the Rust `BackendDraft` enum in `app_state::mod.rs`. */
export const BackendDraft = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('signoz'),
    region: z.string().min(1),
    ingestionKey: z.string().min(1),
  }),
  z.object({
    kind: z.literal('honeycomb'),
    team: z.string().min(1),
    dataset: z.string().min(1),
  }),
  z.object({
    kind: z.literal('grafana-cloud'),
    endpoint: z.string().url(),
    auth: z.string().min(1),
  }),
  z.object({
    kind: z.literal('datadog'),
    site: z.string().min(1),
    apiKey: z.string().min(1),
  }),
  z.object({
    kind: z.literal('otlp-generic'),
    endpoint: z.string().url(),
    protocol: z.enum(['grpc', 'http']),
    headers: z.record(z.string(), z.string()),
  }),
  z.object({
    kind: z.literal('otelcol-passthrough'),
    endpoint: z.string().url(),
  }),
]);
export type BackendDraft = z.infer<typeof BackendDraft>;

/** Config-file format Trove patches. Mirrors the Rust `Format` enum. */
export const PatchFormat = z.enum(['json', 'jsonc', 'toml', 'yaml']);
export type PatchFormat = z.infer<typeof PatchFormat>;

/** Metadata persisted after every successful upsert; consumed by the
 *  Rust `safety::conflict::detect` to identify three-way merges. */
export const TrovePatch = z.object({
  /** sha256 hex of the canonical payload at last write. */
  managedBlockHash: z.string().min(1),
  /** sha256 hex of the entire host file at last write. */
  fileHashAtLastWrite: z.string().min(1),
  /** Format of the host config file. */
  format: PatchFormat,
  /** Canonical region payload string Trove wrote at last apply.
   *  Drives the 3-way merge UI's "original" pane. Defaults to `""`
   *  for records migrated from schema v2 — those harnesses degrade to
   *  the resolver's 2-pane orphan-block view until the next apply
   *  re-populates the field. */
  lastWrittenRegionPayload: z.string().default(''),
});
export type TrovePatch = z.infer<typeof TrovePatch>;

/** Outcome of a three-way conflict check. Mirrors the Rust
 *  `ConflictState` enum produced by `safety::conflict::detect`. */
export const ConflictState = z.enum([
  'clean',
  'user-edited-outside',
  'region-removed',
  'region-conflict',
]);
export type ConflictState = z.infer<typeof ConflictState>;

/** What Trove records about each harness it has touched. */
export const HarnessConfig = z.object({
  id: HarnessId,
  enabled: z.boolean(),
  configPath: z.string().min(1),
  lastPatchedAt: z.string().datetime(),
  trovePatch: TrovePatch,
  options: z.object({
    logUserPrompts: z.boolean().default(false),
    customAttributes: z.record(z.string(), z.string()).default({}),
  }),
});
export type HarnessConfig = z.infer<typeof HarnessConfig>;

/** Persisted application state. Secrets are referenced via SecretRef only.
 *  Schema version bumped to 3 in Sprint 8 alongside TrovePatch's new
 *  `lastWrittenRegionPayload` field that powers the 3-way merge UI.
 *  v2 -> v3 migration: the loader injects `lastWrittenRegionPayload: ""`
 *  for any harness without it, then re-stamps schemaVersion on next
 *  save. Older v2-only consumers cannot read v3 files. */
export const AppState = z.object({
  schemaVersion: z.literal(3),
  backend: Backend.nullable(),
  harnesses: z.array(HarnessConfig),
});
export type AppState = z.infer<typeof AppState>;

/** Which signal led the detector to flag a harness as installed.
 *  Mirrors the Rust `DetectionMethod` enum. Listed in preference order:
 *  config-dir wins over PATH wins over app-bundle when several fire. */
export const DetectionMethod = z.enum(['path-binary', 'config-dir', 'app-bundle']);
export type DetectionMethod = z.infer<typeof DetectionMethod>;

/** Whether the host config currently emits telemetry. Tri-state because
 *  malformed or missing files can't honestly answer; the dashboard maps
 *  `unknown` to a neutral icon. Mirrors the Rust `TelemetryStatus`. */
export const TelemetryStatus = z.enum(['on', 'off', 'unknown']);
export type TelemetryStatus = z.infer<typeof TelemetryStatus>;

/** One row in the dashboard's "Detected harnesses" list. Mirrors the
 *  Rust `DetectedHarness` struct (camelCase keys on the wire). */
export const DetectedHarness = z.object({
  id: HarnessId,
  detected: z.boolean(),
  configPath: z.string().nullable(),
  telemetry: TelemetryStatus,
  detectionMethod: DetectionMethod.nullable(),
  /** Whether the host config currently contains a Trove-managed
   *  region. Drives the toggle's "Enable" vs "Disable" label. */
  troveRegionPresent: z.boolean(),
  /** Whether Trove currently ships an adapter for this harness.
   *  Drives whether the row's toggle is enabled. Populated by the
   *  Rust side from `HarnessId::has_adapter()`; replaces the static
   *  list previously maintained in `HarnessList.tsx`. */
  adapterAvailable: z.boolean(),
});
export type DetectedHarness = z.infer<typeof DetectedHarness>;

/** Per-apply options chosen by the user in the UI. Mirrors the Rust
 *  `adapters::ApplyOptions` struct. `logUserPrompts` defaults to false;
 *  the wizard requires an explicit acknowledgement before flipping it
 *  on. `customAttributes` is forwarded to the harness as
 *  `OTEL_RESOURCE_ATTRIBUTES=key1=val1,…`. */
export const ApplyOptions = z.object({
  logUserPrompts: z.boolean().default(false),
  customAttributes: z.record(z.string(), z.string()).default({}),
});
export type ApplyOptions = z.infer<typeof ApplyOptions>;

/** What `preview_patch` tells the UI about the proposed write. Drives
 *  the diff modal's CTA text and whether `apply_patch` would actually
 *  write. Mirrors the Rust `adapters::PreviewStatus` enum. */
export const PreviewStatus = z.enum(['fresh', 'idempotent', 'conflict']);
export type PreviewStatus = z.infer<typeof PreviewStatus>;

/** What `preview_patch` returns. The diff modal renders before/after
 *  client-side via the `diff` npm package. Mirrors the Rust
 *  `adapters::PatchPreview` struct (camelCase keys on the wire). */
export const PatchPreview = z.object({
  configPath: z.string(),
  format: PatchFormat,
  before: z.string(),
  after: z.string(),
  status: PreviewStatus,
});
export type PatchPreview = z.infer<typeof PatchPreview>;

/** Outcome of the wizard's "Test export" button. The Rust IPC sends a
 *  synthetic OTLP payload to the local collector and watches its log
 *  for the otelcol "successfully sent" line within a 5s budget. The
 *  status drives the wizard's green/red banner; `detail` is shown
 *  inline as a one-liner. Mirrors the Rust `TestExportResult` struct. */
export const TestExportResult = z.object({
  status: z.enum(['ok', 'failed', 'timeout']),
  detail: z.string(),
});
export type TestExportResult = z.infer<typeof TestExportResult>;

/** Sprint 6 PR 1 — collector status surface. Mirrors the Rust
 *  `ipc::collector_status::CollectorRunState` discriminated union.
 *  Each variant carries the kebab-case `kind` discriminator. */
export const CollectorRunState = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('idle') }),
  z.object({ kind: z.literal('starting'), pid: z.number().int().nonnegative() }),
  z.object({
    kind: z.literal('running'),
    pid: z.number().int().nonnegative(),
    restarts: z.number().int().nonnegative(),
  }),
  z.object({
    kind: z.literal('crashed'),
    restarts: z.number().int().nonnegative(),
  }),
  z.object({ kind: z.literal('stopping') }),
  z.object({ kind: z.literal('stopped') }),
  z.object({ kind: z.literal('failed'), reason: z.string() }),
]);
export type CollectorRunState = z.infer<typeof CollectorRunState>;

/** Status payload returned by `get_collector_status`. `logPath` is the
 *  on-disk file the dashboard's logs panel pulls its initial tail from. */
export const CollectorStatus = z.object({
  state: CollectorRunState,
  logPath: z.string(),
});
export type CollectorStatus = z.infer<typeof CollectorStatus>;

/** Counts of one signal type (spans, metric points, log records) summed
 *  across every receiver/exporter/transport label combination. */
export const SignalCounts = z.object({
  spans: z.number().int().nonnegative(),
  metricPoints: z.number().int().nonnegative(),
  logRecords: z.number().int().nonnegative(),
});
export type SignalCounts = z.infer<typeof SignalCounts>;

/** Overall green/amber/red derivation. Both Rust (tray icon) and TS
 *  (dashboard badge) compute this from the same inputs; they must
 *  agree, and Sprint 6 PR 3 has Vitest tests asserting parity. */
export const OverallHealth = z.enum(['green', 'amber', 'red']);
export type OverallHealth = z.infer<typeof OverallHealth>;

/** Wire-format metrics snapshot. Internal Rust state uses
 *  `tokio::time::Instant` for monotonic staleness math; the IPC layer
 *  converts to ms-ago scalars at handler time. `lastSignalMsAgo: null`
 *  means no signal has been observed since the tap started. */
export const MetricsSnapshotWire = z.object({
  received: SignalCounts,
  sent: SignalCounts,
  lastSignalMsAgo: z.number().int().nonnegative().nullable(),
  scrapedMsAgo: z.number().int().nonnegative(),
  unreachable: z.boolean(),
  overallHealth: OverallHealth,
});
export type MetricsSnapshotWire = z.infer<typeof MetricsSnapshotWire>;

/** One log line returned by `get_collector_log_tail` or emitted on
 *  the live `collector-log` event. */
export const CollectorLogLineWire = z.object({
  stream: z.string(),
  line: z.string(),
});
export type CollectorLogLineWire = z.infer<typeof CollectorLogLineWire>;

/** Initial tail returned by `get_collector_log_tail`. `byteOffset` is
 *  the file position at the moment the read finished; the dashboard's
 *  live-event hook discards `collector-log` events whose implicit
 *  offset ≤ this baseline so the tail and stream don't double-display. */
export const CollectorLogTailResponse = z.object({
  lines: z.array(CollectorLogLineWire),
  byteOffset: z.number().int().nonnegative(),
});
export type CollectorLogTailResponse = z.infer<typeof CollectorLogTailResponse>;

/** Live event payload for `collector-log`. Identical shape to
 *  `CollectorLogLineWire`. */
export const CollectorLogEvent = CollectorLogLineWire;
export type CollectorLogEvent = z.infer<typeof CollectorLogEvent>;

/** Discriminated union mirroring the Rust `ipc::IpcError` enum.
 *  The TS side branches on `kind` to render specific UI affordances
 *  (e.g. surface the conflicting `path` for region-conflict). */
export const IpcError = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('config-unparseable'),
    path: z.string(),
    reason: z.string(),
  }),
  z.object({
    kind: z.literal('region-conflict'),
    path: z.string(),
  }),
  z.object({
    kind: z.literal('harness-not-detected'),
    id: HarnessId,
  }),
  z.object({
    kind: z.literal('harness-not-implemented'),
    id: HarnessId,
  }),
  z.object({
    kind: z.literal('io'),
    path: z.string(),
    reason: z.string(),
  }),
  z.object({
    kind: z.literal('internal'),
    reason: z.string(),
  }),
]);
export type IpcError = z.infer<typeof IpcError>;
