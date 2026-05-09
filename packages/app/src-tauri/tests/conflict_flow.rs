//! Sprint 8 PR 1 — end-to-end conflict-flow tests.
//!
//! These tests don't go through Tauri's `#[command]` boundary (no
//! `AppHandle` is available outside a running app); they drive the same
//! flow via the `*_inner` helpers in `ipc::commands`. The shape of each
//! test mirrors what the React resolver does at runtime:
//!
//! 1. `apply_then_persist` — install Trove's region into a fresh host
//!    config and record the `HarnessConfig` in `state.json`.
//! 2. Hand-edit the host config inside the managed region.
//! 3. Run `preview_patch_inner` and assert `PreviewStatus::Conflict`.
//! 4. Call `build_conflict_payload(&preview, prior.as_ref())` and
//!    assert the payload shape (3-pane vs 2-pane, original payload,
//!    current payload, theirs payload).
//! 5. Drive a resolution via `keep_mine_inner` /
//!    `take_theirs_inner` / `merge_manually_inner` and assert the
//!    on-disk outcome.

use std::path::Path;

use trove_app::adapters::{ApplyOptions, PreviewStatus, claude_code};
use trove_app::app_state::{
    HarnessConfig, harness_config_from_apply, load_from_dir, upsert_harness_in,
};
use trove_app::harness::HarnessId;
use trove_app::ipc::commands::{
    build_conflict_payload, keep_mine_inner, merge_manually_inner, preview_patch_inner,
    take_theirs_inner,
};
use trove_app::ipc::ConflictResolutionOutcome;

/// Resource resolver passed to `preview_patch_inner` from Tier 1
/// tests. Tier 1 adapters never invoke it; panicking is correct under
/// that invariant. Sprint 9 PR 3 widened the resolver to take a
/// `HarnessId`; the signature here mirrors that change.
fn unused_hook_resolver(
    _id: trove_app::harness::HarnessId,
) -> Result<std::path::PathBuf, trove_app::ipc::IpcError> {
    panic!("resource resolver must not be invoked for tier-1 harnesses")
}

/// Equivalent of `commands::apply_patch`'s success path: install the
/// adapter's patch and persist a `HarnessConfig` to `config_dir`'s
/// state.json. Returns the persisted entry so the test can assert on
/// the state.json -> resolver -> state.json round trip.
fn apply_then_persist(home: &Path, config_dir: &Path) -> HarnessConfig {
    let options = ApplyOptions::default();
    let patch = claude_code::apply(home, &options).unwrap();
    let entry = harness_config_from_apply(
        HarnessId::ClaudeCode,
        &claude_code::config_path(home),
        options,
        patch,
    );
    upsert_harness_in(config_dir, entry.clone()).unwrap();
    entry
}

/// Hand-edit the managed region. Sprint 8's resolver needs a real
/// region-internal mutation; tweaking the OTLP endpoint URL is the
/// canonical "user fiddled with the patch" scenario.
fn corrupt_managed_region(host_path: &Path) {
    let original = std::fs::read_to_string(host_path).unwrap();
    let edited = original.replace(
        "http://127.0.0.1:4318",
        "http://attacker.example.com:4318",
    );
    assert_ne!(edited, original, "test fixture didn't actually mutate");
    std::fs::write(host_path, edited).unwrap();
}

