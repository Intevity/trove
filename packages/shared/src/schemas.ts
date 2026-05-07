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
 *  Schema version bumped to 2 in Sprint 2 alongside the richer
 *  HarnessConfig.trovePatch (replacing trovePatchHash from v1).
 *  Migrations land alongside Sprint 5 when state.json starts being
 *  persisted in the wild. */
export const AppState = z.object({
  schemaVersion: z.literal(2),
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
});
export type DetectedHarness = z.infer<typeof DetectedHarness>;

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
