//! Tauri `#[command]` functions exposed to the React UI.
//!
//! Sprint 3 PR 1 shipped detection; PR 2 added the patch trio
//! (preview / apply / revert). Sprint 5 PR 1 layers on app-state
//! persistence: `apply_patch` / `revert_patch` now upsert and remove
//! `HarnessConfig` entries in `state.json`, and three new commands
//! (`get_app_state`, `save_backend`, `clear_backend`) drive the
//! backend wizard.
//!
//! The collector reload that follows `save_backend` lands in PR 2 of
//! this sprint — for now we leave a tracing breadcrumb where the
//! reload will hook in.

use std::path::{Path, PathBuf};

use std::collections::HashMap;

use crate::adapters::{
    ApplyOptions, PatchPreview, PreviewStatus, TrovePatch, aider, claude_code, claude_desktop,
    claude_desktop_watcher, cline, cline_watcher, codex_cli, codex_desktop, copilot_cli, cursor_cli,
    cursor_ide, droid, gemini_cli, gemini_watcher, opencode,
    qwen_code,
};
use crate::app_state::{
    self, AppState, BackendDraft, BackendInstance, HarnessConfig, backend_secret_accounts,
    drain_secrets_from_draft, harness_config_from_apply,
};
use crate::collector::codegen;
use crate::detect::{DetectedHarness, TelemetryStatus, detect_all};
use crate::harness::HarnessId;
use crate::tier3_watchers::TierThreeWatchers;
use crate::safety::atomic::write_atomic;
use crate::safety::backup::{backup_file, prune_backups};
use crate::safety::sentinels::extract_region;
use crate::secrets;

use super::test_export::{DEFAULT_TEST_BUDGET, TestExportResult, test_export_at};
use super::{
    ConflictAction, ConflictPayload, ConflictResolutionOutcome, IpcError, SiblingPaths,
};

/// Detect every supported harness on the user's machine. Always
/// succeeds — missing harnesses come back with `detected: false`
/// rather than as errors. Sprint 9 PR 2 layers an overlay over Tier 3
/// rows that reads `state.json` so the dashboard can decide whether
/// the per-row toggle should read "Enable" or "Disable" — the
/// detector itself doesn't see Trove's persisted state.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn list_detected_harnesses(app: tauri::AppHandle) -> Result<Vec<DetectedHarness>, IpcError> {
    use tauri::Manager as _;
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| IpcError::Internal {
            reason: format!("could not resolve app_config_dir: {e}"),
        })?;
    let metrics = app.state::<crate::collector::MetricsTapHandle>();
    let mut observed: HashMap<HarnessId, u64> = metrics
        .latest()
        .map(|snap| {
            snap.diag_observations
                .iter()
                .filter_map(|(k, v)| harness_id_from_suffix(k).map(|id| (id, v.total())))
                .collect()
        })
        .unwrap_or_default();
    // Claude Desktop's audit.jsonl watcher emits OTLP metrics directly
    // (the diag-log pipeline only counts logs). Merge its in-process
    // emission counter so the Harnesses card pill flips to "On" once
    // any Cowork turn has been observed.
    let cowork_observations = claude_desktop_watcher::observation_count();
    if cowork_observations > 0 {
        observed
            .entry(HarnessId::ClaudeDesktop)
            .and_modify(|n| *n = (*n).saturating_add(cowork_observations))
            .or_insert(cowork_observations);
    }
    // Persist every freshly-observed harness so the pill stays green
    // across restarts and idle periods. The save is skipped entirely
    // when nothing changed, so this is a state.json read for the
    // common-case poll.
    let fresh_ids: Vec<HarnessId> = observed
        .iter()
        .filter_map(|(id, count)| if *count > 0 { Some(*id) } else { None })
        .collect();
    if !fresh_ids.is_empty() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        if let Err(e) = app_state::record_telemetry_observed_in(&config_dir, fresh_ids, now) {
            tracing::warn!(error = %e, "failed to persist telemetry observations");
        }
    }
    list_detected_harnesses_inner(&config_dir, &observed)
}

/// Resolve a `harness.id` suffix (the same string used in the codegen's
/// `filter/diag-<suffix>` processor name) back to its [`HarnessId`].
fn harness_id_from_suffix(suffix: &str) -> Option<HarnessId> {
    HarnessId::all()
        .iter()
        .copied()
        .find(|id| crate::collector::harness_id_suffix(*id) == suffix)
}

/// Test-friendly variant of [`list_detected_harnesses`]. Operates on
/// an explicit config directory so unit tests can lay down a synthetic
/// `state.json` and assert the Tier 3 overlay.
///
/// `observed_log_records` is a sparse map keyed by [`HarnessId`] of the
/// per-harness diagnostic log-record counts pulled from the metrics tap.
/// Any harness with a non-zero count has demonstrably emitted telemetry
/// this collector run, so its `telemetry` field is forced to
/// [`TelemetryStatus::On`].
pub fn list_detected_harnesses_inner<S: ::std::hash::BuildHasher>(
    config_dir: &Path,
    observed_log_records: &HashMap<HarnessId, u64, S>,
) -> Result<Vec<DetectedHarness>, IpcError> {
    let mut rows = detect_all();
    let state = app_state::load_from_dir(config_dir)?;
    overlay_watcher_only_state(&mut rows, &state);
    overlay_observed_telemetry(&mut rows, observed_log_records);
    overlay_persisted_observations(&mut rows, &state);
    Ok(rows)
}

/// For each watcher-only row (Tier 3 + Claude Desktop), override
/// `trove_region_present` (and `telemetry`) from the corresponding
/// `state.json` entry. Tier 1 / Tier 2 rows that write a managed region
/// into a host file already have authoritative answers from the
/// host-file inspection in `detect/harnesses.rs` and are left
/// unchanged. See [`HarnessId::enables_via_watcher_only`] for the
/// rationale.
fn overlay_watcher_only_state(rows: &mut [DetectedHarness], state: &AppState) {
    for row in rows.iter_mut() {
        if !row.id.enables_via_watcher_only() {
            continue;
        }
        let enabled = state
            .harnesses
            .iter()
            .any(|h| h.id == row.id && h.enabled);
        if enabled {
            row.trove_region_present = true;
            row.telemetry = TelemetryStatus::On;
        }
    }
}

/// For each row, flip `telemetry` to `On` if the collector's diagnostic
/// filter pipeline has observed any records carrying the harness's
/// known `service.name`. This is how adapterless Tier-1 emitters (e.g.
/// Claude Desktop, whose OTLP is admin-configured server-side and
/// invisible to local detection) escape the default "Unknown" pill.
fn overlay_observed_telemetry<S: ::std::hash::BuildHasher>(
    rows: &mut [DetectedHarness],
    observed: &HashMap<HarnessId, u64, S>,
) {
    for row in rows.iter_mut() {
        if observed.get(&row.id).copied().unwrap_or(0) > 0 {
            row.telemetry = TelemetryStatus::On;
        }
    }
}

/// Sticky telemetry: any harness with a `telemetry_observed` entry in
/// `state.json` has emitted telemetry at some point in the past and we
/// keep its pill green from then on. Survives app restarts and stays
/// green during long idle gaps when the live counter would otherwise
/// reset to zero. The persisted entry is the timestamp of the first
/// observation; presence is the signal, the value is informational.
///
/// Exception: if the user has explicitly disabled the harness
/// (`state.harnesses[id].enabled == false`), the sticky overlay is
/// suppressed. The Disable click is an authoritative intent — we
/// stopped the tap, so the pill should reflect "no longer flowing"
/// even though we once saw it.
fn overlay_persisted_observations(rows: &mut [DetectedHarness], state: &AppState) {
    for row in rows.iter_mut() {
        if !state.telemetry_observed.contains_key(&row.id) {
            continue;
        }
        let explicitly_disabled = state
            .harnesses
            .iter()
            .any(|h| h.id == row.id && !h.enabled);
        if explicitly_disabled {
            continue;
        }
        row.telemetry = TelemetryStatus::On;
    }
}

/// Compute the diff Trove would write for `harness_id` with `options`.
/// The UI renders the unified diff client-side from `before`/`after`.
//
// Tauri requires owned argument types for JSON deserialization, even
// when the function body only reads them — silence the lint locally.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn preview_patch(
    app: tauri::AppHandle,
    harness_id: HarnessId,
    options: ApplyOptions,
) -> Result<PatchPreview, IpcError> {
    let home = home_dir()?;
    preview_patch_inner(harness_id, &options, &home, |id| {
        external_resource_path(&app, id)
    })
}

/// Free-function inner for [`preview_patch`] so unit tests can exercise
/// the dispatch without synthesising a Tauri `AppHandle`.
/// `resolve_resource` is invoked only on the arms that need a bundled
/// resource (Cursor hook, wrapper scripts); Tier 1 arms and the
/// log-watch-only Cline arm never call it. Tests can pass a closure
/// that panics on the unused arms.
pub fn preview_patch_inner<F>(
    harness_id: HarnessId,
    options: &ApplyOptions,
    home: &Path,
    resolve_resource: F,
) -> Result<PatchPreview, IpcError>
where
    F: FnOnce(HarnessId) -> Result<PathBuf, IpcError>,
{
    match harness_id {
        HarnessId::ClaudeCode => claude_code::preview(home, options),
        HarnessId::CodexCli => codex_cli::preview(home, options),
        HarnessId::CodexDesktop => codex_desktop::preview(home, options),
        HarnessId::GeminiCli => gemini_cli::preview(home, options),
        HarnessId::QwenCode => qwen_code::preview(home, options),
        HarnessId::CursorIde => {
            cursor_ide::preview(home, options, &resolve_resource(HarnessId::CursorIde)?)
        }
        HarnessId::CursorCli => {
            cursor_cli::preview(home, options, &resolve_resource(HarnessId::CursorCli)?)
        }
        HarnessId::Opencode => opencode::preview(home, options),
        HarnessId::Cline => cline::preview(home, options),
        HarnessId::Aider => {
            aider::preview(home, options, &resolve_resource(HarnessId::Aider)?)
        }
        HarnessId::CopilotCli => {
            copilot_cli::preview(home, options, &resolve_resource(HarnessId::CopilotCli)?)
        }
        // Claude Desktop is adapter-backed by an audit-log tap rather
        // than a host-file patch. Same shape as Cline: synthetic preview,
        // synthetic apply, no host file touched.
        HarnessId::ClaudeDesktop => claude_desktop::preview(home, options),
        HarnessId::Droid => droid::preview(home, options),
        // Detection-only harnesses: no adapter wired today. The UI keeps
        // the toggle disabled (via adapter_available = has_adapter()),
        // so this branch should never be hit in practice. Surface the
        // error explicitly so any accidental IPC call is informative
        // rather than a panic.
        HarnessId::JunieCli
        | HarnessId::KimiCodeCli
        | HarnessId::Devin
        | HarnessId::Forgecode => Err(IpcError::HarnessNotImplemented { id: harness_id }),
    }
}

