import { z } from 'zod';

import { DetectedHarness } from './schemas.js';

/** Tauri command name → response shape table. Each command's request
 *  args are passed positionally via `invoke(name, args)`; only response
 *  shapes need a Zod schema for runtime validation on the TS side. */

/** `list_detected_harnesses` — no arguments. Returns one row per Tier 1
 *  harness; rows for harnesses without an adapter yet still appear with
 *  `detected: false`-or-true depending on system state. */
export const ListDetectedHarnessesResponse = z.array(DetectedHarness);
export type ListDetectedHarnessesResponse = z.infer<typeof ListDetectedHarnessesResponse>;

/** Canonical Tauri command names, kept here so the IPC client wrapper
 *  and tests share a single source of truth. The Rust side registers
 *  these in `tauri::generate_handler!` in lib.rs; renames must update
 *  both sides at once. */
export const IpcCommandName = {
  ListDetectedHarnesses: 'list_detected_harnesses',
} as const;
export type IpcCommandName = (typeof IpcCommandName)[keyof typeof IpcCommandName];
