//! Atomic file writes.
//!
//! `write_atomic` writes bytes to a path through a temp file in the same
//! directory, fsyncs the file, renames into place, then fsyncs the parent
//! directory entry. A crash between any two steps leaves the destination
//! either untouched (old contents intact) or completely replaced (new
//! contents intact) — never half-written. This is the property every
//! adapter relies on.
//!
//! When the destination already exists, its permissions are cloned onto
//! the temp file before the rename so a `0600` settings file stays
//! `0600` after we touch it. Owner and group preservation requires root
//! privilege we don't have on a developer machine, so we deliberately
//! skip that — note this in `SECURITY.md` post-Sprint 2.
//!
//! Generalises the `write_settings` helper in
//! claude-sentinel/packages/app/src-tauri/src/settings_patch.rs:51 by
//! adding directory fsync and mode preservation.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

/// Write `bytes` to `path` atomically.
///
/// On success the destination is byte-equal to `bytes`. On any error the
/// destination is left in its prior state (or absent if it did not exist).
///
/// `write_atomic` does **not** create the parent directory. Callers that
/// need that should run `fs::create_dir_all` first — adapters know whether
/// a missing parent is a bug or expected.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "write_atomic: path has no parent directory",
        )
    })?;

    // NamedTempFile::new_in places the temp file in the destination's
    // directory, which guarantees the eventual `persist` is a same-filesystem
    // rename (atomic on Unix; emulated via MoveFileEx on Windows). Constructed
    // with O_CREAT|O_EXCL via `mkstemp`, so two concurrent callers on the
    // same path won't collide.
    let mut tmp = NamedTempFile::new_in(parent)?;

    tmp.write_all(bytes)?;
    tmp.flush()?;

    // sync_all on the temp file forces the new contents (and metadata)
    // out to disk before the rename. Without this, a crash after rename
    // could expose a zero-length file even though rename "succeeded".
    tmp.as_file().sync_all()?;

    // Clone the destination's permissions onto the temp file so the rename
    // doesn't reset mode (e.g. 0600 -> umask default). NoOp if the dest
    // doesn't exist yet.
    if let Ok(meta) = fs::metadata(path) {
        // PermissionsError on the temp file is non-fatal — we still want
        // the data write to succeed. Log via tracing so a user with a
        // peculiar filesystem (e.g. SMB share) can diagnose.
        if let Err(e) = fs::set_permissions(tmp.path(), meta.permissions()) {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "could not preserve destination permissions on temp file",
            );
        }
    }

    // persist consumes the NamedTempFile; on success the temp path no
    // longer exists and `path` holds our bytes.
    tmp.persist(path).map_err(|e| e.error)?;

    // Finally fsync the parent directory so the rename itself is durable.
    // Some platforms (notably older ext4 without journaled metadata) can
    // reorder a rename behind subsequent writes without this. Best-effort
    // — some filesystems return EINVAL on directory fsync, which we treat
    // as already-durable.
    match fs::File::open(parent) {
        Ok(dir) => {
            if let Err(e) = dir.sync_all() {
                if e.raw_os_error() != Some(libc_einval()) {
                    tracing::warn!(
                        error = %e,
                        parent = %parent.display(),
                        "directory fsync failed; rename durability not guaranteed",
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                parent = %parent.display(),
                "could not open parent directory for fsync",
            );
        }
    }

    Ok(())
}

#[inline]
fn libc_einval() -> i32 {
    // EINVAL is 22 on Linux/macOS/BSD. On Windows, `File::open` on a
    // directory generally fails earlier and we already swallow that.
    22
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::ErrorKind;

    use super::write_atomic;

    use tempfile::tempdir;

    #[test]
    fn writes_fresh_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        write_atomic(&path, b"{\"hello\":\"trove\"}").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"hello\":\"trove\"}"
        );
    }

    #[test]
    fn overwrites_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn missing_parent_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope").join("config.json");
        let err = write_atomic(&path, b"x").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn rejects_path_without_parent() {
        // A bare filename has parent "" not None, so this exercises the
        // empty-parent branch via Path::new("/").
        let err = write_atomic(std::path::Path::new("/"), b"x").unwrap_err();
        // / has no parent that can hold a tempfile in a writable way; the
        // exact ErrorKind is platform-dependent but it must not silently
        // succeed.
        assert!(err.kind() != ErrorKind::Other || err.raw_os_error().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_unix_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        write_atomic(&path, b"new").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
    }

    #[cfg(unix)]
    #[test]
    fn fresh_file_uses_default_mode() {
        // No source file → we don't have a mode to preserve. The temp file
        // mode (0600 from mkstemp) carries through. We only assert that
        // the file is at least readable by the owner.
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        write_atomic(&path, b"x").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert!(mode & 0o400 != 0, "owner-readable expected, got {mode:o}");
    }

    #[test]
    fn does_not_create_parent_directory() {
        // Adapters that need mkdir-p must do it themselves. Verify
        // write_atomic refuses missing parents instead of silently
        // creating them.
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        let path = nested.join("c.json");
        let err = write_atomic(&path, b"x").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(!nested.exists(), "write_atomic must not have created {nested:?}");
    }

    #[test]
    fn temp_file_does_not_leak_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");
        write_atomic(&path, b"v1").unwrap();
        write_atomic(&path, b"v2").unwrap();

        // Only `file.txt` should remain — no `.tmp` siblings.
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["file.txt"]);
        assert_eq!(fs::read_to_string(&path).unwrap(), "v2");
    }
}