/// Apply Trove's patch to `harness_id`'s host config. On success, upsert
/// a [`HarnessConfig`] entry into `state.json` so the three-way conflict
/// UI has the metadata it needs (managed-block hash, post-write file
/// hash, payload snapshot, options snapshot, last-patched timestamp).
///
/// Sprint 8 routes through a preview-first flow: a conflict no longer
/// short-circuits with [`IpcError::RegionConflict`] but with
/// [`IpcError::RegionConflictDetected { conflict }`], where `conflict`
/// carries everything the React resolver needs to render its 3-way (or
/// 2-way orphan-block) merge UI. The 2-way fallback fires when no prior
/// `HarnessConfig` is on record (`state.json` was never written or got
/// wiped). `RegionConflict` (the Sprint 3 variant) stays in the error
/// enum but is no longer returned by this command.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
// `async` so Tauri executes this on the tokio runtime: `spawn_tier3_watcher`
// below calls `tokio::spawn` for tier-3 adapters (Claude Desktop, Cline,
// Aider, Copilot CLI, Gemini CLI), which aborts the process if invoked
// from a sync command's worker thread (no runtime context). Same fix
// pattern as `respawn_persisted_watchers` in `lib.rs`.
pub async fn apply_patch(
    app: tauri::AppHandle,
    harness_id: HarnessId,
    options: ApplyOptions,
) -> Result<TrovePatch, IpcError> {
    let home = home_dir()?;
    let preview = preview_patch_inner(harness_id, &options, &home, |id| {
        external_resource_path(&app, id)
    })?;

    if matches!(preview.status, PreviewStatus::Conflict) {
        let prior = load_prior_harness_config(&app, harness_id)?;
        let conflict = build_conflict_payload(&preview, prior.as_ref())?;
        return Err(IpcError::RegionConflictDetected {
            conflict: Box::new(conflict),
        });
    }

    let patch = match harness_id {
        HarnessId::ClaudeCode => claude_code::apply(&home, &options),
        HarnessId::CodexCli => codex_cli::apply(&home, &options),
        HarnessId::CodexDesktop => codex_desktop::apply(&home, &options),
        HarnessId::GeminiCli => gemini_cli::apply(&home, &options),
        HarnessId::QwenCode => qwen_code::apply(&home, &options),
        HarnessId::CursorIde => {
            let hook = external_resource_path(&app, HarnessId::CursorIde)?;
            cursor_ide::apply(&home, &options, &hook)
        }
        HarnessId::CursorCli => {
            let hook = external_resource_path(&app, HarnessId::CursorCli)?;
            cursor_cli::apply(&home, &options, &hook)
        }
        HarnessId::Opencode => opencode::apply(&home, &options),
        HarnessId::Cline => cline::apply(&home, &options),
        HarnessId::Aider => {
            let wrapper = external_resource_path(&app, HarnessId::Aider)?;
            aider::apply(&home, &options, &wrapper)
        }
        HarnessId::CopilotCli => {
            let wrapper = external_resource_path(&app, HarnessId::CopilotCli)?;
            copilot_cli::apply(&home, &options, &wrapper)
        }
        // Synthetic apply — no host file written. The IPC layer's
        // follow-on `spawn_tier3_watcher` + `upsert_harness` calls do
        // the real work (watcher up, state.json entry persisted).
        HarnessId::ClaudeDesktop => claude_desktop::apply(&home, &options),
        HarnessId::Droid => droid::apply(&home, &options),
        // Detection-only harnesses: see comment in preview_patch_inner.
        HarnessId::JunieCli
        | HarnessId::KimiCodeCli
        | HarnessId::Devin
        | HarnessId::Forgecode => Err(IpcError::HarnessNotImplemented { id: harness_id }),
    }?;

    // Sprint 9 PR 2/PR 3 — Tier 3 watchers. Each enabled tier-3
    // adapter spawns one tokio task and registers it in
    // `TierThreeWatchers`. `tier3_watchers::insert` replaces any prior
    // handle for the same id, so re-applies are idempotent.
    spawn_tier3_watcher(&app, harness_id, &home, &options);

    let harness_config = harness_config_from_apply(
        harness_id,
        &harness_config_path(harness_id, &home),
        options,
        patch.clone(),
    );

    app_state::upsert_harness(&app, harness_config)?;

    Ok(patch)
}

/// Sprint 8 — resolve a 3-way conflict. Called by the React resolver
/// after the user picks one of the three actions on the modal:
///
/// - `KeepMine`: re-baselines `state.json` against the user's current
///   region (no host-file write). Future re-applies with the same
///   options will be `Idempotent` rather than re-conflicting.
/// - `TakeTheirs`: backs the host file up, atomically overwrites it
///   with what the preview says Trove would write, and stores a fresh
///   `TrovePatch` in `state.json`.
/// - `MergeManually`: writes `<host>.trove.original` (the prior payload,
///   empty for orphan-block paths) and `<host>.trove.theirs` (what
///   Trove wants to write) next to the host file. The renderer opens
///   the host config in the OS default editor via `tauri-plugin-shell`
///   using the returned `host` path. State.json is not touched until
///   the user re-applies.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn resolve_conflict(
    app: tauri::AppHandle,
    harness_id: HarnessId,
    action: ConflictAction,
) -> Result<ConflictResolutionOutcome, IpcError> {
    let home = home_dir()?;
    match action {
        ConflictAction::KeepMine => keep_mine(&app, harness_id, &home),
        ConflictAction::TakeTheirs { options } => take_theirs(&app, harness_id, &home, options),
        ConflictAction::MergeManually { options } => {
            merge_manually(&app, harness_id, &home, &options)
        }
    }
}

/// Look up the existing [`HarnessConfig`] for `harness_id`, if any. The
/// presence of this record discriminates the 3-way (Some) vs 2-way
/// orphan-block (None) conflict-resolver UI mode.
fn load_prior_harness_config(
    app: &tauri::AppHandle,
    harness_id: HarnessId,
) -> Result<Option<HarnessConfig>, IpcError> {
    let state = app_state::load(app)?;
    Ok(state.harnesses.into_iter().find(|h| h.id == harness_id))
}

/// Test-friendly variant of [`load_prior_harness_config`] that operates
/// on an explicit config directory instead of going through the running
/// Tauri instance. Used by the integration tests in
/// `tests/conflict_flow.rs`.
pub fn load_prior_harness_config_in(
    config_dir: &Path,
    harness_id: HarnessId,
) -> Result<Option<HarnessConfig>, IpcError> {
    let state = app_state::load_from_dir(config_dir)?;
    Ok(state.harnesses.into_iter().find(|h| h.id == harness_id))
}

/// Build a [`ConflictPayload`] from a `Conflict`-status preview plus any
/// prior `HarnessConfig`. The current and theirs region payloads come
/// out of `preview.before` / `preview.after` via [`extract_region`] —
/// both are guaranteed `Some` because the conflict status implies a
/// managed region exists in the file (current) and the preview's after
/// content is the result of upsert (theirs). If either extraction
/// returns `None`, the host file's managed-region semantics are
/// inconsistent in a way the safety contract should never produce; we
/// surface that as `Internal` rather than misrepresent the state to
/// the resolver UI.
pub fn build_conflict_payload(
    preview: &PatchPreview,
    prior: Option<&HarnessConfig>,
) -> Result<ConflictPayload, IpcError> {
    let current_region = extract_region(preview.format, &preview.before)
        .map_err(|e| IpcError::ConfigUnparseable {
            path: preview.config_path.display().to_string(),
            reason: e.to_string(),
        })?
        .ok_or_else(|| IpcError::Internal {
            reason: "expected managed region in preview.before but found none".into(),
        })?;
    let theirs_region = extract_region(preview.format, &preview.after)
        .map_err(|e| IpcError::Internal {
            reason: format!("could not extract region from preview.after: {e}"),
        })?
        .ok_or_else(|| IpcError::Internal {
            reason: "expected managed region in preview.after but found none".into(),
        })?;

    let original_region_payload = prior.map(|h| h.trove_patch.last_written_region_payload.clone());
    Ok(ConflictPayload {
        config_path: preview.config_path.display().to_string(),
        format: preview.format,
        original_region_payload,
        current_region_payload: current_region.payload,
        theirs_region_payload: theirs_region.payload,
        file_before: preview.before.clone(),
        file_after_if_taking_theirs: preview.after.clone(),
    })
}

/// `KeepMine` resolution (Tauri-bound wrapper). Resolves the platform
/// config directory from the running app, then delegates to
/// [`keep_mine_inner`]. See [`keep_mine_inner`] for the contract.
fn keep_mine(
    app: &tauri::AppHandle,
    harness_id: HarnessId,
    home: &Path,
) -> Result<ConflictResolutionOutcome, IpcError> {
    use tauri::Manager as _;
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| IpcError::Internal {
            reason: format!("could not resolve app_config_dir: {e}"),
        })?;
    keep_mine_inner(harness_id, home, &config_dir)
}

/// Test-friendly variant of [`keep_mine`]. Reads the host file, hashes
/// its current managed region as the new baseline, and persists a
/// fresh `TrovePatch` to `state.json` under `config_dir` so future
/// re-applies don't re-trigger the resolver.
pub fn keep_mine_inner(
    harness_id: HarnessId,
    home: &Path,
    config_dir: &Path,
) -> Result<ConflictResolutionOutcome, IpcError> {
    let prior = load_prior_harness_config_in(config_dir, harness_id)?
        .ok_or_else(|| IpcError::Internal {
            reason: "keep-mine requires a prior HarnessConfig in state.json".into(),
        })?;
    let format = prior.trove_patch.format;
    let host_path = harness_config_path(harness_id, home);
    let current = std::fs::read_to_string(&host_path).map_err(|e| IpcError::Io {
        path: host_path.display().to_string(),
        reason: e.to_string(),
    })?;
    let region = extract_region(format, &current)
        .map_err(|e| IpcError::ConfigUnparseable {
            path: host_path.display().to_string(),
            reason: e.to_string(),
        })?
        .ok_or_else(|| IpcError::Internal {
            reason: "keep-mine requires an existing managed region in the host file".into(),
        })?;

    let new_patch = TrovePatch {
        managed_block_hash: region.hash.clone(),
        file_hash_at_last_write: sha256_hex(current.as_bytes()),
        format,
        last_written_region_payload: region.payload,
    };
    let entry =
        harness_config_from_apply(harness_id, &host_path, prior.options.clone(), new_patch.clone());
    app_state::upsert_harness_in(config_dir, entry)?;
    Ok(ConflictResolutionOutcome::MarkedMine { patch: new_patch })
}

/// `TakeTheirs` resolution (Tauri-bound wrapper). Resolves the cursor
/// hook script path and the platform config directory from the running
/// app, then delegates to [`take_theirs_inner`].
fn take_theirs(
    app: &tauri::AppHandle,
    harness_id: HarnessId,
    home: &Path,
    options: ApplyOptions,
) -> Result<ConflictResolutionOutcome, IpcError> {
    use tauri::Manager as _;
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| IpcError::Internal {
            reason: format!("could not resolve app_config_dir: {e}"),
        })?;
    take_theirs_inner(harness_id, home, &config_dir, options, |id| {
        external_resource_path(app, id)
    })
}

