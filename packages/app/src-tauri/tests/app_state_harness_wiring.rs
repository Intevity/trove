//! Integration test for the apply/revert state-wiring contract:
//! `apply_patch` upserts a `HarnessConfig` into `state.json`, and
//! `revert_patch` removes it.
//!
//! The Tauri command takes an `AppHandle` we can't easily synthesise
//! from an integration test, so the test calls the same per-step pieces
//! the command does (adapter `apply`, `harness_config_from_apply`,
//! `upsert_harness_in` / `remove_harness_in`) against a tempdir. That
//! covers the exact code paths the command executes — only the Tauri
//! dispatch and the `AppHandle::path()` lookup live above this layer,
//! and both are exercised by the dev `pnpm tauri:dev` flow.

use std::path::Path;

use tempfile::tempdir;

use trove_app::adapters::{ApplyOptions, claude_code, codex_cli, gemini_cli, qwen_code};
use trove_app::app_state::{
    AppState, harness_config_from_apply, load_from_dir, remove_harness_in, upsert_harness_in,
};
use trove_app::harness::HarnessId;

/// Stand-in for `apply_patch` + the post-apply state.json upsert. This
/// is exactly the sequence `ipc::commands::apply_patch` runs once the
/// Tauri layer has resolved `app: AppHandle`.
fn apply_then_persist(
    id: HarnessId,
    home: &Path,
    config_dir: &Path,
    options: ApplyOptions,
) -> trove_app::adapters::TrovePatch {
    let (patch, config_path) = match id {
        HarnessId::ClaudeCode => (
            claude_code::apply(home, &options).unwrap(),
            claude_code::config_path(home),
        ),
        HarnessId::CodexCli => (
            codex_cli::apply(home, &options).unwrap(),
            codex_cli::config_path(home),
        ),
        HarnessId::GeminiCli => (
            gemini_cli::apply(home, &options).unwrap(),
            gemini_cli::config_path(home),
        ),
        HarnessId::QwenCode => (
            qwen_code::apply(home, &options).unwrap(),
            qwen_code::config_path(home),
        ),
        other => panic!("Tier 2/3 harness {other:?} should not reach apply_then_persist"),
    };
    let entry = harness_config_from_apply(id, &config_path, options, patch.clone());
    upsert_harness_in(config_dir, entry).unwrap();
    patch
}

fn revert_then_unpersist(id: HarnessId, home: &Path, config_dir: &Path) {
    match id {
        HarnessId::ClaudeCode => claude_code::revert(home).unwrap(),
        HarnessId::CodexCli => codex_cli::revert(home).unwrap(),
        HarnessId::GeminiCli => gemini_cli::revert(home).unwrap(),
        HarnessId::QwenCode => qwen_code::revert(home).unwrap(),
        other => panic!("Tier 2/3 harness {other:?} should not reach revert_then_unpersist"),
    }
    remove_harness_in(config_dir, id).unwrap();
}

#[test]
fn first_apply_lands_a_harness_config_in_state_json() {
    let home = tempdir().unwrap();
    let cfg = tempdir().unwrap();

    let patch = apply_then_persist(
        HarnessId::ClaudeCode,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );

    let state: AppState = load_from_dir(cfg.path()).unwrap();
    assert_eq!(state.harnesses.len(), 1);
    let entry = &state.harnesses[0];
    assert_eq!(entry.id, HarnessId::ClaudeCode);
    assert!(entry.enabled);
    assert!(
        entry
            .config_path
            .ends_with(".claude/settings.json"),
        "config_path was {}",
        entry.config_path,
    );
    assert_eq!(entry.trove_patch.managed_block_hash.len(), 64);
    assert_eq!(entry.trove_patch, patch);
    // RFC3339 strings start with the year; loose check that we wrote
    // *some* timestamp rather than an empty string.
    assert!(!entry.last_patched_at.is_empty());
    assert!(entry.last_patched_at.contains('T'));
}

