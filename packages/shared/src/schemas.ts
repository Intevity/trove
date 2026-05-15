import { z } from 'zod';

/** Discriminated union of supported AI coding harness identifiers (MVP set). */
export const HarnessId = z.enum([
  'claude-code',
  'claude-desktop',
  'gemini-cli',
  'codex-cli',
  'qwen-code',
  'opencode',
  'cursor-ide',
  'cursor-cli',
  'cline',
  'aider',
  'copilot-cli',
  'junie-cli',
  'droid',
  'kimi-code-cli',
  'devin',
  'forgecode',
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
    endpoint: z.string().min(1),
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
  z.object({
    kind: z.literal('new-relic'),
    /** 'us' or 'eu' — selects the OTLP intake host. */
    region: z.string().min(1),
    licenseKey: SecretRef,
  }),
  z.object({
    kind: z.literal('splunk-observability'),
    realm: z.string().min(1),
    accessToken: SecretRef,
  }),
  z.object({
    kind: z.literal('dynatrace'),
    /** Base environment URL, e.g. https://abc12345.live.dynatrace.com */
    environmentUrl: z.string().url(),
    apiToken: SecretRef,
  }),
  z.object({
    kind: z.literal('elastic'),
    endpoint: z.string().url(),
    apiKey: SecretRef,
  }),
  z.object({
    kind: z.literal('opensearch'),
    endpoint: z.string().url(),
    /** Literal `Authorization` header value (e.g. `Basic <base64(user:pass)>`). */
    authorization: SecretRef,
  }),
  z.object({
    kind: z.literal('openobserve'),
    endpoint: z.string().url(),
    organization: z.string().min(1),
    authorization: SecretRef,
  }),
  z.object({
    kind: z.literal('clickstack'),
    endpoint: z.string().url(),
    ingestionKey: SecretRef,
  }),
  z.object({
    kind: z.literal('chronosphere'),
    tenant: z.string().min(1),
    apiToken: SecretRef,
  }),
  z.object({
    kind: z.literal('sentry'),
    endpoint: z.string().url(),
    authHeader: SecretRef,
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
    endpoint: z.string().min(1),
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
  z.object({
    kind: z.literal('new-relic'),
    region: z.string().min(1),
    licenseKey: z.string().min(1),
  }),
  z.object({
    kind: z.literal('splunk-observability'),
    realm: z.string().min(1),
    accessToken: z.string().min(1),
  }),
  z.object({
    kind: z.literal('dynatrace'),
    environmentUrl: z.string().url(),
    apiToken: z.string().min(1),
  }),
  z.object({
    kind: z.literal('elastic'),
    endpoint: z.string().url(),
    apiKey: z.string().min(1),
  }),
  z.object({
    kind: z.literal('opensearch'),
    endpoint: z.string().url(),
    authorization: z.string().min(1),
  }),
  z.object({
    kind: z.literal('openobserve'),
    endpoint: z.string().url(),
    organization: z.string().min(1),
    authorization: z.string().min(1),
  }),
  z.object({
    kind: z.literal('clickstack'),
    endpoint: z.string().url(),
    ingestionKey: z.string().min(1),
  }),
  z.object({
    kind: z.literal('chronosphere'),
    tenant: z.string().min(1),
    apiToken: z.string().min(1),
  }),
  z.object({
    kind: z.literal('sentry'),
    endpoint: z.string().url(),
    authHeader: z.string().min(1),
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
  /** sha256 hex of the entire host file at last write. Adapterless
   *  harnesses (Cline, Claude Desktop) don't patch a host file and
   *  persist this as the empty string — the conflict UI keys off
   *  `lastWrittenRegionPayload` instead. */
  fileHashAtLastWrite: z.string(),
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
  // Rust writes RFC 3339 with explicit offset (chrono::DateTime::to_rfc3339()
  // emits `+00:00`, never `Z`). Zod 3.20+'s default datetime() rejects
  // non-Z offsets, so without `{ offset: true }` getAppState silently
  // fails validation and the wizard never dismisses.
  lastPatchedAt: z.string().datetime({ offset: true }),
  trovePatch: TrovePatch,
  options: z.object({
    customAttributes: z.record(z.string(), z.string()).default({}),
  }),
});
export type HarnessConfig = z.infer<typeof HarnessConfig>;

/** Identity attribution for outgoing telemetry. Opt-in; off by
 *  default. When enabled, the collector pipeline tags every signal
 *  with `user.name` and `user.email` resource attributes. Mirrors the
 *  Rust `Identity` struct. */
export const IdentitySource = z.enum(['auto', 'manual']);
export type IdentitySource = z.infer<typeof IdentitySource>;

export const Identity = z.object({
  enabled: z.boolean().default(false),
  source: IdentitySource.default('auto'),
  name: z.string().default(''),
  email: z.string().default(''),
});
export type Identity = z.infer<typeof Identity>;

/** Source label returned by `resolve_identity_preview`. Mirrors the
 *  Rust `ResolvedSource` enum. The `harness` case carries the
 *  detected harness id; the other variants carry no payload. */
export const ResolvedIdentitySource = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('manual') }),
  z.object({ kind: z.literal('harness'), id: HarnessId }),
  z.object({ kind: z.literal('git-config') }),
  z.object({ kind: z.literal('none') }),
]);
export type ResolvedIdentitySource = z.infer<typeof ResolvedIdentitySource>;

