//! PATH probing for harness binaries. Centralizes the test seam:
//! production code calls [`probe_path`], tests call [`probe_path_in`]
//! with a synthetic list of directories so detection is hermetic.

use std::path::PathBuf;

/// Resolves `binary` against the process's `$PATH`. Returns `None` if
/// the binary isn't on PATH (the typical "harness not installed" case).
#[must_use]
pub fn probe_path(binary: &str) -> Option<PathBuf> {
    which::which(binary).ok()
}

/// Resolves `binary` against an explicit list of search directories,
/// ignoring the process's `$PATH`. Used by tests that prepare a
/// `tempdir` containing a fake executable.
#[must_use]
pub fn probe_path_in(binary: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    use std::ffi::OsString;

    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut joined = OsString::new();
    for (i, dir) in dirs.iter().enumerate() {
        if i > 0 {
            joined.push(separator);
        }
        joined.push(dir);
    }

    // `which::which_in` searches the explicit path arg instead of the
    // process env. cwd is irrelevant for absolute-path resolution but
    // required by the API; pass the first dir as a stable choice.
    let cwd = dirs.first().cloned().unwrap_or_else(|| PathBuf::from("/"));
    which::which_in(binary, Some(joined), cwd).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &Path) {}

    #[test]
    fn probe_path_in_finds_binary_in_one_directory() {
        let dir = tempdir().unwrap();
        let exe_name = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        let exe = dir.path().join(exe_name);
        fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
        make_executable(&exe);

        let found = probe_path_in("claude", &[dir.path().to_path_buf()]);
        assert!(
            found.is_some(),
            "expected to resolve fake claude in {:?}",
            dir.path()
        );
    }

    #[test]
    fn probe_path_in_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        let result = probe_path_in("definitely-not-installed-xyz", &[dir.path().to_path_buf()]);
        assert!(result.is_none());
    }

    #[test]
    fn probe_path_in_returns_none_for_empty_dirs() {
        let result = probe_path_in("claude", &[]);
        assert!(result.is_none());
    }

    #[test]
    fn probe_path_in_searches_multiple_directories() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let exe_name = if cfg!(windows) {
            "gemini.exe"
        } else {
            "gemini"
        };
        let exe = second.path().join(exe_name);
        fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
        make_executable(&exe);

        let found = probe_path_in(
            "gemini",
            &[first.path().to_path_buf(), second.path().to_path_buf()],
        );
        assert!(found.is_some(), "expected gemini found in second dir");
    }
}