/// Test-friendly variant of [`take_theirs`]. Backs the host file up,
/// overwrites it with what the preview says Trove would write, prunes
/// old backups, and stamps `state.json` with the new patch.
pub fn take_theirs_inner<F>(
    harness_id: HarnessId,
    home: &Path,
    config_dir: &Path,
    options: ApplyOptions,
    hook_resolver: F,
) -> Result<ConflictResolutionOutcome, IpcError>
where
    F: FnOnce(HarnessId) -> Result<PathBuf, IpcError>,
{
    let preview = preview_patch_inner(harness_id, &options, home, hook_resolver)?;
    let host_path = preview.config_path.clone();
    backup_file(&host_path).map_err(|e| IpcError::Io {
        path: host_path.display().to_string(),
        reason: format!("backup failed: {e}"),
    })?;
    write_atomic(&host_path, preview.after.as_bytes()).map_err(|e| IpcError::Io {
        path: host_path.display().to_string(),
        reason: e.to_string(),
    })?;
    let _ = prune_backups(&host_path, crate::adapters::BACKUPS_TO_KEEP);

    let theirs_region = extract_region(preview.format, &preview.after)
        .map_err(|e| IpcError::Internal {
            reason: format!("post-write region extraction failed: {e}"),
        })?
        .ok_or_else(|| IpcError::Internal {
            reason: "post-write file is missing the managed region".into(),
        })?;
    let new_patch = TrovePatch {
        managed_block_hash: theirs_region.hash.clone(),
        file_hash_at_last_write: sha256_hex(preview.after.as_bytes()),
        format: preview.format,
        last_written_region_payload: theirs_region.payload,
    };
    let entry = harness_config_from_apply(harness_id, &host_path, options, new_patch.clone());
    app_state::upsert_harness_in(config_dir, entry)?;
    Ok(ConflictResolutionOutcome::Applied { patch: new_patch })
}

/// `MergeManually` resolution (Tauri-bound wrapper).
fn merge_manually(
    app: &tauri::AppHandle,
    harness_id: HarnessId,
    home: &Path,
    options: &ApplyOptions,
) -> Result<ConflictResolutionOutcome, IpcError> {
    use tauri::Manager as _;
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| IpcError::Internal {
            reason: format!("could not resolve app_config_dir: {e}"),
        })?;
    merge_manually_inner(harness_id, home, &config_dir, options, |id| {
        external_resource_path(app, id)
    })
}

/// Test-friendly variant of [`merge_manually`]. Writes sibling files
/// next to the host config (the prior payload as `.trove.original`;
/// what Trove would write as `.trove.theirs`) and returns the paths so
/// the renderer can open the host file in the OS default editor.
pub fn merge_manually_inner<F>(
    harness_id: HarnessId,
    home: &Path,
    config_dir: &Path,
    options: &ApplyOptions,
    hook_resolver: F,
) -> Result<ConflictResolutionOutcome, IpcError>
where
    F: FnOnce(HarnessId) -> Result<PathBuf, IpcError>,
{
    let preview = preview_patch_inner(harness_id, options, home, hook_resolver)?;
    let path = preview.config_path.clone();
    let original_payload = load_prior_harness_config_in(config_dir, harness_id)?
        .map(|h| h.trove_patch.last_written_region_payload)
        .unwrap_or_default();
    let theirs_region = extract_region(preview.format, &preview.after)
        .map_err(|e| IpcError::Internal {
            reason: format!("could not extract region from preview.after: {e}"),
        })?
        .ok_or_else(|| IpcError::Internal {
            reason: "preview.after has no managed region".into(),
        })?;

    let original_path = sibling_path(&path, "trove.original");
    let theirs_path = sibling_path(&path, "trove.theirs");
    write_atomic(&original_path, original_payload.as_bytes()).map_err(|e| IpcError::Io {
        path: original_path.display().to_string(),
        reason: e.to_string(),
    })?;
    write_atomic(&theirs_path, theirs_region.payload.as_bytes()).map_err(|e| IpcError::Io {
        path: theirs_path.display().to_string(),
        reason: e.to_string(),
    })?;

    Ok(ConflictResolutionOutcome::MergeDeferred {
        sibling_paths: SiblingPaths {
            original: original_path.display().to_string(),
            theirs: theirs_path.display().to_string(),
            host: path.display().to_string(),
        },
    })
}

/// Compute `<host>.<suffix>` next to `host`. Falls back to appending
/// the suffix when the host path has no parent (e.g. relative paths in
/// tests).
fn sibling_path(host: &Path, suffix: &str) -> PathBuf {
    let mut name = host
        .file_name()
        .map_or_else(|| std::ffi::OsString::from("file"), ToOwned::to_owned);
    name.push(".");
    name.push(suffix);
    host.parent().map_or_else(|| PathBuf::from(&name), |p| p.join(&name))
}

/// Hex-encoded SHA-256 of `bytes`. Local copy of the helper from
/// `adapters::common`; pulling it out into a shared module would be
/// over-abstraction for two call sites.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Remove Trove's patch from `harness_id`'s host config and drop the
/// matching entry from `state.json`. Permissive — any managed region
/// present in the host file is removed even when state.json has no
/// record of the harness, so a fresh reinstall can still unwire a
/// previous machine's patch.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn revert_patch(app: tauri::AppHandle, harness_id: HarnessId) -> Result<(), IpcError> {
    let home = home_dir()?;
    match harness_id {
        HarnessId::ClaudeCode => claude_code::revert(&home),
        HarnessId::CodexCli => codex_cli::revert(&home),
        HarnessId::CodexDesktop => codex_desktop::revert(&home),
        HarnessId::GeminiCli => gemini_cli::revert(&home),
        HarnessId::QwenCode => qwen_code::revert(&home),
        // Both Cursor harnesses share `~/.cursor/hooks.json`; either
        // adapter's revert removes the entire managed block. We keep
        // the dispatch separate (rather than collapsing onto a shared
        // arm) so the per-harness state.json upsert/remove call below
        // still receives the correct id.
        HarnessId::CursorIde => cursor_ide::revert(&home),
        HarnessId::CursorCli => cursor_cli::revert(&home),
        HarnessId::Opencode => opencode::revert(&home),
        HarnessId::Cline => cline::revert(&home),
        HarnessId::Aider => aider::revert(&home),
        HarnessId::CopilotCli => copilot_cli::revert(&home),
        // No host file touched on apply, so nothing to undo here.
        // The follow-on watcher abort + state.json remove (below) does
        // the disable work.
        HarnessId::ClaudeDesktop => claude_desktop::revert(&home),
        HarnessId::Droid => droid::revert(&home),
        // Detection-only harnesses: apply never succeeds, so revert is
        // a no-op. Returning Ok rather than HarnessNotImplemented keeps
        // a stray revert call from confusing the UI.
        HarnessId::JunieCli
        | HarnessId::KimiCodeCli
        | HarnessId::Devin
        | HarnessId::Forgecode => Ok(()),
    }?;

    // Sprint 9 PR 2 — abort the Tier 3 watcher (if any) for this id.
    // No-op for Tier 1 / Tier 2 since they never insert.
    {
        use tauri::Manager as _;
        if let Some(registry) = app.try_state::<TierThreeWatchers>() {
            let _aborted = registry.abort(harness_id);
        }
    }

    app_state::remove_harness(&app, harness_id)?;

    Ok(())
}

/// Return the persisted [`AppState`]. A fresh launch (no `state.json`
/// yet) gets [`AppState::default`]: schema v2, null backend, empty
/// harness list.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn get_app_state(app: tauri::AppHandle) -> Result<AppState, IpcError> {
    Ok(app_state::load(&app)?)
}

/// Initial-render fetch for the per-destination health pill. The
/// `backend-health` Tauri event pushes updates afterwards (debounced
/// 250 ms); this command exists so the frontend hook has a value on
/// mount before the first event arrives. Returns a vec sorted by
/// `backendId`; entries with no observations yet have `status: "gray"`.
#[allow(clippy::needless_pass_by_value)]
#[must_use]
#[tauri::command]
pub fn get_backend_health(
    app: tauri::AppHandle,
) -> Vec<crate::collector::BackendHealth> {
    use tauri::Manager;
    app.state::<crate::collector::BackendHealthHandle>()
        .inner()
        .latest()
}

/// Append a new platform to [`AppState::backends`]. Stores each secret
/// in the OS keychain (under id-scoped account names), writes the
/// resulting [`BackendInstance`] to `state.json`, then regenerates
/// `collector.yaml` and recycles the supervised sidecar so telemetry
/// begins flowing to the new destination alongside every other
/// configured platform.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn add_backend(
    app: tauri::AppHandle,
    draft: BackendDraft,
    label: Option<String>,
) -> Result<BackendInstance, IpcError> {
    let id = uuid::Uuid::new_v4().to_string();
    let (backend, secrets_to_store) = drain_secrets_from_draft(draft, &id);

    for secret in &secrets_to_store {
        secrets::store(&secret.account, &secret.value)?;
    }

    let instance = BackendInstance {
        id,
        label: label.filter(|s| !s.trim().is_empty()),
        enabled: true,
        backend,
    };

    let mut state = app_state::load(&app)?;
    state.backends.push(instance.clone());
    app_state::save(&app, &state)?;

    reload_for_backends(&app, &state)?;
    Ok(instance)
}

/// Replace the [`BackendInstance`] with `id` using a fresh `draft`. The
/// caller must re-supply secrets (we never read keychain values back
/// into JS). Wipes the prior instance's keychain entries, stores the
/// new ones under the same id-scoped account names, and reloads the
/// supervisor.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn update_backend(
    app: tauri::AppHandle,
    id: String,
    draft: BackendDraft,
    label: Option<String>,
) -> Result<BackendInstance, IpcError> {
    let mut state = app_state::load(&app)?;
    let slot_idx = state
        .backends
        .iter()
        .position(|b| b.id == id)
        .ok_or_else(|| IpcError::Internal {
            reason: format!("no backend with id {id}"),
        })?;

    // Wipe the prior keychain entries before re-running drain_secrets,
    // which (for OTLP-generic) may produce a different set of account
    // names if the header list changed.
    for account in backend_secret_accounts(&state.backends[slot_idx].backend) {
        secrets::delete(&account)?;
    }

    let (backend, secrets_to_store) = drain_secrets_from_draft(draft, &id);
    for secret in &secrets_to_store {
        secrets::store(&secret.account, &secret.value)?;
    }

    let instance = BackendInstance {
        id: id.clone(),
        label: label.filter(|s| !s.trim().is_empty()),
        // Preserve the prior enabled state across edits so saving a
        // disabled instance from the wizard doesn't silently re-enable it.
        enabled: state.backends[slot_idx].enabled,
        backend,
    };
    state.backends[slot_idx] = instance.clone();
    app_state::save(&app, &state)?;

    reload_for_backends(&app, &state)?;
    Ok(instance)
}

/// Remove the [`BackendInstance`] with `id` from the configured list.
/// Deletes its keychain entries and recycles the supervisor. When the
/// list becomes empty the collector reverts to the smoke (pass-through)
/// config so harnesses still have an OTLP target to talk to.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn remove_backend(app: tauri::AppHandle, id: String) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    let slot_idx = state.backends.iter().position(|b| b.id == id);
    let Some(idx) = slot_idx else {
        return Ok(());
    };

    let removed = state.backends.remove(idx);
    for account in backend_secret_accounts(&removed.backend) {
        secrets::delete(&account)?;
    }
    app_state::save(&app, &state)?;

    reload_for_backends(&app, &state)?;
    Ok(())
}

