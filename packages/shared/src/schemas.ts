import { z } from 'zod';

// Placeholder schemas for Sprint 0. Sprint 2 fleshes these out with the full
// safety contract (atomic writes, sentinel-bracketed regions, conflict
// detection). Until then these exist so packages/app can import the workspace
// type and the workspace wiring is exercised end-to-end.

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

/** What Trove records about each harness it has touched. */
export const HarnessConfig = z.object({
  id: HarnessId,
  enabled: z.boolean(),
  configPath: z.string().min(1),
  lastPatchedAt: z.string().datetime(),
  trovePatchHash: z.string().min(1),
  options: z.object({
    logUserPrompts: z.boolean().default(false),
    customAttributes: z.record(z.string(), z.string()).default({}),
  }),
});
export type HarnessConfig = z.infer<typeof HarnessConfig>;

/** Persisted application state. Secrets are referenced via SecretRef only. */
export const AppState = z.object({
  schemaVersion: z.literal(1),
  backend: Backend.nullable(),
  harnesses: z.array(HarnessConfig),
});
export type AppState = z.infer<typeof AppState>;