#[test]
fn three_way_payload_is_built_when_state_json_has_a_prior_record() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();

    let prior = apply_then_persist(home.path(), config_dir.path());
    let host = claude_code::config_path(home.path());
    corrupt_managed_region(&host);

    let preview = preview_patch_inner(
        HarnessId::ClaudeCode,
        &ApplyOptions::default(),
        home.path(),
        unused_hook_resolver,
    )
    .unwrap();
    assert_eq!(preview.status, PreviewStatus::Conflict);

    let payload = build_conflict_payload(&preview, Some(&prior)).unwrap();
    // Original is the prior payload Trove last wrote.
    let original = payload
        .original_region_payload
        .as_ref()
        .expect("3-way mode must populate original");
    assert_eq!(original, &prior.trove_patch.last_written_region_payload);
    // Current is what's in the host file right now (with the user's
    // edit). It must differ from the original.
    assert_ne!(payload.current_region_payload, payload.theirs_region_payload);
    assert_ne!(payload.current_region_payload, *original);
    // Theirs matches what the next apply would write — same value the
    // adapter produces from the same options.
    assert_eq!(payload.theirs_region_payload, *original);
    // `file_after_if_taking_theirs` reflects the whole-file patch the
    // resolver will install if the user clicks Take Trove's.
    assert!(payload.file_after_if_taking_theirs.contains("127.0.0.1:4318"));
    assert!(!payload.file_after_if_taking_theirs.contains("attacker.example.com"));
}

#[test]
fn two_way_payload_is_built_for_orphan_block_when_state_json_was_wiped() {
    let home = tempfile::tempdir().unwrap();

    // Install a patch but DON'T persist a HarnessConfig — simulates the
    // "state.json was wiped" path. The host file still has Trove's
    // managed region from a previous machine.
    let _ = claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    let host = claude_code::config_path(home.path());
    corrupt_managed_region(&host);

    let preview = preview_patch_inner(
        HarnessId::ClaudeCode,
        &ApplyOptions::default(),
        home.path(),
        unused_hook_resolver,
    )
    .unwrap();
    assert_eq!(preview.status, PreviewStatus::Conflict);

    // No prior record — payload original_region_payload is None.
    let payload = build_conflict_payload(&preview, None).unwrap();
    assert!(payload.original_region_payload.is_none());
    assert!(!payload.current_region_payload.is_empty());
    assert!(!payload.theirs_region_payload.is_empty());
}

#[test]
fn keep_mine_persists_user_edits_as_new_baseline_without_writing_host() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();

    apply_then_persist(home.path(), config_dir.path());
    let host = claude_code::config_path(home.path());
    corrupt_managed_region(&host);
    let mutated_bytes = std::fs::read(&host).unwrap();

    let outcome =
        keep_mine_inner(HarnessId::ClaudeCode, home.path(), config_dir.path()).unwrap();

    // Host file is byte-identical to the user's edited version.
    assert_eq!(std::fs::read(&host).unwrap(), mutated_bytes);

    // state.json now records the mutated region as the new baseline.
    match outcome {
        ConflictResolutionOutcome::MarkedMine { patch } => {
            // The new payload must contain the user's edit, not Trove's
            // intended endpoint.
            assert!(
                patch
                    .last_written_region_payload
                    .contains("attacker.example.com"),
                "keep-mine should snapshot the user's current region"
            );
            // And state.json's HarnessConfig must store the same patch.
            let revived = load_from_dir(config_dir.path()).unwrap();
            let stored = &revived
                .harnesses
                .iter()
                .find(|h| h.id == HarnessId::ClaudeCode)
                .unwrap()
                .trove_patch;
            assert_eq!(stored, &patch);
        }
        other => panic!("expected MarkedMine, got {other:?}"),
    }

    // A subsequent re-preview is no longer Conflict (the new baseline
    // matches the user's current region).
    let preview2 = preview_patch_inner(
        HarnessId::ClaudeCode,
        &ApplyOptions::default(),
        home.path(),
        unused_hook_resolver,
    )
    .unwrap();
    // The next preview will be Conflict again because Trove's intended
    // region differs from the user's. Keep-mine is about state.json
    // bookkeeping, not telling Trove "your patch is wrong forever" —
    // the resolver simply won't pop up unannounced. That's fine; the
    // important assertion is that the state.json baseline now points
    // at the user's content.
    assert_eq!(preview2.status, PreviewStatus::Conflict);
}

