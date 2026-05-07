//! Real-keychain round-trip for [`trove_app::secrets`].
//!
//! Gated with `#[ignore]` because the OS keychain isn't available in
//! Trove's headless Ubuntu CI runners (no D-Bus, no Secret Service).
//! Run locally with:
//!
//! ```sh
//! cargo test -p trove --test secrets_keyring -- --ignored
//! ```
//!
//! The test names every entry under a fresh randomised account so
//! re-runs don't collide and a failed run leaves no residue beyond a
//! single keychain row labelled `trove`.

use trove_app::secrets;
use zeroize::Zeroizing;

fn unique_account() -> String {
    // Each test run gets its own account so a half-run leaves nothing
    // behind that the next run might mistake for stale state. PID alone
    // would collide across processes; nanoseconds-since-epoch makes
    // accidental reuse vanishingly unlikely.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("test.pr1.{}.{}", std::process::id(), nanos)
}

#[test]
#[ignore = "requires OS keychain access; run with --ignored locally"]
fn store_then_retrieve_returns_the_same_value() {
    let account = unique_account();
    let secret = Zeroizing::new("ok-secret-test-CAN4RY".to_string());

    secrets::store(&account, &secret).expect("store should succeed on a working keychain");
    let revived = secrets::retrieve(&account).expect("retrieve should find what we just stored");
    assert_eq!(revived.as_str(), "ok-secret-test-CAN4RY");

    secrets::delete(&account).expect("delete after assertion to leave the keychain clean");
}

#[test]
#[ignore = "requires OS keychain access; run with --ignored locally"]
fn delete_is_idempotent_for_missing_account() {
    let account = unique_account();
    // Nothing stored under this account; delete should still succeed
    // because secrets::delete swallows keyring::Error::NoEntry.
    secrets::delete(&account).expect("delete on a missing entry should be a no-op");
}

#[test]
#[ignore = "requires OS keychain access; run with --ignored locally"]
fn store_overwrites_existing_value() {
    let account = unique_account();
    secrets::store(&account, &Zeroizing::new("first".into())).unwrap();
    secrets::store(&account, &Zeroizing::new("second".into())).unwrap();
    let revived = secrets::retrieve(&account).unwrap();
    assert_eq!(revived.as_str(), "second");
    secrets::delete(&account).unwrap();
}

#[test]
#[ignore = "requires OS keychain access; run with --ignored locally"]
fn retrieve_after_delete_errors() {
    let account = unique_account();
    secrets::store(&account, &Zeroizing::new("ephemeral".into())).unwrap();
    secrets::delete(&account).unwrap();
    let err = secrets::retrieve(&account).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("keychain"),
        "expected keychain error after delete, got: {msg}"
    );
}