/** Outcome of `resolve_identity_preview`. Mirrors the Rust `Resolved`
 *  struct. The `source` field reflects which step of the probe ladder
 *  contributed the values. */
export const ResolvedIdentity = z.object({
  name: z.string(),
  email: z.string(),
  source: ResolvedIdentitySource,
});
export type ResolvedIdentity = z.infer<typeof ResolvedIdentity>;

// ---------------------------------------------------------------------------
// Sprint 13 — Tier A mapping configuration
// ---------------------------------------------------------------------------

/** Helper enum naming the five builtin metrics. Kept as a TS-side
 *  enum (the v1 wire form) for backward compatibility with consumers
 *  that still pattern-match on these literals, but the persisted
 *  `targetMetric`/`metric` fields now reference any `TroveMetricDefinition.id`
 *  string in the catalog, builtin or custom. Mirrors the Rust
 *  `TierAMetric` helper. */
export const TierAMetric = z.enum(['events', 'tokens', 'cost.usd', 'turn.duration', 'errors']);
export type TierAMetric = z.infer<typeof TierAMetric>;

/** OTLP signal shape for a {@link TroveMetricDefinition}. Drives the
 *  codegen + watcher emission path. Mirrors the Rust `TroveMetricKind`. */
export const TroveMetricKind = z.enum(['counter', 'gauge', 'histogram']);
export type TroveMetricKind = z.infer<typeof TroveMetricKind>;

/** One entry in the metric catalog. The five builtin Tier A metrics
 *  ship with `builtin: true` and are locked in the UI (rename / delete
 *  disabled). Users may add custom entries with their own name, kind,
 *  and required attribute list. Mirrors the Rust
 *  `TroveMetricDefinition`. */
export const TroveMetricDefinition = z.object({
  /** Stable reference used by {@link MappingSource} target fields and
   *  {@link HookEmit.metric}. For builtins this is the legacy short
   *  string (`"events"`, `"tokens"`, `"cost.usd"`, `"turn.duration"`,
   *  `"errors"`) so v1 docs migrate forward without rewriting any
   *  target references. */
  id: z.string().min(1),
  /** Full OTLP metric name as it appears on the wire. For builtins
   *  this is `trove.harness.<id>`. */
  name: z.string().min(1),
  kind: TroveMetricKind,
  description: z.string().default(''),
  requiredAttributes: z.array(z.string()).default([]),
  builtin: z.boolean().default(false),
});
export type TroveMetricDefinition = z.infer<typeof TroveMetricDefinition>;

/** Per-model cost-rate override row. Both rates are USD per 1,000
 *  tokens. Mirrors the Rust `CostOverride` struct. */
export const CostOverride = z.object({
  inputUsdPer1kTokens: z.number(),
  outputUsdPer1kTokens: z.number(),
});
export type CostOverride = z.infer<typeof CostOverride>;

