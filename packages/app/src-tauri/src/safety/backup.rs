//! Timestamped sidecar backups.
//!
//! Before the first edit of a file, adapters call `backup_file` to copy
//! the original to `<file>.trove.bak.<RFC3339-microseconds>` next to it.
//! `prune_backups` keeps only the N most recent so a long-lived install
//! doesn't accumulate backup files indefinitely. `list_backups` returns
//! the existing backups newest-first for the dashboard.
//!
//! The timestamp suffix uses `_` instead of `:` so the resulting filename
//! is portable across Windows (which forbids `:` in filenames). It still
//! sorts lexicographically by time, which `list_backups` and
//! `prune_backups` rely on.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;

/// Format used for the suffix on backup filenames. Sorts lexicographically
/// in chronological order. Microsecond precision keeps backups distinct
/// even when adapters re-run within the same second.
const BACKUP_TIMESTAMP_FORMAT: &str = "%Y%m%dT%H%M%S%.6f";

/// Suffix prefix that marks a Trove backup. Anything matching
/// `<basename>.trove.bak.<...>` in the same directory is one of ours;
/// other files in the directory are left alone.
const BACKUP_INFIX: &str = ".trove.bak.";

/// Copies `path` to a new sidecar named
/// `<file>.trove.bak.<UTC-timestamp>`. Returns the backup path on success.
/// Errors propagate verbatim — a missing source file or read-only parent
/// directory is the caller's problem to handle.
pub fn backup_file(path: &Path) -> io::Result<PathBuf> {
    let backup = backup_path_for(path)?;
    fs::copy(path, &backup)?;
    Ok(backup)
}

/// Returns existing backups for `path`, newest-first. An empty vector if
/// none exist. Non-backup files in the same directory are ignored.
pub fn list_backups(path: &Path) -> io::Result<Vec<PathBuf>> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "list_backups: path has no parent directory",
        )
    })?;
    let stem = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "list_backups: path has no file name",
            )
        })?
        .to_owned();

    let mut backups: Vec<PathBuf> = match fs::read_dir(parent) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .filter(|e| is_backup_of(e.file_name().as_os_str(), &stem))
            .map(|e| e.path())
            .collect(),
        // A nonexistent parent dir is treated as "no backups yet" rather
        // than an error: list_backups is informational and shouldn't
        // bubble up filesystem state changes that prune/backup did
        // already report.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    // Sorted-descending puts the newest backup first. Lexicographic order
    // matches chronological order because of the timestamp format.
    backups.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    Ok(backups)
}

/// Deletes oldest backups beyond the `keep` most recent. Returns the
/// number of files removed. `keep == 0` deletes everything matching the
/// backup pattern for `path`.
pub fn prune_backups(path: &Path, keep: usize) -> io::Result<usize> {
    let mut backups = list_backups(path)?;
    if backups.len() <= keep {
        return Ok(0);
    }
    let to_remove = backups.split_off(keep);
    let mut removed = 0usize;
    for victim in to_remove {
        match fs::remove_file(&victim) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Another process already deleted it — fine.
            }
            Err(e) => return Err(e),
        }
    }
    Ok(removed)
}

fn backup_path_for(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "backup_file: path has no parent directory",
        )
    })?;
    let stem = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "backup_file: path has no file name",
            )
        })?
        .to_string_lossy()
        .into_owned();

    let suffix = Utc::now().format(BACKUP_TIMESTAMP_FORMAT).to_string();
    Ok(parent.join(format!("{stem}{BACKUP_INFIX}{suffix}")))
}

fn is_backup_of(candidate: &OsStr, source: &OsStr) -> bool {
    let Some(candidate) = candidate.to_str() else {
        return false;
    };
    let Some(source) = source.to_str() else {
        return false;
    };
    let prefix = format!("{source}{BACKUP_INFIX}");
    candidate.starts_with(&prefix) && candidate.len() > prefix.len()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::thread;
    use std::time::Duration;

    use super::{backup_file, list_backups, prune_backups};

    use tempfile::tempdir;

    #[test]
    fn backup_creates_sibling_file_with_same_contents() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("settings.json");
        fs::write(&src, b"original").unwrap();

        let backup = backup_file(&src).unwrap();
        assert!(backup.exists());
        assert_eq!(fs::read(&backup).unwrap(), b"original");
        assert_eq!(backup.parent(), Some(src.parent().unwrap()));
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings.json.trove.bak."));
    }

    #[test]
    fn list_backups_returns_newest_first() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("settings.json");
        fs::write(&src, b"v1").unwrap();

        let first = backup_file(&src).unwrap();
        // Microsecond precision plus a tiny sleep guarantees a distinct
        // timestamp in the suffix.
        thread::sleep(Duration::from_millis(2));
        let second = backup_file(&src).unwrap();

        let listed = list_backups(&src).unwrap();
        assert_eq!(listed, vec![second, first]);
    }

    #[test]
    fn list_backups_ignores_unrelated_files() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("settings.json");
        fs::write(&src, b"v1").unwrap();
        let _ = backup_file(&src).unwrap();

        // Sibling files that aren't ours.
        fs::write(dir.path().join("settings.json"), b"v1").unwrap();
        fs::write(dir.path().join("other.json"), b"x").unwrap();
        fs::write(dir.path().join("other.json.trove.bak.1"), b"x").unwrap();

        let listed = list_backups(&src).unwrap();
        assert_eq!(listed.len(), 1, "got {listed:?}");
    }

    #[test]
    fn list_backups_returns_empty_vec_when_parent_missing() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("nope").join("settings.json");
        let listed = list_backups(&src).unwrap();
        assert!(listed.is_empty());
    }

    #[test]
    fn prune_keeps_n_most_recent() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("settings.json");
        fs::write(&src, b"v1").unwrap();

        // Make 5 backups, keeping 2 most recent.
        let mut all = Vec::new();
        for _ in 0..5 {
            all.push(backup_file(&src).unwrap());
            thread::sleep(Duration::from_millis(2));
        }

        let removed = prune_backups(&src, 2).unwrap();
        assert_eq!(removed, 3);

        let surviving = list_backups(&src).unwrap();
        assert_eq!(surviving.len(), 2);
        // The newest two are the survivors.
        assert!(surviving.contains(&all[4]));
        assert!(surviving.contains(&all[3]));
    }

    #[test]
    fn prune_with_keep_zero_removes_everything() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("settings.json");
        fs::write(&src, b"v1").unwrap();
        let _ = backup_file(&src).unwrap();
        thread::sleep(Duration::from_millis(2));
        let _ = backup_file(&src).unwrap();

        let removed = prune_backups(&src, 0).unwrap();
        assert_eq!(removed, 2);
        assert!(list_backups(&src).unwrap().is_empty());
    }

    #[test]
    fn prune_when_under_threshold_is_noop() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("settings.json");
        fs::write(&src, b"v1").unwrap();
        let _ = backup_file(&src).unwrap();

        assert_eq!(prune_backups(&src, 5).unwrap(), 0);
        assert_eq!(list_backups(&src).unwrap().len(), 1);
    }

    #[test]
    fn backup_file_errors_when_source_missing() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("ghost.json");
        let err = backup_file(&src).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