#[test]
fn revert_removes_the_state_json_entry() {
    let home = tempdir().unwrap();
    let cfg = tempdir().unwrap();

    apply_then_persist(
        HarnessId::ClaudeCode,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );
    assert_eq!(load_from_dir(cfg.path()).unwrap().harnesses.len(), 1);

    revert_then_unpersist(HarnessId::ClaudeCode, home.path(), cfg.path());
    let state = load_from_dir(cfg.path()).unwrap();
    assert!(state.harnesses.is_empty());
}

#[test]
fn double_apply_is_idempotent_except_last_patched_at() {
    let home = tempdir().unwrap();
    let cfg = tempdir().unwrap();

    let first = apply_then_persist(
        HarnessId::ClaudeCode,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );
    let after_first = load_from_dir(cfg.path()).unwrap();
    let first_entry = &after_first.harnesses[0];
    let first_ts = first_entry.last_patched_at.clone();

    // Sleep one millisecond to guarantee the RFC3339 timestamp moves
    // (chrono::Utc::now uses microsecond resolution on macOS but the
    // serialized string is millisecond-rounded on some platforms).
    std::thread::sleep(std::time::Duration::from_millis(2));

    let second = apply_then_persist(
        HarnessId::ClaudeCode,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );
    let after_second = load_from_dir(cfg.path()).unwrap();
    assert_eq!(after_second.harnesses.len(), 1, "should still be exactly one entry");
    let second_entry = &after_second.harnesses[0];

    assert_eq!(first.managed_block_hash, second.managed_block_hash);
    assert_eq!(
        first_entry.trove_patch.managed_block_hash,
        second_entry.trove_patch.managed_block_hash,
    );
    assert_ne!(
        first_ts, second_entry.last_patched_at,
        "lastPatchedAt should advance between applies",
    );
}

#[test]
fn separate_harnesses_get_separate_entries() {
    let home = tempdir().unwrap();
    let cfg = tempdir().unwrap();

    apply_then_persist(
        HarnessId::ClaudeCode,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );
    apply_then_persist(
        HarnessId::GeminiCli,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );
    apply_then_persist(
        HarnessId::CodexCli,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );
    apply_then_persist(
        HarnessId::QwenCode,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );

    let state = load_from_dir(cfg.path()).unwrap();
    assert_eq!(state.harnesses.len(), 4);
    let ids: Vec<HarnessId> = state.harnesses.iter().map(|h| h.id).collect();
    for id in HarnessId::tier_1() {
        assert!(ids.contains(id), "missing entry for {id:?}");
    }
}

#[test]
fn revert_only_drops_its_own_id() {
    let home = tempdir().unwrap();
    let cfg = tempdir().unwrap();

    apply_then_persist(
        HarnessId::ClaudeCode,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );
    apply_then_persist(
        HarnessId::GeminiCli,
        home.path(),
        cfg.path(),
        ApplyOptions::default(),
    );

    revert_then_unpersist(HarnessId::ClaudeCode, home.path(), cfg.path());

    let state = load_from_dir(cfg.path()).unwrap();
    assert_eq!(state.harnesses.len(), 1);
    assert_eq!(state.harnesses[0].id, HarnessId::GeminiCli);
}

#[test]
fn options_round_trip_through_state_json() {
    let home = tempdir().unwrap();
    let cfg = tempdir().unwrap();

    let mut options = ApplyOptions {
        log_user_prompts: true,
        ..Default::default()
    };
    options
        .custom_attributes
        .insert("team".into(), "platform".into());
    options.custom_attributes.insert("env".into(), "prod".into());

    apply_then_persist(HarnessId::GeminiCli, home.path(), cfg.path(), options.clone());

    let state = load_from_dir(cfg.path()).unwrap();
    let entry = &state.harnesses[0];
    assert_eq!(entry.options, options);
    assert!(entry.options.log_user_prompts);
    assert_eq!(entry.options.custom_attributes.len(), 2);
}
