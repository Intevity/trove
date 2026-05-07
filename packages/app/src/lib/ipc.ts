import { invoke } from '@tauri-apps/api/core';
import {
  ApplyPatchResponse,
  IpcCommandName,
  IpcError,
  ListDetectedHarnessesResponse,
  PreviewPatchResponse,
  RevertPatchResponse,
  type ApplyOptions,
  type DetectedHarness,
  type HarnessId,
  type PatchPreview,
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
