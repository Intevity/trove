import { z } from 'zod';

import {
  AppState,
  Backend,
  CollectorLogTailResponse,
  CollectorStatus,
  ConflictResolutionOutcome,
  DetectedHarness,
  MetricsSnapshotWire,
  PatchPreview,
  ResolvedIdentity,
  TestExportResult,
  TrovePatch,
  UpdateMetadata,
} from './schemas.js';

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
 *  carrying the hash pair persisted into `state.json` for Sprint 8's
 *  three-way conflict UI. The `HarnessConfig` upsert into `state.json`
 *  happens server-side; the response is unchanged from Sprint 4. */
export const ApplyPatchResponse = TrovePatch;
export type ApplyPatchResponse = z.infer<typeof ApplyPatchResponse>;

/** `revert_patch` — args: { harnessId }. Returns nothing on success. */
export const RevertPatchResponse = z.null();
export type RevertPatchResponse = z.infer<typeof RevertPatchResponse>;

/** Sprint 8 — `resolve_conflict`. Args: { harnessId, action }. The
 *  resolver UI calls this when the user picks one of `keep-mine`,
 *  `take-theirs`, or `merge-manually`. Returns the post-resolution
 *  outcome (a fresh `TrovePatch` for the apply paths, or sibling-file
 *  paths for the manual-merge branch). */
export const ResolveConflictResponse = ConflictResolutionOutcome;
export type ResolveConflictResponse = z.infer<typeof ResolveConflictResponse>;

/** `get_app_state` — no arguments. Returns the current `AppState`
 *  (a fresh launch with no `state.json` returns the default). */
export const GetAppStateResponse = AppState;
export type GetAppStateResponse = z.infer<typeof GetAppStateResponse>;

/** `save_backend` — args: { draft: BackendDraft }. Stores each secret
 *  in the OS keychain, persists the resulting Backend (with `SecretRef`
 *  handles) into `state.json`, and triggers a collector reload (PR 2).
 *  Returns the persisted Backend. */
export const SaveBackendResponse = Backend;
export type SaveBackendResponse = z.infer<typeof SaveBackendResponse>;

/** `clear_backend` — no arguments. Deletes the keychain entries for
 *  the active backend and nulls out `state.backend`. Returns nothing. */
export const ClearBackendResponse = z.null();
export type ClearBackendResponse = z.infer<typeof ClearBackendResponse>;

/** `test_export` — no arguments. Sends a synthetic OTLP payload through
 *  the local collector and watches the collector log for an otelcol
 *  "successfully sent" line within ~5s. Returns the resolved status. */
export const TestExportResponse = TestExportResult;
export type TestExportResponse = z.infer<typeof TestExportResponse>;

/** Sprint 6 PR 1 — `get_collector_status`. Snapshots the supervisor's
 *  current run state plus the on-disk collector log path. The PR 3
 *  dashboard subscribes to live transitions via the `collector-state`
 *  Tauri event (same shape as the inner `state` field). */
export const GetCollectorStatusResponse = CollectorStatus;
export type GetCollectorStatusResponse = z.infer<typeof GetCollectorStatusResponse>;

/** Sprint 6 PR 1 — `get_metrics_snapshot`. Latest scrape from the
 *  Collector's internal Prometheus endpoint, normalised into ms-ago
 *  deltas. `null` means no scrape has completed yet (UI shows a
 *  skeleton). Live updates flow on the `metrics-snapshot` event. */
export const GetMetricsSnapshotResponse = MetricsSnapshotWire.nullable();
export type GetMetricsSnapshotResponse = z.infer<typeof GetMetricsSnapshotResponse>;

/** Sprint 6 PR 1 — `get_collector_log_tail`. Returns the last N lines
 *  from `collector.log` and the byte offset reached, used by the
 *  dashboard logs panel to thread its live-event stream without
 *  double-display. */
export const GetCollectorLogTailResponse = CollectorLogTailResponse;
export type GetCollectorLogTailResponse = z.infer<typeof GetCollectorLogTailResponse>;