/// Flip the `enabled` flag on a single [`BackendInstance`]. The instance
/// stays in `state.backends` either way — the collector pipeline is
/// what changes — so the user can disable a platform to pause
/// forwarding without losing its configuration, then re-enable later.
/// No-op (returns `Ok`) when the id is unknown so the JS side doesn't
/// have to coordinate state on a stale view.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_backend_enabled(
    app: tauri::AppHandle,
    id: String,
    enabled: bool,
) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    let Some(slot) = state.backends.iter_mut().find(|b| b.id == id) else {
        return Ok(());
    };
    if slot.enabled == enabled {
        return Ok(());
    }
    slot.enabled = enabled;
    app_state::save(&app, &state)?;
    reload_for_backends(&app, &state)?;
    Ok(())
}

/// Re-render `collector.yaml` from `state.backends` and reload the
/// supervisor. When the enabled list is empty (no backends configured,
/// or all configured backends are disabled), fall back to the smoke
/// config (the `OTel` collector refuses to start with a pipeline that
/// lists no exporters). Used by every backend-mutating IPC command.
fn reload_for_backends(app: &tauri::AppHandle, state: &AppState) -> Result<(), IpcError> {
    // Only enabled backends end up in the rendered pipeline. Disabled
    // ones stay in `state.backends` (so the user can re-enable them
    // without re-entering credentials) but contribute zero exporters.
    let enabled: Vec<BackendInstance> = state
        .backends
        .iter()
        .filter(|b| b.enabled)
        .cloned()
        .collect();
    if enabled.is_empty() {
        crate::reload_collector(app, crate::smoke_config_yaml(), HashMap::new())
            .map_err(|e| boot_error_to_ipc(&e))?;
        return Ok(());
    }
    let rendered = codegen::render(&enabled).map_err(render_error_to_ipc)?;
    let env = unwrap_env(rendered.env);
    let yaml = render_with_overlays(rendered.yaml, state);
    crate::reload_collector(app, &yaml, env).map_err(|e| boot_error_to_ipc(&e))?;
    Ok(())
}

/// Wrap a freshly rendered Collector YAML with the active
/// `resource/identity` overlay when [`AppState::identity`] is enabled,
/// or pass it through unchanged otherwise. Lives next to the IPC
/// callers because the identity probe ladder reads the current
/// detection set, and the IPC layer is where the detection sweep
/// already runs.
///
/// Layers Sprint 13's Tier A mapping overlay on top of the identity
/// overlay so every reload (backend save, identity toggle, mapping
/// apply) regenerates the active collector config with both overlays
/// in their canonical order: identity first (so `resource/identity`
/// sits at the tail of the pipeline list), then mapping (which slots
/// `transform/harness-tag` and the `metricstransform/tierA-*` blocks
/// in *before* identity, preserving identity-tags-everything semantics).
fn render_with_overlays(yaml: String, state: &AppState) -> String {
    let with_identity = if state.identity.enabled {
        let harnesses = crate::detect::detect_all();
        let resolved = crate::identity::resolve(&state.identity, &harnesses);
        crate::collector::codegen::apply_identity_overlay(yaml, &resolved)
    } else {
        yaml
    };
    crate::collector::codegen::apply_mapping_overlay(with_identity, &state.mappings)
}

/// Send a synthetic OTLP/HTTP traces payload through the local
/// collector and return the wizard's "Test export" result. See
/// [`super::test_export`] for the underlying logic.
///
/// The endpoint is hard-coded to `127.0.0.1:4318/v1/traces` — the
/// loopback address the supervised collector binds, identical across
/// every backend. Whether the export succeeds end-to-end depends on
/// the user's saved backend (which the collector forwards to).
#[tauri::command]
pub async fn test_export(app: tauri::AppHandle) -> Result<TestExportResult, IpcError> {
    let log_path = crate::collector_log_path(&app).map_err(|e| boot_error_to_ipc(&e))?;
    let result = test_export_at(
        "http://127.0.0.1:4318/v1/traces",
        &log_path,
        DEFAULT_TEST_BUDGET,
    )
    .await;
    Ok(result)
}

/// Wipe every configured backend: delete every keychain entry, empty
/// `state.backends`, restore the smoke `collector.yaml`, and recycle
/// the supervisor with no env vars set. Idempotent — a no-op when the
/// list is already empty.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn clear_backend(app: tauri::AppHandle) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    if state.backends.is_empty() {
        return Ok(());
    }
    let drained: Vec<BackendInstance> = std::mem::take(&mut state.backends);
    for inst in &drained {
        for account in backend_secret_accounts(&inst.backend) {
            secrets::delete(&account)?;
        }
    }
    app_state::save(&app, &state)?;
    crate::reload_collector(&app, crate::smoke_config_yaml(), HashMap::new())
        .map_err(|e| boot_error_to_ipc(&e))?;
    Ok(())
}

/// Quit the application from the React UI. Mirrors the tray menu's
/// `quit` handler so the existing `RunEvent::ExitRequested` path in
/// `lib.rs` still runs (which shuts the collector down cleanly).
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) -> Result<(), IpcError> {
    app.exit(0);
    Ok(())
}

/// Uninstall Trove from the user's machine. Optionally wipes persisted
/// data (state.json, secrets.json, collector logs, keychain residue).
///
/// The running app cannot delete its own installed bundle directly —
/// macOS keeps the executable file mapped, and on Windows the binary
/// is locked while running. We instead spawn a detached helper that
/// polls our PID, waits for the process to exit, then removes the
/// install path. The helper inherits no stdio so it survives parent
/// death cleanly.
///
/// Best-effort across platforms:
/// - **macOS:** removes `<...>.app` (walked up from `current_exe`).
/// - **Linux:** removes the `APPIMAGE` file when set; otherwise warns
///   (deb/pacman installs need privileged removal).
/// - **Windows:** removes the install directory (parent of
///   `current_exe`) — guarded by a `trove.exe` presence check.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn uninstall_app(app: tauri::AppHandle, remove_data: bool) -> Result<(), IpcError> {
    use tauri::Manager as _;

    if remove_data {
        if let Ok(state) = app_state::load(&app) {
            for inst in &state.backends {
                for account in backend_secret_accounts(&inst.backend) {
                    if let Err(e) = secrets::delete(&account) {
                        tracing::warn!(account = %account, error = %e, "uninstall: secret delete failed");
                    }
                }
            }
        }
        for dir in [
            app.path().app_config_dir().ok(),
            app.path().app_data_dir().ok(),
            app.path().app_log_dir().ok(),
        ]
        .into_iter()
        .flatten()
        {
            if dir.exists() {
                if let Err(e) = std::fs::remove_dir_all(&dir) {
                    tracing::warn!(?dir, error = %e, "uninstall: data dir cleanup failed");
                }
            }
        }
    }

    if let Some(install_path) = resolve_install_path_for_uninstall() {
        if let Err(e) = spawn_post_exit_cleanup(&install_path) {
            tracing::warn!(?install_path, error = %e, "uninstall: could not schedule cleanup helper");
        } else {
            tracing::info!(?install_path, "uninstall: cleanup helper scheduled");
        }
    } else {
        tracing::warn!("uninstall: could not resolve install path; exiting without bundle removal");
    }

    app.exit(0);
    Ok(())
}

/// Resolve the install path the post-exit cleanup helper should
/// remove. Returns `None` for dev builds and for Linux non-AppImage
/// layouts where unprivileged removal isn't safe.
fn resolve_install_path_for_uninstall() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let exe = std::env::current_exe().ok()?;
        for ancestor in exe.ancestors() {
            if ancestor
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("app"))
            {
                return Some(ancestor.to_path_buf());
            }
        }
        None
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(appimage) = std::env::var("APPIMAGE") {
            let p = PathBuf::from(appimage);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?.to_path_buf();
        // Guard: only proceed when the install dir clearly contains
        // our binary, so a custom-layout install doesn't accidentally
        // remove a parent that owns other apps.
        if dir.join("trove.exe").exists() {
            Some(dir)
        } else {
            None
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Spawn a detached child that waits for our PID to exit and then
/// removes `install_path`. Stdio is detached so the child outlives us.
fn spawn_post_exit_cleanup(install_path: &Path) -> std::io::Result<()> {
    let pid = std::process::id();
    #[cfg(unix)]
    {
        // Quote-safe via shell single-quoting; reject paths containing
        // a single quote so the inline script can't be broken out of.
        let path_str = install_path.display().to_string();
        if path_str.contains('\'') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "install path contains a single quote",
            ));
        }
        let script = format!(
            "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; rm -rf '{path_str}'"
        );
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&script)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let path_str = install_path.display().to_string();
        // cmd.exe waits while tasklist still reports our PID, then
        // removes the install dir. /D ensures Y/N prompts get answered.
        let script = format!(
            ":loop\r\n\
             tasklist /FI \"PID eq {pid}\" /NH 2>NUL | find \"{pid}\" >NUL\r\n\
             if not errorlevel 1 (timeout /T 1 /NOBREAK >NUL & goto loop)\r\n\
             rmdir /S /Q \"{path_str}\"\r\n"
        );
        std::process::Command::new("cmd.exe")
            .args(["/D", "/C", &script])
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = install_path;
        let _ = pid;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "post-exit cleanup not implemented on this platform",
        ))
    }
}

/// Sprint 10 — flip the persisted `auto_update_enabled` flag in
/// `state.json`. Drives nothing on its own; the background-on-launch
/// update probe (when wired) reads this flag, and the React Settings
/// component renders the toggle's checked state from it.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_auto_update_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    if state.auto_update_enabled == enabled {
        return Ok(());
    }
    state.auto_update_enabled = enabled;
    app_state::save(&app, &state)?;
    Ok(())
}

/// Flip the persisted `launch_at_startup_enabled` opt-out and apply the
/// change to the OS login-items mechanism in one step. The plugin
/// writes a `LaunchAgent` on macOS, a Run-key entry on Windows, and a
/// `.desktop` autostart file on Linux. A failure to apply the OS-side
/// change leaves the persisted preference untouched so the UI doesn't
/// drift out of sync with reality.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_launch_at_startup_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    if state.launch_at_startup_enabled == enabled {
        return Ok(());
    }
    apply_launch_at_startup(&app, enabled).map_err(|e| IpcError::Internal {
        reason: format!("autostart: {e}"),
    })?;
    state.launch_at_startup_enabled = enabled;
    app_state::save(&app, &state)?;
    Ok(())
}

/// Apply the launch-at-startup preference to the OS login-items
/// mechanism via `tauri-plugin-autostart`. Idempotent: re-enabling an
/// already-registered entry or disabling a non-existent one is a no-op
/// at the plugin layer.
fn apply_launch_at_startup(
    app: &tauri::AppHandle,
    enabled: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable()?;
    } else {
        mgr.disable()?;
    }
    Ok(())
}

