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
    ApplyOptions, PatchPreview, PreviewStatus, TrovePatch, aider, claude_code, cline,
    cline_watcher, codex_cli, copilot_cli, cursor_cli, cursor_ide, gemini_cli, opencode,
    qwen_code,
};
use crate::app_state::{
    self, AppState, Backend, BackendDraft, HarnessConfig, backend_secret_accounts,
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
    list_detected_harnesses_inner(&config_dir)
}

/// Test-friendly variant of [`list_detected_harnesses`]. Operates on
/// an explicit config directory so unit tests can lay down a synthetic
/// `state.json` and assert the Tier 3 overlay.
pub fn list_detected_harnesses_inner(
    config_dir: &Path,
) -> Result<Vec<DetectedHarness>, IpcError> {
    let mut rows = detect_all();
    let state = app_state::load_from_dir(config_dir)?;
    overlay_tier3_state(&mut rows, &state);
    Ok(rows)
}

/// For each Tier 3 row, override `trove_region_present` (and `telemetry`)
/// from the corresponding `state.json` entry. Tier 1 / Tier 2 rows
/// already have authoritative answers from the host-file inspection in
/// `detect/harnesses.rs` and are left unchanged.
fn overlay_tier3_state(rows: &mut [DetectedHarness], state: &AppState) {
    for row in rows.iter_mut() {
        if !HarnessId::tier_3().contains(&row.id) {
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
pub fn apply_patch(
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

/// Persist a new backend chosen by the wizard. Stores each secret in
/// the OS keychain, replaces the raw values in `draft` with [`SecretRef`]
/// handles, writes the resulting [`Backend`] to `state.json`, then
/// regenerates `collector.yaml` and recycles the supervised sidecar so
/// telemetry begins flowing to the new destination.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn save_backend(app: tauri::AppHandle, draft: BackendDraft) -> Result<Backend, IpcError> {
    let (backend, secrets_to_store) = drain_secrets_from_draft(draft);

    for secret in &secrets_to_store {
        secrets::store(&secret.account, &secret.value)?;
    }

    let mut state = app_state::load(&app)?;
    state.backend = Some(backend.clone());
    app_state::save(&app, &state)?;

    let rendered = codegen::render(&backend).map_err(render_error_to_ipc)?;
    let env = unwrap_env(rendered.env);
    let yaml = render_with_overlays(rendered.yaml, &state);
    crate::reload_collector(&app, &yaml, env).map_err(|e| boot_error_to_ipc(&e))?;

    Ok(backend)
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

/// Wipe the active backend: delete every keychain entry it referenced,
/// null out `state.backend`, restore the smoke `collector.yaml`, and
/// recycle the supervisor with no env vars set. Idempotent — a no-op
/// when no backend is currently saved.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn clear_backend(app: tauri::AppHandle) -> Result<(), IpcError> {
    let mut state = app_state::load(&app)?;
    if let Some(backend) = state.backend.take() {
        for account in backend_secret_accounts(&backend) {
            secrets::delete(&account)?;
        }
        app_state::save(&app, &state)?;

        crate::reload_collector(&app, crate::smoke_config_yaml(), HashMap::new())
            .map_err(|e| boot_error_to_ipc(&e))?;
    }
    Ok(())
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
    let Some(backend) = state.backend.as_ref() else {
        return Ok(());
    };
    let rendered = codegen::render(backend).map_err(render_error_to_ipc)?;
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
    crate::mappings::validate(&mappings).map_err(|e| IpcError::Internal {
        reason: format!("invalid mapping state: {e}"),
    })?;
    let mut state = app_state::load(&app)?;
    if state.mappings == mappings {
        // No-op: a noisy UI may call apply on every mount; bail before
        // touching the supervisor.
        return Ok(());
    }
    state.mappings = mappings;
    app_state::save(&app, &state)?;
    let Some(backend) = state.backend.as_ref() else {
        return Ok(());
    };
    let rendered = codegen::render(backend).map_err(render_error_to_ipc)?;
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
        HarnessId::CursorIde | HarnessId::CursorCli => &["resources", "hooks", "cursor-otel-hook.cjs"],
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

/// Spawn the appropriate Tier 3 watcher and register it in the
/// long-lived `TierThreeWatchers` slot. No-op for non-tier-3 ids.
fn spawn_tier3_watcher(
    app: &tauri::AppHandle,
    id: HarnessId,
    home: &Path,
    options: &ApplyOptions,
) {
    use tauri::Manager as _;
    let handle = match id {
        HarnessId::Cline => Some(cline_watcher::spawn(
            cline::tasks_dir(home),
            options.clone(),
            cline_watcher::DEFAULT_POLL_INTERVAL,
        )),
        HarnessId::Aider => {
            let log = aider::log_path(home);
            ensure_log_parent(&log);
            Some(spawn_wrapper_log_watcher(log, options.clone(), id))
        }
        HarnessId::CopilotCli => {
            let log = copilot_cli::log_path(home);
            ensure_log_parent(&log);
            Some(spawn_wrapper_log_watcher(log, options.clone(), id))
        }
        _ => None,
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
) -> crate::log_watcher::WatcherHandle {
    use crate::log_watcher::{DEFAULT_POLL_INTERVAL, spawn as spawn_tail};
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
    let tail = spawn_tail(log_path, tx, DEFAULT_POLL_INTERVAL);
    let join = tokio::spawn(async move {
        // The tail handle moves into the task so cancelling this outer
        // task drops it (and so cancels the inner watcher).
        let _tail = tail;
        while let Some(line) = rx.recv().await {
            let log_payload = match id {
                HarnessId::Aider => aider::parse_event_line(&line, &options),
                HarnessId::CopilotCli => copilot_cli::parse_event_line(&line, &options),
                _ => None,
            };
            let metric_payload = match id {
                HarnessId::Aider => aider::parse_event_metric_payload(&line, &options),
                HarnessId::CopilotCli => {
                    copilot_cli::parse_event_metric_payload(&line, &options)
                }
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
        let result = list_detected_harnesses_inner(dir.path()).unwrap();
        let expected =
            HarnessId::tier_1().len() + HarnessId::tier_2().len() + HarnessId::tier_3().len();
        assert_eq!(result.len(), expected);
    }

    #[test]
    fn list_detected_harnesses_inner_overlays_tier3_state_for_enabled_cline() {
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

        let rows = list_detected_harnesses_inner(dir.path()).unwrap();
        let cline_row = rows.iter().find(|r| r.id == HarnessId::Cline).unwrap();
        assert!(cline_row.trove_region_present, "cline should overlay enabled");
        assert_eq!(cline_row.telemetry, TelemetryStatus::On);

        // Other Tier 3 rows are unaffected (state.json has no entry).
        let aider_row = rows.iter().find(|r| r.id == HarnessId::Aider).unwrap();
        assert!(!aider_row.trove_region_present);

        let _ = AppState::default(); // keep import live
    }

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
        let home = tempfile::tempdir().unwrap();
        let fake_hook = std::path::PathBuf::from("/opt/trove/cursor-otel-hook.cjs");
        let result = preview_patch_inner(
            HarnessId::CursorCli,
            &ApplyOptions::default(),
            home.path(),
            |_| Ok(fake_hook.clone()),
        )
        .unwrap();
        assert!(result.after.contains("/opt/trove/cursor-otel-hook.cjs"));
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
