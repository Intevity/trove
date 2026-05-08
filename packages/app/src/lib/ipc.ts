import { invoke } from '@tauri-apps/api/core';
import {
  ApplyPatchResponse,
  ClearBackendResponse,
  GetAppStateResponse,
  GetCollectorLogTailResponse,
  GetCollectorStatusResponse,
  GetMetricsSnapshotResponse,
  IpcCommandName,
  IpcError,
  ListDetectedHarnessesResponse,
  PreviewPatchResponse,
  RevertPatchResponse,
  SaveBackendResponse,
  TestExportResponse,
  type AppState,
  type ApplyOptions,
  type Backend,
  type BackendDraft,
  type CollectorLogTailResponse,
  type CollectorStatus,
  type DetectedHarness,
  type HarnessId,
  type MetricsSnapshotWire,
  type PatchPreview,
  type TestExportResult,
  type TrovePatch,
} from '@trove/shared';

/** Thrown by IPC wrappers when the Rust side rejected. The structured
 *  Rust `IpcError` enum is preserved as `cause` so callers can branch on
 *  `cause.kind` (e.g. show a toast for `region-conflict`). */
export class TroveIpcError extends Error {
  public override readonly cause: IpcError;
  constructor(cause: IpcError) {
    super(`Trove IPC error: ${cause.kind}`);
    this.name = 'TroveIpcError';
    this.cause = cause;
  }
}

/** Wrap a `tauri::invoke` call so the Rust-side `Result<T, IpcError>`
 *  becomes either `T` or a thrown `TroveIpcError`. The response is
 *  Zod-parsed against `responseSchema` so wire-format drift between
 *  Rust and TS surfaces as a runtime error rather than a silent type
 *  mismatch. */
async function invokeIpc<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  responseSchema: { parse: (raw: unknown) => T },
): Promise<T> {
  let raw: unknown;
  try {
    raw = await invoke(command, args);
  } catch (err) {
    // Tauri rejects with the serialized Rust error. If it parses as our
    // structured `IpcError`, rethrow as `TroveIpcError`; otherwise fall
    // back to a generic `internal` shape so callers always see the
    // discriminated union.
    const parsed = IpcError.safeParse(err);
    if (parsed.success) {
      throw new TroveIpcError(parsed.data);
    }
    throw new TroveIpcError({
      kind: 'internal',
      reason: err instanceof Error ? err.message : String(err),
    });
  }
  return responseSchema.parse(raw);
}

/** Detect every Tier 1 harness on the user's machine. Always succeeds —
 *  rows for missing harnesses come back with `detected: false`. */
export async function listDetectedHarnesses(): Promise<DetectedHarness[]> {
  return invokeIpc(IpcCommandName.ListDetectedHarnesses, undefined, ListDetectedHarnessesResponse);
}

/** Compute the diff Trove would write for a harness. The diff modal
 *  renders the result client-side via the `diff` npm package. */
export async function previewPatch(
  harnessId: HarnessId,
  options: ApplyOptions,
): Promise<PatchPreview> {
  return invokeIpc(IpcCommandName.PreviewPatch, { harnessId, options }, PreviewPatchResponse);
}

/** Apply Trove's patch to the harness's host config. Returns the
 *  `TrovePatch` the caller can persist (Sprint 5+) for later
 *  three-way conflict detection. */
export async function applyPatch(harnessId: HarnessId, options: ApplyOptions): Promise<TrovePatch> {
  return invokeIpc(IpcCommandName.ApplyPatch, { harnessId, options }, ApplyPatchResponse);
}

/** Remove Trove's patch from the harness's host config. */
export async function revertPatch(harnessId: HarnessId): Promise<void> {
  await invokeIpc(IpcCommandName.RevertPatch, { harnessId }, RevertPatchResponse);
}

/** Read the persisted `AppState`. A fresh launch with no `state.json`
 *  yet returns the schema-v2 default (null backend, empty harnesses). */
export async function getAppState(): Promise<AppState> {
  return invokeIpc(IpcCommandName.GetAppState, undefined, GetAppStateResponse);
}

/** Persist a new backend chosen by the wizard. Stores each secret
 *  inline in the OS keychain; the returned `Backend` carries
 *  `SecretRef` handles only. PR 2 of Sprint 5 wires the collector
 *  reload that follows a successful save. */
export async function saveBackend(draft: BackendDraft): Promise<Backend> {
  return invokeIpc(IpcCommandName.SaveBackend, { draft }, SaveBackendResponse);
}

/** Wipe the active backend: deletes every keychain entry it referenced
 *  and nulls out `state.backend`. Idempotent. */
export async function clearBackend(): Promise<void> {
  await invokeIpc(IpcCommandName.ClearBackend, undefined, ClearBackendResponse);
}

/** Send a synthetic OTLP payload through the local collector and wait
 *  (up to ~5s) for the otelcol "successfully sent" log line. Drives the
 *  wizard's "Test export" button — `ok` enables Save, `failed`/`timeout`
 *  surface the underlying detail and unlock a "Save anyway" affordance. */
export async function testExport(): Promise<TestExportResult> {
  return invokeIpc(IpcCommandName.TestExport, undefined, TestExportResponse);
}

/** Snapshot the collector supervisor's current run state plus the
 *  on-disk log path. Sprint 6 PR 3's dashboard subscribes to live
 *  transitions via the `collector-state` Tauri event. */
export async function getCollectorStatus(): Promise<CollectorStatus> {
  return invokeIpc(IpcCommandName.GetCollectorStatus, undefined, GetCollectorStatusResponse);
}

/** Latest internal-Prometheus scrape from the bundled collector. `null`
 *  before the first scrape returns; `unreachable: true` when `:8888`
 *  refused (e.g. user-customised YAML drops the telemetry block). */
export async function getMetricsSnapshot(): Promise<MetricsSnapshotWire | null> {
  return invokeIpc(IpcCommandName.GetMetricsSnapshot, undefined, GetMetricsSnapshotResponse);
}

/** Read the most recent N lines from `collector.log` plus the byte
 *  offset at the moment the read finished. The dashboard threads this
 *  initial tail with the live `collector-log` event stream by
 *  discarding events that arrive against the same offset. */
export async function getCollectorLogTail(lines: number): Promise<CollectorLogTailResponse> {
  return invokeIpc(IpcCommandName.GetCollectorLogTail, { lines }, GetCollectorLogTailResponse);
}