/// Bring the OS-side login item into agreement with the user's
/// persisted `launch_at_startup_enabled` preference. Runs at startup
/// from `respawn_persisted_watchers`; this is the hook that registers
/// the `LaunchAgent` the first time a v7→v8 upgrade or a fresh install
/// produces `launch_at_startup_enabled = true` without a corresponding
/// entry on disk. Skipped in debug builds so `pnpm tauri dev` doesn't
/// register a launch agent pointing at a `target/debug` binary path.
/// All failures are logged at `warn!` and swallowed — the user can fix
/// drift by toggling the Settings switch.
pub fn reconcile_launch_at_startup(app: &tauri::AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    if cfg!(debug_assertions) {
        tracing::debug!("reconcile_launch_at_startup: skipped in debug build");
        return;
    }
    let state = match app_state::load(app) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "reconcile_launch_at_startup: state.json load failed");
            return;
        }
    };
    let mgr = app.autolaunch();
    let registered = match mgr.is_enabled() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "reconcile_launch_at_startup: is_enabled failed");
            return;
        }
    };
    if registered == state.launch_at_startup_enabled {
        return;
    }
    let result = if state.launch_at_startup_enabled {
        mgr.enable()
    } else {
        mgr.disable()
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, "reconcile_launch_at_startup: enable/disable failed");
    }
}

/// Sprint 12 — opt-in identity tagging. Flips the persisted
/// [`AppState::identity::enabled`] flag, then reloads the collector so
/// the active YAML reflects the new processor list immediately. When
/// the collector is not yet running (no backend saved), this is a
/// pure state mutation.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_identity_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    if state.identity.enabled == enabled {
        return Ok(());
    }
    state.identity.enabled = enabled;
    app_state::save(&app, &state)?;
    reload_collector_for_identity(&app, &state)?;
    Ok(())
}

/// Sprint 12 — persist a user-entered name/email override and pin the
/// source to [`crate::app_state::IdentitySource::Manual`]. Empty
/// values are accepted (mirrors the wizard's "clear my overrides"
/// affordance); the resolve ladder falls through when both are empty.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_identity_manual(
    app: tauri::AppHandle,
    name: String,
    email: String,
) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    state.identity.source = crate::app_state::IdentitySource::Manual;
    state.identity.name = name;
    state.identity.email = email;
    app_state::save(&app, &state)?;
    reload_collector_for_identity(&app, &state)?;
    Ok(())
}

/// Sprint 12 — pin the source back to
/// [`crate::app_state::IdentitySource::Auto`] without touching the
/// persisted name/email. The retained values are kept as a fallback
/// after the harness and git layers in [`crate::identity::resolve`].
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_identity_auto(app: tauri::AppHandle) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    if matches!(state.identity.source, crate::app_state::IdentitySource::Auto) {
        return Ok(());
    }
    state.identity.source = crate::app_state::IdentitySource::Auto;
    app_state::save(&app, &state)?;
    reload_collector_for_identity(&app, &state)?;
    Ok(())
}

/// Sprint 12 — preview the resolved identity without persisting. The
/// React Settings panel calls this on mount and after each mutation
/// to render "Source: detected from <harness> | git config | manual
/// entry | none" with the values the next collector reload would
/// pick up.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn resolve_identity_preview(
    app: tauri::AppHandle,
) -> Result<crate::identity::Resolved, IpcError> {
    let state = app_state::load(&app)?;
    let harnesses = crate::detect::detect_all();
    Ok(crate::identity::resolve(&state.identity, &harnesses))
}

/// Shared helper: regenerate `collector.yaml` and recycle the
/// supervised collector with the current identity overlay applied.
/// When no backend is saved yet, the collector is in smoke-config
/// mode and the overlay has nothing to wrap; this is a no-op.
fn reload_collector_for_identity(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<(), IpcError> {
    let enabled: Vec<BackendInstance> = state
        .backends
        .iter()
        .filter(|b| b.enabled)
        .cloned()
        .collect();
    if enabled.is_empty() {
        return Ok(());
    }
    let rendered = codegen::render(&enabled).map_err(render_error_to_ipc)?;
    let env = unwrap_env(rendered.env);
    let yaml = render_with_overlays(rendered.yaml, state);
    crate::reload_collector(app, &yaml, env).map_err(|e| boot_error_to_ipc(&e))
}

/// Sprint 13 — replace the persisted per-harness Tier A mapping table
/// with `mappings` and recycle the collector so the new YAML's
/// `transform/harness-tag` and `metricstransform/tierA-*` blocks take
/// effect. Validates the mapping graph first so invariants
/// (double-emit, out-of-domain attribute values, unknown schema
/// version) surface at the IPC boundary rather than as a confused
/// collector restart.
///
/// When no backend is saved yet, mapping changes are persisted but no
/// collector reload is needed — the smoke config doesn't carry the
/// overlay anyway.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn apply_mappings(
    app: tauri::AppHandle,
    mappings: crate::mappings::MappingState,
) -> Result<(), IpcError> {
    use tauri::Manager as _;
    crate::mappings::validate(&mappings).map_err(|e| IpcError::Internal {
        reason: format!("invalid mapping state: {e}"),
    })?;
    let mut state = app_state::load(&app)?;
    if state.mappings == mappings {
        // No-op: a noisy UI may call apply on every mount; bail before
        // touching the supervisor.
        return Ok(());
    }
    state.mappings = mappings.clone();
    app_state::save(&app, &state)?;
    // Push the new state to in-process watchers (Cline, Gemini, Claude
    // Desktop, Aider/Copilot wrappers). They consult the store at emit
    // time, so user edits to hook rules take effect immediately — no
    // watcher restart needed.
    if let Some(store) = app.try_state::<crate::mappings::MappingStateStore>() {
        store.publish(mappings);
    }
    // Cursor hook is out-of-process JS — regenerate the script so the
    // user's edits take effect on the next Cursor reload.
    let _ = crate::adapters::cursor_common::regenerate_hooks_for_rules(&app, &state.mappings);
    let enabled: Vec<BackendInstance> = state
        .backends
        .iter()
        .filter(|b| b.enabled)
        .cloned()
        .collect();
    if enabled.is_empty() {
        return Ok(());
    }
    let rendered = codegen::render(&enabled).map_err(render_error_to_ipc)?;
    let env = unwrap_env(rendered.env);
    let yaml = render_with_overlays(rendered.yaml, &state);
    crate::reload_collector(&app, &yaml, env).map_err(|e| boot_error_to_ipc(&e))
}

/// Sprint 13 — reset the per-harness Tier A mapping table to Trove's
/// shipped defaults. Same restart semantics as [`apply_mappings`].
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn reset_mappings_to_defaults(app: tauri::AppHandle) -> Result<(), IpcError> {
    apply_mappings(app, crate::mappings::default_state())
}

// ---------------------------------------------------------------------------
// Simulate a single mapping row against a sample input
// ---------------------------------------------------------------------------

/// Input shape for [`simulate_mapping`]. Caller passes their working
/// draft state (which may differ from the persisted state) so preview
/// works on in-flight edits without requiring an apply round-trip.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateMappingInput {
    /// The full mapping state to simulate against. Validated before
    /// simulation so callers can't slip an inconsistent draft past the
    /// usual checks.
    pub mapping_state: crate::mappings::MappingState,
    pub harness_id: crate::harness::HarnessId,
    /// Index into the chosen harness's `sources` array. Caller picked
    /// a specific rule to preview.
    pub source_index: usize,
    /// Attributes on the sample input. For `synthesize-from-native` rules
    /// these are the attributes on the raw `OTel` data point; for
    /// `hook-rule` rules they're the attributes the driver would have
    /// attached at hook fire time.
    #[serde(default)]
    pub sample_attributes: std::collections::BTreeMap<String, String>,
    /// Optional value for the synthesized data point. Defaults to 1.0
    /// when omitted; ignored for `hook-rule` rules (those carry their
    /// own value semantics).
    #[serde(default)]
    pub sample_value: Option<f64>,
}

/// Output shape for [`simulate_mapping`]. `emitted` is `None` when the
/// rule explicitly suppresses emission (`HookRule` with `emit: null`) or
/// when the target metric id doesn't resolve. `notes` carry warnings
/// the UI should surface inline (e.g. "this metric requires attribute
/// X, but your sample doesn't carry it").
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateMappingOutput {
    pub emitted: Option<SimulatedMetric>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatedMetric {
    pub metric_id: String,
    pub metric_name: String,
    pub kind: crate::mappings::TroveMetricKind,
    pub attributes: std::collections::BTreeMap<String, String>,
    pub value: f64,
}

/// Pure simulator over `MappingState` — applies one rule's transform to
/// a sample input and returns what would be emitted. Powers the
/// "Preview" sheet in the Mappings editor. No side effects: doesn't
/// read or modify persisted state, doesn't touch the collector, can
/// be called freely without coordination.
///
/// The simulator validates the passed state first so the same
/// invariants the apply path enforces (unknown metric id, duplicate
/// catalog id, etc.) surface here too.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn simulate_mapping(
    input: SimulateMappingInput,
) -> Result<SimulateMappingOutput, IpcError> {
    use crate::mappings::MappingSource;

    crate::mappings::validate(&input.mapping_state).map_err(|e| IpcError::Internal {
        reason: format!("invalid draft mapping state: {e}"),
    })?;

    let harness = input
        .mapping_state
        .for_harness(input.harness_id)
        .ok_or_else(|| IpcError::Internal {
            reason: format!(
                "harness {:?} not found in passed mapping state",
                input.harness_id
            ),
        })?;
    let source = harness.sources.get(input.source_index).ok_or_else(|| {
        IpcError::Internal {
            reason: format!(
                "source index {} out of range for harness {:?} (has {} rules)",
                input.source_index,
                input.harness_id,
                harness.sources.len()
            ),
        }
    })?;

    let mut notes: Vec<String> = Vec::new();

    let (metric_id, mut attributes, value) = match source {
        MappingSource::HookRule { emit, .. } => {
            let Some(e) = emit.as_ref() else {
                notes.push(
                    "this rule is configured to suppress emission (emit: null)".to_string(),
                );
                return Ok(SimulateMappingOutput {
                    emitted: None,
                    notes,
                });
            };
            // HookEmit.attributes are constants injected by the
            // driver — they override anything in the sample for those
            // keys.
            let mut attrs = input.sample_attributes.clone();
            for (k, v) in &e.attributes {
                attrs.insert(k.clone(), v.clone());
            }
            (e.metric.clone(), attrs, input.sample_value.unwrap_or(1.0))
        }
        MappingSource::SynthesizeFromNative {
            target_metric,
            attribute_map,
            inject_attributes,
            ..
        } => {
            // Apply attribute_map: rename raw key → target key.
            let mut attrs: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::new();
            for (k, v) in &input.sample_attributes {
                let new_key = attribute_map.get(k).cloned().unwrap_or_else(|| k.clone());
                attrs.insert(new_key, v.clone());
            }
            // Inject constant attributes (overrides any conflicting
            // renamed key — same precedence as the collector transform).
            for (k, v) in inject_attributes {
                attrs.insert(k.clone(), v.clone());
            }
            (
                target_metric.clone(),
                attrs,
                input.sample_value.unwrap_or(1.0),
            )
        }
    };

    let def = input.mapping_state.metric(&metric_id).ok_or_else(|| {
        IpcError::Internal {
            reason: format!("rule targets unknown metric id {metric_id:?}"),
        }
    })?;

    // Warn about required attributes that the simulated output doesn't
    // carry — these would result in dashboard filters seeing empty
    // results on the wire.
    for required in &def.required_attributes {
        if !attributes.contains_key(required) {
            notes.push(format!(
                "target metric requires attribute {required:?} but the simulated output doesn't carry it; \
                 consider adding it to injectAttributes or the rule's sample"
            ));
        }
    }

    // Mutation no-op to consume the marker (lints).
    let _ = &mut attributes;

    Ok(SimulateMappingOutput {
        emitted: Some(SimulatedMetric {
            metric_id: def.id.clone(),
            metric_name: def.name.clone(),
            kind: def.kind,
            attributes,
            value,
        }),
        notes,
    })
}