/** Sprint 10 — `set_auto_update_enabled`. Args: { enabled: boolean }.
 *  Toggles the persisted `autoUpdateEnabled` flag in `state.json`.
 *  Returns nothing on success. */
export const SetAutoUpdateEnabledResponse = z.null();
export type SetAutoUpdateEnabledResponse = z.infer<typeof SetAutoUpdateEnabledResponse>;

/** Sprint 10 — `check_for_updates`. No arguments. Calls the Tauri
 *  updater plugin against the configured GitHub Releases endpoint and
 *  returns whether an update is available, the candidate version, and
 *  the running build's version. Failures surface as
 *  `IpcError::UpdaterCheckFailed`. */
export const CheckForUpdatesResponse = UpdateMetadata;
export type CheckForUpdatesResponse = z.infer<typeof CheckForUpdatesResponse>;

/** Sprint 12 — `set_identity_enabled`. Toggles the persisted opt-in
 *  identity-tagging flag and (when a backend is configured) reloads
 *  the collector with the updated YAML. */
export const SetIdentityEnabledRequest = z.object({ enabled: z.boolean() });
export type SetIdentityEnabledRequest = z.infer<typeof SetIdentityEnabledRequest>;
export const SetIdentityEnabledResponse = z.null();
export type SetIdentityEnabledResponse = z.infer<typeof SetIdentityEnabledResponse>;

/** Sprint 12 — `set_identity_manual`. Persists a user-typed
 *  name/email override and pins `identity.source` to `manual`. */
export const SetIdentityManualRequest = z.object({
  name: z.string(),
  email: z.string(),
});
export type SetIdentityManualRequest = z.infer<typeof SetIdentityManualRequest>;
export const SetIdentityManualResponse = z.null();
export type SetIdentityManualResponse = z.infer<typeof SetIdentityManualResponse>;

/** Sprint 12 — `set_identity_auto`. Pins `identity.source` back to
 *  `auto` without touching the persisted name/email values. */
export const SetIdentityAutoResponse = z.null();
export type SetIdentityAutoResponse = z.infer<typeof SetIdentityAutoResponse>;

/** Sprint 12 — `resolve_identity_preview`. Returns the values the
 *  collector would tag with right now, plus the probe-ladder source
 *  the UI surfaces to the user. */
export const ResolveIdentityPreviewResponse = ResolvedIdentity;
export type ResolveIdentityPreviewResponse = z.infer<typeof ResolveIdentityPreviewResponse>;

/** Canonical Tauri command names, kept here so the IPC client wrapper
 *  and tests share a single source of truth. The Rust side registers
 *  these in `tauri::generate_handler!` in lib.rs; renames must update
 *  both sides at once. */
export const IpcCommandName = {
  ListDetectedHarnesses: 'list_detected_harnesses',
  PreviewPatch: 'preview_patch',
  ApplyPatch: 'apply_patch',
  RevertPatch: 'revert_patch',
  ResolveConflict: 'resolve_conflict',
  GetAppState: 'get_app_state',
  SaveBackend: 'save_backend',
  ClearBackend: 'clear_backend',
  TestExport: 'test_export',
  GetCollectorStatus: 'get_collector_status',
  GetMetricsSnapshot: 'get_metrics_snapshot',
  GetCollectorLogTail: 'get_collector_log_tail',
  SetAutoUpdateEnabled: 'set_auto_update_enabled',
  CheckForUpdates: 'check_for_updates',
  SetIdentityEnabled: 'set_identity_enabled',
  SetIdentityManual: 'set_identity_manual',
  SetIdentityAuto: 'set_identity_auto',
  ResolveIdentityPreview: 'resolve_identity_preview',
} as const;
export type IpcCommandName = (typeof IpcCommandName)[keyof typeof IpcCommandName];

/** Tauri event channel names. Emitted by the Rust event pumps in
 *  `ipc::collector_status::spawn_event_pumps`. */
export const TauriEventName = {
  CollectorState: 'collector-state',
  MetricsSnapshot: 'metrics-snapshot',
  CollectorLog: 'collector-log',
} as const;
export type TauriEventName = (typeof TauriEventName)[keyof typeof TauriEventName];
