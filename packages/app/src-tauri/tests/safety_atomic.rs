//! Integration tests for `safety::atomic` and `safety::backup`.
//!
//! Inline `#[cfg(test)]` modules in the source files cover the unit-level
//! behaviour of each function. This file exercises end-to-end flows across
//! both modules — atomic write, then backup, then prune — under conditions
//! representative of how an adapter will use them in Sprint 3+.

use std::fs;

use assert_fs::TempDir;
use pretty_assertions::assert_eq;
use trove_app::safety::{atomic, backup};

#[test]
fn atomic_then_backup_round_trip() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");

    // Adapter sequence: write fresh; back up; rewrite; verify backup
    // captured the prior contents byte-for-byte.
    atomic::write_atomic(&path, b"first").unwrap();
    let first_backup = backup::backup_file(&path).unwrap();
    atomic::write_atomic(&path, b"second").unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    assert_eq!(fs::read_to_string(&first_backup).unwrap(), "first");
}

#[test]
fn prune_after_many_writes_keeps_recent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    atomic::write_atomic(&path, b"v0").unwrap();

    for i in 0..6 {
        let _ = backup::backup_file(&path).unwrap();
        atomic::write_atomic(&path, format!("v{i}").as_bytes()).unwrap();
        // Spread backup timestamps so the lexicographic sort is unambiguous.
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    let removed = backup::prune_backups(&path, 3).unwrap();
    assert_eq!(removed, 3);
    assert_eq!(backup::list_backups(&path).unwrap().len(), 3);
}

#[cfg(unix)]
#[test]
fn atomic_write_preserves_mode_through_backup_cycle() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    atomic::write_atomic(&path, b"private").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    let _ = backup::backup_file(&path).unwrap();
    atomic::write_atomic(&path, b"private2").unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "expected mode preservation; got {mode:o}");
}

#[test]
fn missing_destination_directory_propagates_io_error() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("nope").join("config.json");
    let err = atomic::write_atomic(&path, b"x").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}
