//! PATH probing for harness binaries. Centralizes the test seam:
//! production code calls [`probe_path`], tests call [`probe_path_in`]
//! with a synthetic list of directories so detection is hermetic.

use std::path::PathBuf;

/// Resolves `binary` against the process's `$PATH`, augmented with
/// well-known Homebrew prefixes that macOS strips from GUI-launched
/// apps. launchd inherits a minimal PATH (`/usr/bin:/bin:/usr/sbin:/sbin`),
/// so a Trove launched from Finder / Spotlight / Dock would otherwise
/// miss every CLI installed under `/opt/homebrew/bin` (Apple Silicon)
/// or `/usr/local/bin` (Intel) — codex, claude, gemini, etc. Returns
/// `None` if the binary isn't on the augmented path (the typical
/// "harness not installed" case).
#[must_use]
pub fn probe_path(binary: &str) -> Option<PathBuf> {
    if let Ok(found) = which::which(binary) {
        return Some(found);
    }
    // Fall back to the well-known Homebrew prefixes. These are
    // canonical locations the user's shell rc would normally export,
    // but the GUI-launch case doesn't run shell rc.
    for fallback_dir in homebrew_fallback_dirs() {
        let candidate = fallback_dir.join(binary);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Fallback directories appended to the GUI-launch PATH probe. macOS-
/// only; other platforms return an empty list and the standard
/// `which::which` call is the only signal.
#[cfg(target_os = "macos")]
fn homebrew_fallback_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/local/sbin"),
    ]
}

#[cfg(not(target_os = "macos"))]
fn homebrew_fallback_dirs() -> Vec<PathBuf> {
    Vec::new()
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

    #[cfg(target_os = "macos")]
    #[test]
    fn homebrew_fallback_dirs_include_apple_silicon_and_intel_prefixes() {
        let dirs = super::homebrew_fallback_dirs();
        let joined: String = dirs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(":");
        assert!(joined.contains("/opt/homebrew/bin"), "{joined}");
        assert!(joined.contains("/usr/local/bin"), "{joined}");
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