/// Sprint 10 — outcome of `check_for_updates`. The React Settings
/// component renders `available + version` ("update to v0.6.1
/// available") or just `current` ("you're on v0.6.0, no update").
/// `current` is always populated; `version` only when an update was
/// found.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    /// `true` when the updater located a newer release.
    pub available: bool,
    /// Semver string of the available release. `None` when
    /// `available` is `false`.
    pub version: Option<String>,
    /// Semver string of the running build (from `CARGO_PKG_VERSION`).
    pub current: String,
}

/// Sprint 10 — explicit "check for updates now" probe. The Tauri
/// updater plugin fetches the signed `latest.json` manifest from the
/// GitHub Releases endpoint configured in `tauri.conf.json` and
/// compares the version against the running build. Always runs (the
/// auto-on-launch flag is a separate background path); failures
/// surface as `IpcError::UpdaterCheckFailed`.
#[tauri::command]
pub async fn check_for_updates(app: tauri::AppHandle) -> Result<UpdateMetadata, IpcError> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app
        .updater()
        .map_err(|e| IpcError::UpdaterCheckFailed { reason: e.to_string() })?;
    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateMetadata {
            available: true,
            version: Some(update.version.clone()),
            current: crate::app_version().to_string(),
        }),
        Ok(None) => Ok(UpdateMetadata {
            available: false,
            version: None,
            current: crate::app_version().to_string(),
        }),
        Err(e) => Err(IpcError::UpdaterCheckFailed { reason: e.to_string() }),
    }
}

/// Convert codegen's secrecy-wrapped env map into the plain map the
/// supervisor's `Command::envs` consumes. Each [`Zeroizing`] string
/// drops as the iterator advances; a momentary plain-`String` copy
/// lives in the new `HashMap` until the supervisor passes it to the
/// child. We accept that trade-off — `OsString` (which is what
/// `Command::env` actually stores) doesn't impl `Zeroize`.
fn unwrap_env(env: HashMap<String, zeroize::Zeroizing<String>>) -> HashMap<String, String> {
    env.into_iter().map(|(k, v)| (k, (*v).clone())).collect()
}

fn render_error_to_ipc(err: codegen::RenderError) -> IpcError {
    match err {
        codegen::RenderError::Keychain { account, source } => IpcError::Internal {
            reason: format!("codegen could not read keychain entry {account}: {source}"),
        },
    }
}

fn boot_error_to_ipc(err: &crate::CollectorBootError) -> IpcError {
    IpcError::Internal {
        reason: format!("collector reload failed: {err}"),
    }
}

fn home_dir() -> Result<PathBuf, IpcError> {
    dirs::home_dir().ok_or(IpcError::Internal {
        reason: "could not resolve user home directory".into(),
    })
}

/// Resolve the absolute path of the harness's host config file under
/// `home`. Used to populate [`HarnessConfig.config_path`] after a
/// successful apply.
#[must_use]
pub fn harness_config_path(id: HarnessId, home: &Path) -> PathBuf {
    match id {
        HarnessId::ClaudeCode => claude_code::config_path(home),
        HarnessId::CodexCli => codex_cli::config_path(home),
        HarnessId::CodexDesktop => codex_desktop::config_path(home),
        HarnessId::GeminiCli => gemini_cli::config_path(home),
        HarnessId::QwenCode => qwen_code::config_path(home),
        // Both Cursor harnesses share `~/.cursor/hooks.json`. The
        // state.json entry records the same path under either id; the
        // shared underlying file is the source of truth.
        HarnessId::CursorIde => cursor_ide::config_path(home),
        HarnessId::CursorCli => cursor_cli::config_path(home),
        HarnessId::Opencode => opencode::config_path(home),
        HarnessId::Cline => cline::config_path(home),
        HarnessId::Aider => aider::config_path(home),
        HarnessId::CopilotCli => copilot_cli::config_path(home),
        // Cowork's per-session audit logs root (no Trove-managed file
        // lives there; the Harnesses tab uses this string only for the
        // "config path" tooltip).
        HarnessId::ClaudeDesktop => claude_desktop::config_path(home),
        HarnessId::Droid => droid::config_path(home),
        // Detection-only harnesses: dotfile dir matches config_search_paths;
        // used only as a display tooltip in the Harnesses tab.
        HarnessId::JunieCli => home.join(".junie"),
        HarnessId::KimiCodeCli => home.join(".kimi"),
        HarnessId::Devin => home.join(".devin"),
        HarnessId::Forgecode => home.join(".forge"),
    }
}

/// Resolve the absolute path of the bundled wrapper / hook script for
/// `id`. Used by adapters that depend on a vendored on-disk resource.
/// Tier 1 / Cline don't ship one and never call this.
fn external_resource_path(app: &tauri::AppHandle, id: HarnessId) -> Result<PathBuf, IpcError> {
    use tauri::Manager as _;
    let resource_dir = app.path().resource_dir().map_err(|e| IpcError::Internal {
        reason: format!("could not resolve Tauri resource_dir: {e}"),
    })?;
    let rel: &[&str] = match id {
        HarnessId::CursorIde => &["resources", "hooks", "cursor-otel-hook.cjs"],
        // cursor-cli no longer shares cursor-ide's hooks.json — Cursor's
        // hook system is IDE-only. See `adapters::cursor_cli` for the
        // wrapper-based replacement.
        HarnessId::CursorCli => &["resources", "wrappers", "trove-cursor-agent"],
        HarnessId::Aider => &["resources", "wrappers", "trove-aider"],
        HarnessId::CopilotCli => &["resources", "wrappers", "trove-copilot"],
        _ => {
            return Err(IpcError::Internal {
                reason: format!("no bundled resource for harness {id:?}"),
            });
        }
    };
    let mut p = resource_dir;
    for seg in rel {
        p = p.join(seg);
    }
    Ok(p)
}

/// Walk every harness in the persisted [`AppState`] and respawn its
/// supplementary watcher (if any). Called once during app setup so
/// previously applied harnesses get their watchers back after a
/// relaunch — without this, watchers only run from the moment the user
/// re-applies, which is wrong for emissions sourced from on-disk logs
/// the harness keeps writing across restarts.
///
/// Non-fatal: `state.json` errors and missing-id arms are logged at
/// `warn!`; setup continues.
pub fn respawn_persisted_watchers(app: &tauri::AppHandle) {
    // First-run: auto-enable Claude Desktop so its audit-log tap fires
    // out-of-the-box. Idempotent — a no-op once any explicit user
    // choice (enable/disable) exists in state.json.
    autoenable_claude_desktop_on_first_run(app);

    // Reconcile the OS login-items mechanism with the persisted
    // launch_at_startup preference. Registers the LaunchAgent on first
    // launch after a v7→v8 migration, and re-registers it if the user
    // has manually removed the entry from System Settings.
    reconcile_launch_at_startup(app);

    let state = match app_state::load(app) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "respawn_persisted_watchers: state.json load failed");
            return;
        }
    };
    let Ok(home) = home_dir() else {
        tracing::warn!("respawn_persisted_watchers: HOME unresolvable");
        return;
    };
    // Seed the cursor hook sidecar so the first Cursor invocation after
    // a relaunch reads the persisted rules — not the stale defaults
    // from the bundled cjs script. Idempotent best-effort.
    let _ = crate::adapters::cursor_common::regenerate_hooks_for_rules(app, &state.mappings);
    for harness in &state.harnesses {
        if !harness.enabled {
            continue;
        }
        spawn_tier3_watcher(app, harness.id, &home, &harness.options);
    }
}

/// On a fresh install (no Claude Desktop entry in state.json), upsert
/// an enabled `HarnessConfig` so the regular respawn loop picks it up.
/// Idempotent: a no-op once any entry exists (the user's explicit
/// enable/disable choice always wins). Failures are logged at `warn!`
/// and swallowed — the worst case is the watcher just doesn't start
/// this boot, which the user can fix by clicking Enable.
fn autoenable_claude_desktop_on_first_run(app: &tauri::AppHandle) {
    // Only proceed if Desktop is actually installed on this machine.
    // Reuses the same detection signal the Harnesses tab shows so the
    // two views stay in sync.
    let desktop_detected = detect_all()
        .into_iter()
        .any(|h| h.id == HarnessId::ClaudeDesktop && h.detected);
    if !desktop_detected {
        tracing::debug!("autoenable: claude-desktop not detected on this machine; skipping");
        return;
    }
    let state = match app_state::load(app) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "autoenable: state.json load failed");
            return;
        }
    };
    if state
        .harnesses
        .iter()
        .any(|h| h.id == HarnessId::ClaudeDesktop)
    {
        return;
    }
    let Ok(home) = home_dir() else {
        tracing::warn!("autoenable: HOME unresolvable; skipping claude-desktop");
        return;
    };
    let opts = ApplyOptions::default();
    let patch = match claude_desktop::apply(&home, &opts) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = ?e, "autoenable: claude_desktop::apply failed");
            return;
        }
    };
    let entry = harness_config_from_apply(
        HarnessId::ClaudeDesktop,
        &claude_desktop::config_path(&home),
        opts,
        patch,
    );
    if let Err(e) = app_state::upsert_harness(app, entry) {
        tracing::warn!(error = %e, "autoenable: upsert_harness failed");
    }
}

