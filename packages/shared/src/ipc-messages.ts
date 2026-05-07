import { z } from 'zod';

import { DetectedHarness, PatchPreview, TrovePatch } from './schemas.js';

/** Tauri command name → response shape table. Each command's request
 *  args are passed positionally via `invoke(name, args)`; only response
 *  shapes need a Zod schema for runtime validation on the TS side. */

/** `list_detected_harnesses` — no arguments. Returns one row per Tier 1
 *  harness; rows for harnesses without an adapter yet still appear with
 *  `detected: false`-or-true depending on system state. */
export const ListDetectedHarnessesResponse = z.array(DetectedHarness);
export type ListDetectedHarnessesResponse = z.infer<typeof ListDetectedHarnessesResponse>;

/** `preview_patch` — args: { harnessId, options }. Returns a
 *  `PatchPreview` describing the would-be write. */
export const PreviewPatchResponse = PatchPreview;
export type PreviewPatchResponse = z.infer<typeof PreviewPatchResponse>;

/** `apply_patch` — args: { harnessId, options }. Returns a `TrovePatch`
 *  carrying the hash pair Sprint 5+ will persist into `state.json`. */
export const ApplyPatchResponse = TrovePatch;
export type ApplyPatchResponse = z.infer<typeof ApplyPatchResponse>;

/** `revert_patch` — args: { harnessId }. Returns nothing on success. */
export const RevertPatchResponse = z.null();
export type RevertPatchResponse = z.infer<typeof RevertPatchResponse>;

/** Canonical Tauri command names, kept here so the IPC client wrapper
 *  and tests share a single source of truth. The Rust side registers
 *  these in `tauri::generate_handler!` in lib.rs; renames must update
 *  both sides at once. */
export const IpcCommandName = {
  ListDetectedHarnesses: 'list_detected_harnesses',
  PreviewPatch: 'preview_patch',
  ApplyPatch: 'apply_patch',
  RevertPatch: 'revert_patch',
} as const;
export type IpcCommandName = (typeof IpcCommandName)[keyof typeof IpcCommandName];