/** Payload a `hook-rule` row tells its driver to emit when the rule's
 *  `when` event fires. `null` (in `MappingSource.emit`) means "do not
 *  emit anything" — used by before-event rules to avoid double-counting
 *  against after-event rules. Mirrors the Rust `HookEmit` struct. The
 *  `metric` is a {@link TroveMetricDefinition.id} string ref; validation
 *  on the Rust side rejects refs that don't resolve to a catalog entry. */
export const HookEmit = z.object({
  metric: z.string().min(1),
  attributes: z.record(z.string(), z.string()),
});
export type HookEmit = z.infer<typeof HookEmit>;

/** Where a Tier A signal comes from. Mirrors the Rust `MappingSource`
 *  discriminated union (tag `kind`, kebab-case values). */
export const MappingSource = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('hook-rule'),
    when: z.string(),
    emit: HookEmit.nullable(),
  }),
  z.object({
    kind: z.literal('synthesize-from-native'),
    nativeMetric: z.string(),
    /** Catalog id of the metric this row writes into. For builtins
     *  this is the short string from {@link TierAMetric}; for custom
     *  metrics, whatever slug the user picked. */
    targetMetric: z.string().min(1),
    attributeMap: z.record(z.string(), z.string()),
    /** Static labels injected on the synthesized metric via
     *  metricstransform's `add_label`. Use for required attributes
     *  the native source doesn't carry (e.g. event.kind, error.kind).
     *  Defaults to empty map. */
    injectAttributes: z.record(z.string(), z.string()).default({}),
  }),
]);
export type MappingSource = z.infer<typeof MappingSource>;

/** One harness's mapping rows plus its master enable flag and any
 *  per-model cost-rate overrides. Mirrors the Rust `HarnessMapping`. */
export const HarnessMapping = z.object({
  harnessId: HarnessId,
  enabled: z.boolean(),
  sources: z.array(MappingSource),
  costOverrides: z.record(z.string(), CostOverride).default({}),
});
export type HarnessMapping = z.infer<typeof HarnessMapping>;

/** The whole mapping config, persisted under `AppState.mappings`. The
 *  inner `schemaVersion` is independent of the outer `AppState`
 *  schemaVersion — the inner one only moves when the mapping schema
 *  itself changes. Mirrors the Rust `MappingState`.
 *
 *  v2 introduces the user-customizable `metrics` catalog (sibling of
 *  `harnesses`). On-disk v1 documents are migrated forward by the Rust
 *  loader — the TS side only ever sees v2 over IPC. */
export const MappingState = z.object({
  schemaVersion: z.literal(2),
  metrics: z.array(TroveMetricDefinition).default([]),
  harnesses: z.array(HarnessMapping),
});
export type MappingState = z.infer<typeof MappingState>;

/** One configured forwarding destination. Wraps a {@link Backend} with
 *  a stable identifier and an optional display label so the UI can
 *  edit/remove a specific instance without depending on `kind` (two
 *  instances of the same kind are valid). The Rust mirror is
 *  `app_state::BackendInstance`; both sides use UUID v4 string ids. */
export const BackendInstance = z.object({
  id: z.string().min(1),
  label: z.string().optional(),
  backend: Backend,
});
export type BackendInstance = z.infer<typeof BackendInstance>;

/** Persisted application state. Secrets are referenced via SecretRef only.
 *  Schema version bumped to 7 alongside the multi-platform refactor:
 *  the single nullable `backend` slot is replaced with a list of
 *  `backends`, broadcasting every signal to every configured platform.
 *  Migrations are centralised in the Rust loader: v6 documents with a
 *  single backend are auto-wrapped into a one-element list; the
 *  loader re-stamps schemaVersion to 7 so the next save persists v7 to
 *  disk. Older consumers cannot read v7 files. */