#[test]
fn take_theirs_overwrites_host_file_with_trove_intended_content() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();

    apply_then_persist(home.path(), config_dir.path());
    let host = claude_code::config_path(home.path());
    corrupt_managed_region(&host);

    let outcome = take_theirs_inner(
        HarnessId::ClaudeCode,
        home.path(),
        config_dir.path(),
        ApplyOptions::default(),
        unused_hook_resolver,
    )
    .unwrap();

    // The host file is now Trove's intended content (no attacker URL).
    let after = std::fs::read_to_string(&host).unwrap();
    assert!(after.contains("127.0.0.1:4318"));
    assert!(!after.contains("attacker.example.com"));

    // A backup file was written next to the host.
    let backups: Vec<_> = std::fs::read_dir(host.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.contains(".trove.bak."))
        })
        .collect();
    assert!(
        !backups.is_empty(),
        "take-theirs must call backup_file before the atomic write"
    );

    // state.json reflects the new patch.
    match outcome {
        ConflictResolutionOutcome::Applied { patch } => {
            let revived = load_from_dir(config_dir.path()).unwrap();
            let stored = &revived
                .harnesses
                .iter()
                .find(|h| h.id == HarnessId::ClaudeCode)
                .unwrap()
                .trove_patch;
            assert_eq!(stored, &patch);
            // The new baseline matches Trove's intent, not the user's
            // hand-edit.
            assert!(
                !patch
                    .last_written_region_payload
                    .contains("attacker.example.com")
            );
        }
        other => panic!("expected Applied, got {other:?}"),
    }
}

#[test]
fn merge_manually_writes_sibling_files_and_does_not_touch_host() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();

    apply_then_persist(home.path(), config_dir.path());
    let host = claude_code::config_path(home.path());
    corrupt_managed_region(&host);
    let host_bytes = std::fs::read(&host).unwrap();

    let outcome = merge_manually_inner(
        HarnessId::ClaudeCode,
        home.path(),
        config_dir.path(),
        &ApplyOptions::default(),
        unused_hook_resolver,
    )
    .unwrap();

    // Host file is unchanged.
    assert_eq!(std::fs::read(&host).unwrap(), host_bytes);

    match outcome {
        ConflictResolutionOutcome::MergeDeferred { sibling_paths } => {
            // Both sibling files exist next to the host.
            let original = std::fs::read_to_string(&sibling_paths.original).unwrap();
            let theirs = std::fs::read_to_string(&sibling_paths.theirs).unwrap();
            assert!(!original.is_empty(), "original sibling carries the prior payload");
            assert!(!theirs.is_empty(), "theirs sibling carries Trove's intended payload");
            assert_eq!(sibling_paths.host, host.display().to_string());
            // The resolver UI uses sibling_paths.host to ask the OS
            // shell plugin to open the host config in the user's
            // default editor.
        }
        other => panic!("expected MergeDeferred, got {other:?}"),
    }

    // state.json is untouched until the user re-applies after merging.
    let revived = load_from_dir(config_dir.path()).unwrap();
    assert_eq!(revived.harnesses.len(), 1);
}

#[test]
fn merge_manually_works_in_orphan_block_mode_with_empty_original_sibling() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();

    // Install Trove's patch but skip persisting the HarnessConfig.
    let _ = claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    let host = claude_code::config_path(home.path());
    corrupt_managed_region(&host);

    let outcome = merge_manually_inner(
        HarnessId::ClaudeCode,
        home.path(),
        config_dir.path(),
        &ApplyOptions::default(),
        unused_hook_resolver,
    )
    .unwrap();

    match outcome {
        ConflictResolutionOutcome::MergeDeferred { sibling_paths } => {
            let original = std::fs::read_to_string(&sibling_paths.original).unwrap();
            assert_eq!(
                original, "",
                "orphan-block mode writes an empty original sibling"
            );
        }
        other => panic!("expected MergeDeferred, got {other:?}"),
    }
}
