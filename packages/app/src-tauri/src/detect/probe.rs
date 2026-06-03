//! PATH probing for harness binaries. Centralizes the test seam:
//! production code calls [`probe_path`], tests call [`probe_path_in`]
//! with a synthetic list of directories so detection is hermetic.

use std::path::PathBuf;

/// Resolves `binary` against the process's `$PATH`, augmented with
/// well-known Homebrew prefixes (and the Node version managers — nvm,
/// volta, fnm) that macOS strips from GUI-launched apps. launchd
/// inherits a minimal PATH (`/usr/bin:/bin:/usr/sbin:/sbin`), so a
/// Trove launched from Finder / Spotlight / Dock would otherwise miss
/// every CLI installed under `/opt/homebrew/bin` (Apple Silicon),
/// `/usr/local/bin` (Intel), `~/.nvm/.../bin/` (nvm-managed node),
/// `~/.volta/bin/`, or `~/.local/share/fnm/.../bin/`. Returns `None`
/// if the binary isn't on the augmented path (the typical "harness
/// not installed" case).
///
/// The nvm/volta/fnm fallback follows the same resolution order the
/// `cursor-otel-hook` shim uses (see
/// `resources/hooks/cursor-otel-hook.cjs`) so users with a single
/// node toolchain don't need to know which one is installed for
/// Trove's npm-driven bootstraps (opencode plugin install today) to
/// pick the right one.
#[must_use]
pub fn probe_path(binary: &str) -> Option<PathBuf> {
    if let Ok(found) = which::which(binary) {
        return Some(found);
    }
    for fallback_dir in fallback_bin_dirs() {
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
fn fallback_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/local/sbin"),
    ];
    // Node version managers — nvm/volta/fnm — are the most common
    // way `npm` lands on user machines without a Homebrew node, and
    // (unlike Homebrew) install under $HOME so they need a per-user
    // resolution step. macOS-only because launchd PATH stripping is
    // a Mac-specific behavior; on Linux GUI launches inherit the
    // session env and these dirs are usually on PATH already.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        // nvm: `~/.nvm/versions/node/<semver>/bin/`. There can be
        // multiple installed versions; we walk them in mtime-desc
        // order so the most recently installed wins (matches how
        // `nvm use latest` behaves for the shell).
        let nvm_root = home.join(".nvm").join("versions").join("node");
        if let Ok(entries) = std::fs::read_dir(&nvm_root) {
            let mut versions: Vec<_> = entries
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir())
                .filter_map(|e| {
                    e.metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .map(|t| (t, e.path()))
                })
                .collect();
            versions.sort_by_key(|v| std::cmp::Reverse(v.0));
            for (_, p) in versions {
                dirs.push(p.join("bin"));
            }
        }
        // volta: `~/.volta/bin/` (single dir; volta proxies binaries
        // to the right toolchain version internally).
        dirs.push(home.join(".volta").join("bin"));
        // fnm: `~/.local/share/fnm/node-versions/<semver>/installation/bin/`.
        // Same multi-version pattern as nvm; mtime-desc for the most
        // recent install.
        let fnm_root = home
            .join(".local")
            .join("share")
            .join("fnm")
            .join("node-versions");
        if let Ok(entries) = std::fs::read_dir(&fnm_root) {
            let mut versions: Vec<_> = entries
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir())
                .filter_map(|e| {
                    e.metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .map(|t| (t, e.path()))
                })
                .collect();
            versions.sort_by_key(|v| std::cmp::Reverse(v.0));
            for (_, p) in versions {
                dirs.push(p.join("installation").join("bin"));
            }
        }
    }
    dirs
}

#[cfg(not(target_os = "macos"))]
fn fallback_bin_dirs() -> Vec<PathBuf> {
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
    fn fallback_bin_dirs_include_apple_silicon_and_intel_prefixes() {
        let dirs = super::fallback_bin_dirs();
        let joined: String = dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(":");
        assert!(joined.contains("/opt/homebrew/bin"), "{joined}");
        assert!(joined.contains("/usr/local/bin"), "{joined}");
        // The volta dir is unconditional once HOME is set — tests
        // running under a real-home macOS environment should see it
        // even if the user hasn't installed volta. nvm + fnm dirs
        // are conditional on the respective version-manager root
        // existing, so we don't assert them here.
        if std::env::var_os("HOME").is_some() {
            assert!(joined.contains(".volta/bin"), "{joined}");
        }
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