export const AppState = z.object({
  schemaVersion: z.literal(7),
  backends: z.array(BackendInstance),
  harnesses: z.array(HarnessConfig),
  autoUpdateEnabled: z.boolean(),
  identity: Identity,
  mappings: MappingState,
  /** Sticky "first telemetry observed at" timestamp (Unix seconds)
   *  per harness id. Once any harness has emitted telemetry, its entry
   *  lands here and persists across restarts; the Harnesses dashboard
   *  reads this to keep the pill green even when the live counter
   *  resets. Defaults to `{}` for forward-compat with v7 docs written
   *  before this field existed. */
  telemetryObserved: z.record(HarnessId, z.number().int()).default({}),
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
 *  `adapters::ApplyOptions` struct. Trove is metrics-only by policy —
 *  no prompt or response bodies cross the wire — so there's no toggle
 *  to gate body capture. `customAttributes` is forwarded to the harness
 *  as `OTEL_RESOURCE_ATTRIBUTES=key1=val1,…`. */
export const ApplyOptions = z.object({
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
  /** Per-harness outgoing counts from the collector's diag filter
   *  pipelines. Keyed by harness suffix (e.g. `"gemini-cli"`); each
   *  entry holds span / metric-point / log-record counters this run.
   *  Only populated for native-OTel emitters with `service.name`
   *  candidates; watcher-emitter harnesses are absent. Defaults to `{}`
   *  for forward-compat with snapshots produced before this field
   *  existed. */
  diagObservations: z.record(z.string(), SignalCounts).default({}),
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

/** 3-way merge payload returned via {@link IpcError} when the apply
 *  path detects a hand-edited managed region and `state.json` carries
 *  the prior baseline. The resolver UI renders three panes:
 *
 *  - Original — what Trove last wrote (the canonical region payload).
 *    `null` for the orphan-block path (state.json wiped or never had a
 *    record), where the resolver collapses to a 2-pane (Yours / Trove's)
 *    layout instead of three.
 *  - Yours — the region payload as it currently appears in the host
 *    file (typically the user's hand-edit).
 *  - Trove's — what `apply_patch` would write next.
 *
 *  Mirrors the Rust `ConflictPayload` struct in `ipc::mod`. */
export const ConflictPayload = z.object({
  configPath: z.string(),
  format: PatchFormat,
  originalRegionPayload: z.string().nullable(),
  currentRegionPayload: z.string(),
  theirsRegionPayload: z.string(),
  fileBefore: z.string(),
  fileAfterIfTakingTheirs: z.string(),
});
export type ConflictPayload = z.infer<typeof ConflictPayload>;

/** What the resolver tells `resolve_conflict` to do. Tagged union with
 *  kebab-case discriminators that mirror the Rust `ConflictAction`
 *  enum's serde shape. */
export const ConflictAction = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('keep-mine') }),
  z.object({ kind: z.literal('take-theirs'), options: ApplyOptions }),
  z.object({ kind: z.literal('merge-manually'), options: ApplyOptions }),
]);
export type ConflictAction = z.infer<typeof ConflictAction>;

/** Absolute paths of the sibling files dropped by `MergeManually`.
 *  Mirrors the Rust `SiblingPaths` struct. */
export const SiblingPaths = z.object({
  original: z.string(),
  theirs: z.string(),
  host: z.string(),
});
export type SiblingPaths = z.infer<typeof SiblingPaths>;

/** Successful outcome of `resolve_conflict`. Discriminated by `status`
 *  on the wire. Mirrors the Rust `ConflictResolutionOutcome` enum. */
export const ConflictResolutionOutcome = z.discriminatedUnion('status', [
  z.object({ status: z.literal('applied'), patch: TrovePatch }),
  z.object({ status: z.literal('marked-mine'), patch: TrovePatch }),
  z.object({ status: z.literal('merge-deferred'), siblingPaths: SiblingPaths }),
]);
export type ConflictResolutionOutcome = z.infer<typeof ConflictResolutionOutcome>;

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
    kind: z.literal('region-conflict-detected'),
    conflict: ConflictPayload,
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
    kind: z.literal('updater-check-failed'),
    reason: z.string(),
  }),
  z.object({
    kind: z.literal('internal'),
    reason: z.string(),
  }),
]);
export type IpcError = z.infer<typeof IpcError>;

/** Sprint 10 — outcome of `check_for_updates`. The React Settings
 *  component branches on `available` to render either the "update to vX
 *  available" CTA or the "you're on vY, no update" notice. Mirrors the
 *  Rust `UpdateMetadata` struct (camelCase keys on the wire). */
export const UpdateMetadata = z.object({
  available: z.boolean(),
  version: z.string().nullable(),
  current: z.string(),
});
export type UpdateMetadata = z.infer<typeof UpdateMetadata>;