/// Spawn the appropriate Tier 3 watcher and register it in the
/// long-lived `TierThreeWatchers` slot. No-op for non-tier-3 ids.
fn spawn_tier3_watcher(
    app: &tauri::AppHandle,
    id: HarnessId,
    home: &Path,
    options: &ApplyOptions,
) {
    use tauri::Manager as _;
    // Subscribe the watcher to the live mapping store so user edits
    // (apply_mappings IPC) take effect without restarting. Falls back
    // to a fresh-default store when the slot is missing (test paths,
    // boot ordering): the watcher still works on the builtin rules.
    let mappings = if let Some(store) = app.try_state::<crate::mappings::MappingStateStore>() {
        store.subscribe()
    } else {
        let store = crate::mappings::MappingStateStore::new(crate::mappings::default_state());
        tracing::warn!("MappingStateStore slot missing; watcher using ephemeral defaults");
        store.subscribe()
    };
    let handle = match id {
        HarnessId::Cline => Some(cline_watcher::spawn(
            cline::tasks_dir(home),
            options.clone(),
            mappings,
            cline_watcher::DEFAULT_POLL_INTERVAL,
        )),
        HarnessId::Aider => {
            let log = aider::log_path(home);
            ensure_log_parent(&log);
            Some(spawn_wrapper_log_watcher(log, options.clone(), id, mappings))
        }
        HarnessId::CopilotCli => {
            let log = copilot_cli::log_path(home);
            ensure_log_parent(&log);
            Some(spawn_wrapper_log_watcher(log, options.clone(), id, mappings))
        }
        HarnessId::CursorCli => {
            let log = cursor_cli::log_path(home);
            ensure_log_parent(&log);
            Some(spawn_wrapper_log_watcher(log, options.clone(), id, mappings))
        }
        // Gemini emits Tier B natively, but the chat-log watcher fills
        // the gaps: per-turn `cost.usd` (metricstransform can't do
        // per-model rate × token-count math) and reliable
        // `tokens` / `turn.duration` with `model` labels, sourced from
        // `~/.gemini/tmp/<proj>/chats/session-*.jsonl`.
        HarnessId::GeminiCli => Some(gemini_watcher::spawn(
            home.join(".gemini").join("tmp"),
            options.clone(),
            mappings,
            gemini_watcher::DEFAULT_POLL_INTERVAL,
        )),
        // Claude Desktop (Cowork) has no admin-OTLP path that actually
        // works upstream (Anthropic #39471, #38984). We tail
        // `audit.jsonl` files Cowork writes per session and synthesise
        // Tier A metrics directly. See `claude_desktop_watcher`.
        HarnessId::ClaudeDesktop => Some(claude_desktop_watcher::spawn(
            claude_desktop_watcher::sessions_root(home),
            options.clone(),
            mappings,
            claude_desktop_watcher::DEFAULT_POLL_INTERVAL,
        )),
        // Tier 1 native-OTLP harnesses need no watcher — the SDK pushes
        // directly to the collector. Droid is included here.
        HarnessId::ClaudeCode
        | HarnessId::Droid
        | HarnessId::CodexCli
        | HarnessId::CodexDesktop
        | HarnessId::CursorIde
        | HarnessId::QwenCode
        | HarnessId::Opencode
        // Detection-only harnesses never spawn a watcher.
        | HarnessId::JunieCli
        | HarnessId::KimiCodeCli
        | HarnessId::Devin
        | HarnessId::Forgecode => None,
    };
    let Some(handle) = handle else { return };
    if let Some(registry) = app.try_state::<TierThreeWatchers>() {
        registry.insert(id, handle);
    } else {
        tracing::warn!(?id, "TierThreeWatchers slot missing; watcher aborted");
    }
}

