//! Round-trip for [`trove_app::secrets`].
//!
//! Sprint 13 swapped the keychain-backed implementation for a file-based
//! store under `<app_config_dir>/secrets.json` (see `secrets/mod.rs`).
//! These tests no longer need to be `#[ignore]`d because they hit a
//! tmpdir instead of the OS keychain. The file name keeps the original
//! `secrets_keyring` for git-history continuity; the contents exercise
//! the file path.

use tempfile::TempDir;
use trove_app::secrets;
use zeroize::Zeroizing;

fn unique_account() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("test.pr1.{}.{}", std::process::id(), nanos)
}

#[test]
fn store_then_retrieve_returns_the_same_value() {
    let dir = TempDir::new().unwrap();
    let account = unique_account();
    let secret = Zeroizing::new("ok-secret-test-CAN4RY".to_string());

    secrets::store_in(dir.path(), &account, &secret).expect("store should succeed");
    let revived =
        secrets::retrieve_in(dir.path(), &account).expect("retrieve should find what we stored");
    assert_eq!(revived.as_str(), "ok-secret-test-CAN4RY");

    secrets::delete_in(dir.path(), &account).expect("delete after assertion");
}

#[test]
fn delete_is_idempotent_for_missing_account() {
    let dir = TempDir::new().unwrap();
    let account = unique_account();
    // Nothing stored under this account; delete is a no-op.
    secrets::delete_in(dir.path(), &account).expect("delete on missing entry is a no-op");
}

#[test]
fn store_overwrites_existing_value() {
    let dir = TempDir::new().unwrap();
    let account = unique_account();
    secrets::store_in(dir.path(), &account, &Zeroizing::new("first".into())).unwrap();
    secrets::store_in(dir.path(), &account, &Zeroizing::new("second".into())).unwrap();
    let revived = secrets::retrieve_in(dir.path(), &account).unwrap();
    assert_eq!(revived.as_str(), "second");
    secrets::delete_in(dir.path(), &account).unwrap();
}

#[test]
fn retrieve_after_delete_errors() {
    let dir = TempDir::new().unwrap();
    let account = unique_account();
    secrets::store_in(dir.path(), &account, &Zeroizing::new("ephemeral".into())).unwrap();
    secrets::delete_in(dir.path(), &account).unwrap();
    let err = secrets::retrieve_in(dir.path(), &account).unwrap_err();
    let msg = err.to_string();
    // After delete the file no longer has the entry; the migration
    // fallback's keychain probe is also a miss, so we land on
    // `NotFound`. On CI without keychain access the keychain backend
    // itself errors — both variants are acceptable here.
    assert!(
        msg.contains("no secret stored") || msg.contains("keychain"),
        "expected not-found or keychain error after delete, got: {msg}"
    );
}