/// Best-effort `mkdir -p` for the wrapper log file's parent. The
/// wrapper script also tries this; duplicating it here means the
/// `log_watcher`'s first poll doesn't have to wait for the user to
/// invoke the wrapper before the parent appears.
fn ensure_log_parent(log: &Path) {
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

/// Tail `log_path` and emit one OTLP log + one Tier A metrics payload
/// per parseable line. Returns a `WatcherHandle` whose `abort()` halts
/// the chain (it owns the inner tail watcher's handle so dropping
/// cancels both).
fn spawn_wrapper_log_watcher(
    log_path: PathBuf,
    options: ApplyOptions,
    id: HarnessId,
    mappings: crate::mappings::SharedMappingState,
) -> crate::log_watcher::WatcherHandle {
    use crate::log_watcher::{DEFAULT_POLL_INTERVAL, spawn as spawn_tail};
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
    let tail = spawn_tail(log_path, tx, DEFAULT_POLL_INTERVAL);
    let join = tokio::spawn(async move {
        // The tail handle moves into the task so cancelling this outer
        // task drops it (and so cancels the inner watcher).
        let _tail = tail;
        while let Some(line) = rx.recv().await {
            // Snapshot the live mapping state at emit time so user
            // edits (via apply_mappings) take effect on the next
            // wrapper invocation without a watcher restart.
            let mapping_snapshot = mappings.current();
            let log_payload = match id {
                HarnessId::Aider => aider::parse_event_line(&line, &options),
                HarnessId::CopilotCli => copilot_cli::parse_event_line(&line, &options),
                HarnessId::CursorCli => cursor_cli::parse_event_line(&line, &options),
                _ => None,
            };
            let metric_payload = match id {
                HarnessId::Aider => {
                    aider::parse_event_metric_payload(&line, &options, mapping_snapshot.clone())
                }
                HarnessId::CopilotCli => copilot_cli::parse_event_metric_payload(
                    &line,
                    &options,
                    mapping_snapshot.clone(),
                ),
                HarnessId::CursorCli => cursor_cli::parse_event_metric_payload(
                    &line,
                    &options,
                    mapping_snapshot.clone(),
                ),
                _ => None,
            };
            if let Some(payload) = log_payload {
                if let Err(e) = crate::otlp_emit::post_logs_json(&payload).await {
                    tracing::warn!(error = %e, ?id, "wrapper log watcher OTLP log emit failed");
                }
            }
            if let Some(payload) = metric_payload {
                if let Err(e) = crate::otlp_emit::post_metrics_json(&payload).await {
                    tracing::warn!(error = %e, ?id, "wrapper log watcher OTLP metric emit failed");
                }
            }
        }
    });
    crate::log_watcher::WatcherHandle::from_join(join)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // simulate_mapping
    // -----------------------------------------------------------------

    fn state_with_one_synth_rule(target_id: &str) -> crate::mappings::MappingState {
        use crate::mappings::{HarnessMapping, MappingSource};
        use std::collections::BTreeMap;
        let mut state = crate::mappings::default_state();
        state.harnesses.retain(|h| h.harness_id != crate::harness::HarnessId::ClaudeCode);
        state.harnesses.push(HarnessMapping {
            harness_id: crate::harness::HarnessId::ClaudeCode,
            enabled: true,
            sources: vec![MappingSource::SynthesizeFromNative {
                native_metric: "claude_code.tool.usage".into(),
                target_metric: target_id.to_string(),
                attribute_map: BTreeMap::from([
                    ("type".to_string(), "direction".to_string()),
                ]),
                inject_attributes: BTreeMap::from([
                    ("event.kind".to_string(), "tool.call".to_string()),
                ]),
            }],
            cost_overrides: BTreeMap::new(),
        });
        state
    }

    #[test]
    fn simulate_mapping_synthesizes_target_with_renamed_and_injected_attrs() {
        let state = state_with_one_synth_rule("events");
        let out = simulate_mapping(SimulateMappingInput {
            mapping_state: state,
            harness_id: crate::harness::HarnessId::ClaudeCode,
            source_index: 0,
            sample_attributes: std::collections::BTreeMap::from([
                ("type".to_string(), "input".to_string()),
                ("model".to_string(), "sonnet-4".to_string()),
            ]),
            sample_value: Some(3.0),
        })
        .unwrap();
        let emitted = out.emitted.unwrap();
        assert_eq!(emitted.metric_id, "events");
        assert_eq!(emitted.metric_name, "trove.harness.events");
        assert!((emitted.value - 3.0).abs() < f64::EPSILON);
        // `type` renamed to `direction`.
        assert_eq!(emitted.attributes.get("direction"), Some(&"input".to_string()));
        assert!(!emitted.attributes.contains_key("type"));
        // `model` passes through unchanged (not in attribute_map).
        assert_eq!(emitted.attributes.get("model"), Some(&"sonnet-4".to_string()));
        // Injected literal lands on the output.
        assert_eq!(
            emitted.attributes.get("event.kind"),
            Some(&"tool.call".to_string())
        );
    }

    #[test]
    fn simulate_mapping_warns_when_required_attribute_missing() {
        // Claude Code rule targeting `tokens` (which requires `direction`),
        // but the sample doesn't include `direction` — should warn.
        use crate::mappings::{HarnessMapping, MappingSource};
        let mut state = crate::mappings::default_state();
        state.harnesses.retain(|h| h.harness_id != crate::harness::HarnessId::ClaudeCode);
        state.harnesses.push(HarnessMapping {
            harness_id: crate::harness::HarnessId::ClaudeCode,
            enabled: true,
            sources: vec![MappingSource::SynthesizeFromNative {
                native_metric: "claude_code.token.usage".into(),
                target_metric: "tokens".to_string(),
                attribute_map: std::collections::BTreeMap::new(),
                inject_attributes: std::collections::BTreeMap::new(),
            }],
            cost_overrides: std::collections::BTreeMap::new(),
        });
        let out = simulate_mapping(SimulateMappingInput {
            mapping_state: state,
            harness_id: crate::harness::HarnessId::ClaudeCode,
            source_index: 0,
            sample_attributes: std::collections::BTreeMap::new(),
            sample_value: None,
        })
        .unwrap();
        assert!(
            out.notes
                .iter()
                .any(|n| n.contains("direction") && n.contains("requires")),
            "expected a 'direction' required-attribute note; got {:?}",
            out.notes
        );
    }

    #[test]
    fn simulate_mapping_returns_no_emission_for_suppressed_hook_rule() {
        use crate::mappings::{HarnessMapping, MappingSource};
        let mut state = crate::mappings::default_state();
        state.harnesses.retain(|h| h.harness_id != crate::harness::HarnessId::CursorIde);
        state.harnesses.push(HarnessMapping {
            harness_id: crate::harness::HarnessId::CursorIde,
            enabled: true,
            sources: vec![MappingSource::HookRule {
                when: "beforeSubmitPrompt".into(),
                emit: None,
            }],
            cost_overrides: std::collections::BTreeMap::new(),
        });
        let out = simulate_mapping(SimulateMappingInput {
            mapping_state: state,
            harness_id: crate::harness::HarnessId::CursorIde,
            source_index: 0,
            sample_attributes: std::collections::BTreeMap::new(),
            sample_value: None,
        })
        .unwrap();
        assert!(out.emitted.is_none());
        assert!(out
            .notes
            .iter()
            .any(|n| n.contains("suppress")));
    }

    #[test]
    fn simulate_mapping_rejects_inconsistent_draft_state() {
        // Draft state with an unknown metric id should be rejected up
        // front — same invariant the apply path enforces.
        let state = state_with_one_synth_rule("not_in_catalog");
        let err = simulate_mapping(SimulateMappingInput {
            mapping_state: state,
            harness_id: crate::harness::HarnessId::ClaudeCode,
            source_index: 0,
            sample_attributes: std::collections::BTreeMap::new(),
            sample_value: None,
        })
        .unwrap_err();
        match err {
            IpcError::Internal { reason } => assert!(reason.contains("invalid")),
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[test]
    fn simulate_mapping_rejects_out_of_range_source_index() {
        let state = state_with_one_synth_rule("events");
        let err = simulate_mapping(SimulateMappingInput {
            mapping_state: state,
            harness_id: crate::harness::HarnessId::ClaudeCode,
            source_index: 99,
            sample_attributes: std::collections::BTreeMap::new(),
            sample_value: None,
        })
        .unwrap_err();
        match err {
            IpcError::Internal { reason } => assert!(reason.contains("out of range")),
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[test]
    fn list_detected_harnesses_returns_a_row_per_supported_harness() {
        // Calls into the real environment — every supported harness
        // returns a row even when nothing is installed (`detected:
        // false`). The detect module's hermetic tests already cover
        // scoping; this test asserts the IPC entry point doesn't drop
        // or reorder rows. Sprint 9 PR 1 adds the Tier 3 trio. Uses
        // the `_inner` variant so we can exercise the AppHandle-less
        // path with a tempdir-scoped state.
        let dir = tempfile::tempdir().unwrap();
        let result = list_detected_harnesses_inner(dir.path(), &HashMap::new()).unwrap();
        assert_eq!(result.len(), HarnessId::all().len());
    }

    #[test]
    fn list_detected_harnesses_inner_overlays_watcher_only_state_for_enabled_cline() {
        // PR 2 — when state.json records cline as enabled, the
        // dashboard row reports trove_region_present = true and
        // telemetry = On, even though the detector itself can't see
        // those signals (cline has no host file region).
        use crate::adapters::TrovePatch;
        use crate::app_state::{AppState, HarnessConfig, upsert_harness_in};

        let dir = tempfile::tempdir().unwrap();
        let entry = HarnessConfig {
            id: HarnessId::Cline,
            enabled: true,
            config_path: "/home/dev/.config/Code/User/globalStorage/saoudrizwan.claude-dev"
                .to_string(),
            last_patched_at: "2026-05-09T00:00:00Z".to_string(),
            trove_patch: TrovePatch {
                managed_block_hash: "a".repeat(64),
                file_hash_at_last_write: String::new(),
                format: crate::safety::sentinels::Format::Json,
                last_written_region_payload: r#"{"harness":"cline"}"#.to_string(),
            },
            options: ApplyOptions::default(),
        };
        upsert_harness_in(dir.path(), entry).unwrap();

        let rows = list_detected_harnesses_inner(dir.path(), &HashMap::new()).unwrap();
        let cline_row = rows.iter().find(|r| r.id == HarnessId::Cline).unwrap();
        assert!(cline_row.trove_region_present, "cline should overlay enabled");
        assert_eq!(cline_row.telemetry, TelemetryStatus::On);

        // Other Tier 3 rows are unaffected (state.json has no entry).
        let aider_row = rows.iter().find(|r| r.id == HarnessId::Aider).unwrap();
        assert!(!aider_row.trove_region_present);

        let _ = AppState::default(); // keep import live
    }

    #[test]
    fn list_detected_harnesses_inner_overlays_watcher_only_state_for_enabled_claude_desktop() {
        // Claude Desktop has no host file to patch — its adapter is an
        // audit-log tap. Detection always returns trove_region_present
        // = false because read_trove_region_present scans the host file.
        // When state.json records it as enabled, the row must report
        // trove_region_present = true so the UI button reads "Disable".
        use crate::adapters::TrovePatch;
        use crate::app_state::{HarnessConfig, upsert_harness_in};

        let dir = tempfile::tempdir().unwrap();
        // Use the tempdir itself as the sessions root so the v11 migration's
        // Path::exists() check preserves this entry (a non-existent path would
        // be treated as a phantom entry and cleaned up).
        let sessions_root = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions_root).unwrap();
        let entry = HarnessConfig {
            id: HarnessId::ClaudeDesktop,
            enabled: true,
            config_path: sessions_root.to_string_lossy().into_owned(),
            last_patched_at: "2026-05-17T00:00:00Z".to_string(),
            trove_patch: TrovePatch {
                managed_block_hash: "c".repeat(64),
                file_hash_at_last_write: String::new(),
                format: crate::safety::sentinels::Format::Json,
                last_written_region_payload: r#"{"harness":"claude-desktop"}"#.to_string(),
            },
            options: ApplyOptions::default(),
        };
        upsert_harness_in(dir.path(), entry).unwrap();

        let rows = list_detected_harnesses_inner(dir.path(), &HashMap::new()).unwrap();
        let cd_row = rows
            .iter()
            .find(|r| r.id == HarnessId::ClaudeDesktop)
            .expect("claude-desktop row missing from detection");
        assert!(
            cd_row.trove_region_present,
            "claude-desktop should overlay enabled — UI button stays 'Enable' otherwise"
        );
        assert_eq!(cd_row.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn observed_telemetry_overlay_flips_claude_desktop_pill_to_on() {
        // Claude Desktop's local detection always reports
        // TelemetryStatus::Unknown (admin-managed). When the diag
        // filter pipeline has observed at least one log record carrying
        // its service.name, the overlay forces the row to On.
        let dir = tempfile::tempdir().unwrap();
        let mut observed = HashMap::new();
        observed.insert(HarnessId::ClaudeDesktop, 7u64);
        let rows = list_detected_harnesses_inner(dir.path(), &observed).unwrap();
        let cd = rows
            .iter()
            .find(|r| r.id == HarnessId::ClaudeDesktop)
            .expect("claude-desktop row missing from detection");
        assert_eq!(cd.telemetry, TelemetryStatus::On);
    }

    #[test]
    fn observed_telemetry_overlay_is_a_noop_at_zero_or_absent() {
        let dir = tempfile::tempdir().unwrap();
        let mut observed = HashMap::new();
        // Zero count = no observed records = no overlay.
        observed.insert(HarnessId::ClaudeDesktop, 0u64);
        let rows = list_detected_harnesses_inner(dir.path(), &observed).unwrap();
        let cd = rows
            .iter()
            .find(|r| r.id == HarnessId::ClaudeDesktop)
            .unwrap();
        assert_eq!(cd.telemetry, TelemetryStatus::Unknown);
    }

    #[test]
    fn persisted_observations_keep_pill_on_even_when_live_counter_is_empty() {
        // The sticky overlay: once a harness has emitted telemetry in
        // *any* prior session, the persisted state.json carries that
        // forward and the pill stays green from then on — even if the
        // live diag-log counter is empty (e.g. Trove just restarted,
        // or the harness has been idle for hours).
        use crate::app_state::record_telemetry_observed_in;
        let dir = tempfile::tempdir().unwrap();
        record_telemetry_observed_in(dir.path(), [HarnessId::ClaudeDesktop], 1_715_000_000)
            .unwrap();

        let rows = list_detected_harnesses_inner(dir.path(), &HashMap::new()).unwrap();
        let cd = rows
            .iter()
            .find(|r| r.id == HarnessId::ClaudeDesktop)
            .unwrap();
        assert_eq!(
            cd.telemetry,
            TelemetryStatus::On,
            "persisted observation must flip pill to On regardless of live counter",
        );
    }

    // Note: a negative test "persisted observation does not flip other
    // harnesses to On" is intentionally absent here — `detect_all()`
    // reads the host's real config, so any unrecorded harness might
    // legitimately be `On` on a dev machine where the user has it
    // configured. The positive test above plus the BTreeMap-keyed
    // overlay code (`if state.telemetry_observed.contains_key(&row.id)`)
    // make the per-id specificity self-evident.

    #[test]
    fn preview_patch_inner_routes_cline_through_adapter_without_resolver() {
        // PR 2 wires Cline: preview returns Fresh with no host file
        // patch. The resource resolver must not be invoked.
        let home = tempfile::tempdir().unwrap();
        let preview = preview_patch_inner(
            HarnessId::Cline,
            &ApplyOptions::default(),
            home.path(),
            |_| panic!("resource resolver must not be invoked for cline"),
        )
        .unwrap();
        assert!(matches!(preview.status, PreviewStatus::Fresh));
        assert!(preview.after.contains("Cline"));
    }

    #[test]
    fn preview_patch_inner_routes_opencode_through_adapter_without_resolver() {
        // OpenCode is now wired in Sprint 7 PR 2 with a standard SPEC; the
        // resource resolver must not be invoked for it.
        let home = tempfile::tempdir().unwrap();
        let result = preview_patch_inner(
            HarnessId::Opencode,
            &ApplyOptions::default(),
            home.path(),
            |_| panic!("resource resolver must not be invoked for opencode"),
        )
        .unwrap();
        assert!(
            result.after.contains("@devtheops/opencode-plugin-otel"),
            "preview should embed the OTel plugin package name"
        );
    }

    #[test]
    fn preview_patch_inner_routes_cursor_ide_through_resolver_and_adapter() {
        let home = tempfile::tempdir().unwrap();
        let fake_hook = std::path::PathBuf::from("/opt/trove/cursor-otel-hook.cjs");
        let resolver_called = std::cell::Cell::new(false);
        let result = preview_patch_inner(
            HarnessId::CursorIde,
            &ApplyOptions::default(),
            home.path(),
            |id| {
                assert_eq!(id, HarnessId::CursorIde);
                resolver_called.set(true);
                Ok(fake_hook.clone())
            },
        )
        .unwrap();
        assert!(resolver_called.get(), "resolver must be called for cursor-ide");
        assert!(result.after.contains("/opt/trove/cursor-otel-hook.cjs"));
    }

    #[test]
    fn preview_patch_inner_routes_cursor_cli_through_resolver_and_adapter() {
        // After the cursor-cli wrapper rewrite, cursor-cli's preview no
        // longer renders the hooks.json patch — it renders a shell-rc
        // function block referencing the bundled wrapper script.
        let home = tempfile::tempdir().unwrap();
        // wrapper_common refuses to render a preview when no shell rc
        // file exists, so seed one.
        std::fs::write(home.path().join(".zshrc"), "# user content\n").unwrap();
        let fake_wrapper = std::path::PathBuf::from("/opt/trove/wrappers/trove-cursor-agent");
        let result = preview_patch_inner(
            HarnessId::CursorCli,
            &ApplyOptions::default(),
            home.path(),
            |_| Ok(fake_wrapper.clone()),
        )
        .unwrap();
        assert!(result.after.contains("/opt/trove/wrappers/trove-cursor-agent"));
    }

    #[test]
    fn preview_patch_inner_propagates_resource_resolver_errors() {
        let home = std::path::PathBuf::from("/tmp/should-not-be-touched");
        let err = preview_patch_inner(
            HarnessId::CursorIde,
            &ApplyOptions::default(),
            &home,
            |_| {
                Err(IpcError::Internal {
                    reason: "could not resolve resource_dir".to_string(),
                })
            },
        )
        .unwrap_err();
        assert!(matches!(err, IpcError::Internal { .. }));
    }

    #[test]
    fn preview_patch_inner_routes_aider_through_resolver_and_adapter() {
        // Sprint 9 PR 3 — Aider preview drives wrapper_common against
        // the user's primary shell rc.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".zshrc"), "user content\n").unwrap();
        let fake_wrapper = std::path::PathBuf::from("/opt/trove/wrappers/trove-aider");
        let result = preview_patch_inner(
            HarnessId::Aider,
            &ApplyOptions::default(),
            dir.path(),
            |id| {
                assert_eq!(id, HarnessId::Aider);
                Ok(fake_wrapper.clone())
            },
        )
        .unwrap();
        assert!(matches!(result.status, PreviewStatus::Fresh));
        assert!(result.after.contains("aider() {"));
        assert!(result.after.contains("/opt/trove/wrappers/trove-aider"));
    }

    #[test]
    fn preview_patch_inner_routes_copilot_cli_through_resolver_and_adapter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".zshrc"), "").unwrap();
        let fake_wrapper = std::path::PathBuf::from("/opt/trove/wrappers/trove-copilot");
        let result = preview_patch_inner(
            HarnessId::CopilotCli,
            &ApplyOptions::default(),
            dir.path(),
            |id| {
                assert_eq!(id, HarnessId::CopilotCli);
                Ok(fake_wrapper.clone())
            },
        )
        .unwrap();
        assert!(result.after.contains("gh-copilot() {"));
        assert!(result.after.contains("/opt/trove/wrappers/trove-copilot"));
    }

    // apply_patch / revert_patch / save_backend / clear_backend / get_app_state
    // require a Tauri AppHandle and are exercised by the integration tests
    // under `tests/app_state_*.rs` against a real temp HOME and config dir.
}
