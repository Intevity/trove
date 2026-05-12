//! Shared shell-rc patching for the Tier 3 wrapper-based adapters
//! (Aider, Copilot CLI). Both adapters install a Trove-managed block
//! into the user's shell rc files (`~/.zshrc`, `~/.bashrc`,
//! `~/.config/fish/config.fish` — whichever exist) that defines
//! shell *functions* (not aliases — aliases don't expand inside
//! scripts) which exec the bundled wrapper script.
//!
//! ## Why a bespoke patcher and not `safety::sentinels::Format::Yaml`
//!
//! YAML's validator parses the host file with `serde_yml`. A real shell
//! rc has lines like `export PATH="..."` and unbalanced quotes that
//! YAML rejects. So the sentinels engine isn't a good fit here. We
//! use a small purpose-built patcher that:
//! - Reads the rc file as plain text (or treats missing as empty).
//! - Looks for a `# trove:start ... # trove:end` block.
//! - Replaces or appends the block atomically.
//! - On revert, strips the block and any single trailing blank line.
//!
//! The fence format matches the rest of Trove's adapters so the
//! convention stays consistent across formats.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::ipc::IpcError;
use crate::safety::atomic::write_atomic;
use crate::safety::backup::{backup_file, prune_backups};
use crate::safety::sentinels::Format;

use super::{ApplyOptions, BACKUPS_TO_KEEP, PatchPreview, PreviewStatus, TrovePatch};

/// Trove's fence markers in shell rc files. Same as TOML / YAML.
const FENCE_START: &str = "# trove:start";
const FENCE_END: &str = "# trove:end";

/// Specification for one shell-function install: the function name
/// the user will invoke (`aider`, `gh-copilot`) and the bundled
/// wrapper script's path on disk.
#[derive(Clone, Debug)]
pub struct WrapperSpec {
    /// Name of the shell function to define (e.g. `aider`).
    pub function_name: &'static str,
    /// Absolute path of the bundled wrapper script on disk. Resolved
    /// at runtime via Tauri's `resource_dir()`.
    pub wrapper_path: PathBuf,
    /// Logical harness id used in commit-message-style comments inside
    /// the managed block (e.g. `trove::aider`).
    pub label: &'static str,
}

/// Render the Trove-managed block for `spec` and `opts`. Same payload
/// → same hash → idempotent re-apply.
#[must_use]
pub fn build_managed_block(spec: &WrapperSpec, opts: &ApplyOptions) -> String {
    let path = spec.wrapper_path.display();
    let mut attrs = String::new();
    for (k, v) in &opts.custom_attributes {
        // shell-safe escape: only allow ASCII alnum + simple punct in
        // values, drop the rest. Trove's UI validates user input first
        // but defense-in-depth here is cheap.
        let safe = v
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | ' '))
            .collect::<String>();
        let _ = writeln!(attrs, "    # attr: {k}={safe}");
    }

    format!(
        "{label}() {{ \"{path}\" \"$@\"; }}\n\
{attrs}\
",
        label = spec.function_name,
        path = path,
        attrs = attrs,
    )
}

/// `~/.zshrc`, `~/.bashrc`, `~/.config/fish/config.fish` — the shell
/// rc files Trove will append to whichever exist on disk. Filtered by
/// existence at apply time; missing files are skipped.
#[must_use]
pub fn shell_rc_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".zshrc"),
        home.join(".bashrc"),
        home.join(".bash_profile"),
        home.join(".config").join("fish").join("config.fish"),
    ]
}

/// `~/.zshrc`, `~/.bashrc`, etc. that exist on disk. Used by adapter
/// preview/apply/revert.
#[must_use]
pub fn existing_shell_rc(home: &Path) -> Vec<PathBuf> {
    shell_rc_candidates(home)
        .into_iter()
        .filter(|p| p.exists())
        .collect()
}

/// Combine: the canonical shell rc Trove writes its sentinel into when
/// at least one rc file exists. Order of preference: zshrc, bashrc,
/// `bash_profile`, fish. Returns `None` when none exist (the adapter's
/// preview surfaces a warning row that the user must create one
/// before enabling).
#[must_use]
pub fn primary_shell_rc(home: &Path) -> Option<PathBuf> {
    existing_shell_rc(home).into_iter().next()
}

/// Apply Trove's managed block to the primary shell rc file. Atomic
/// write + backup. Returns the resulting `TrovePatch` (or
/// `IpcError::Internal` if no shell rc exists).
pub fn apply_to_primary_shell_rc(
    home: &Path,
    spec: &WrapperSpec,
    opts: &ApplyOptions,
) -> Result<TrovePatch, IpcError> {
    let path = primary_shell_rc(home).ok_or_else(|| IpcError::Internal {
        reason: "no shell rc file (~/.zshrc, ~/.bashrc, fish/config.fish) exists; create one before enabling".into(),
    })?;
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let block = build_managed_block(spec, opts);
    let new_content = upsert_managed_block(&current, &block);

    if new_content != current {
        backup_file(&path).map_err(|e| IpcError::Io {
            path: path.display().to_string(),
            reason: format!("backup failed: {e}"),
        })?;
        write_atomic(&path, new_content.as_bytes()).map_err(|e| IpcError::Io {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let _ = prune_backups(&path, BACKUPS_TO_KEEP);
    }

    Ok(TrovePatch {
        managed_block_hash: sha256_hex(block.as_bytes()),
        file_hash_at_last_write: sha256_hex(new_content.as_bytes()),
        format: Format::Yaml, // shell rc shares the # comment fence with YAML/TOML
        last_written_region_payload: block,
    })
}

/// Compute the diff for `apply_to_primary_shell_rc` without writing.
/// Status: `Idempotent` if the existing block is byte-identical to
/// the proposed block; `Conflict` if a Trove block is present with
/// different bytes; `Fresh` otherwise.
pub fn preview_for_primary_shell_rc(
    home: &Path,
    spec: &WrapperSpec,
    opts: &ApplyOptions,
) -> Result<PatchPreview, IpcError> {
    let path = primary_shell_rc(home).ok_or_else(|| IpcError::Internal {
        reason: "no shell rc file exists; create ~/.zshrc or ~/.bashrc before enabling".into(),
    })?;
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let proposed = build_managed_block(spec, opts);
    let new_content = upsert_managed_block(&current, &proposed);

    let status = if let Some(existing) = extract_managed_block(&current) {
        if existing.trim() == proposed.trim() {
            PreviewStatus::Idempotent
        } else {
            PreviewStatus::Conflict
        }
    } else {
        PreviewStatus::Fresh
    };

    Ok(PatchPreview {
        config_path: path,
        format: Format::Yaml,
        before: current,
        after: new_content,
        status,
    })
}

/// Strip Trove's managed block from the primary shell rc file.
/// No-op when no block is present.
pub fn revert_primary_shell_rc(home: &Path) -> Result<(), IpcError> {
    let Some(path) = primary_shell_rc(home) else {
        return Ok(());
    };
    let Ok(current) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    if extract_managed_block(&current).is_none() {
        return Ok(());
    }
    let stripped = strip_managed_block(&current);
    if stripped != current {
        backup_file(&path).map_err(|e| IpcError::Io {
            path: path.display().to_string(),
            reason: format!("backup failed: {e}"),
        })?;
        write_atomic(&path, stripped.as_bytes()).map_err(|e| IpcError::Io {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let _ = prune_backups(&path, BACKUPS_TO_KEEP);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plain-text fence helpers (no parser; shell rc is not a parseable format).
// ---------------------------------------------------------------------------

/// Insert or replace Trove's managed block in `content`. Idempotent.
#[must_use]
pub fn upsert_managed_block(content: &str, block: &str) -> String {
    if let Some((start, end)) = locate_block(content) {
        let mut out = String::with_capacity(content.len() + block.len());
        out.push_str(&content[..start]);
        out.push_str(&render_fenced(block));
        out.push_str(&content[end..]);
        out
    } else {
        let mut out = String::with_capacity(content.len() + block.len() + 64);
        out.push_str(content);
        if !content.is_empty() && !content.ends_with('\n') {
            out.push('\n');
        }
        if !content.is_empty() && !content.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&render_fenced(block));
        out
    }
}

/// Strip Trove's managed block from `content`. No-op when absent.
/// Removes the visual-separator blank line `upsert_managed_block`
/// inserts above the fence so an apply-then-revert sequence is
/// byte-identical to the original.
#[must_use]
pub fn strip_managed_block(content: &str) -> String {
    let Some((start, end)) = locate_block(content) else {
        return content.to_string();
    };
    let prefix = &content[..start];
    // Drop the single visual-separator newline before the fence (if
    // one exists). The original content always ends with at most one
    // newline; upsert adds a second one to create a blank line. We
    // peel that second newline back off here.
    let prefix_trimmed: &str = if prefix.ends_with("\n\n") {
        &prefix[..prefix.len() - 1]
    } else {
        prefix
    };
    let mut out = String::with_capacity(content.len());
    out.push_str(prefix_trimmed);
    let after = &content[end..];
    out.push_str(after.trim_start_matches('\n'));
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Return the inner managed-block text (between the fence markers,
/// excluding the markers themselves) if present.
#[must_use]
pub fn extract_managed_block(content: &str) -> Option<String> {
    let (start, end) = locate_block(content)?;
    let block = &content[start..end];
    let mut lines: Vec<&str> = block.lines().collect();
    if lines.first().is_some_and(|l| l.trim_start().starts_with(FENCE_START))
        && lines.last().is_some_and(|l| l.trim_start().starts_with(FENCE_END))
    {
        // Drop fence start + fence end lines.
        lines.remove(0);
        lines.pop();
    }
    Some(lines.join("\n"))
}

/// Wrap `block` in fence markers, ensuring a single trailing newline.
fn render_fenced(block: &str) -> String {
    let body = block.trim_end_matches('\n');
    format!("{FENCE_START}\n{body}\n{FENCE_END}\n")
}

/// Locate the byte range covering Trove's managed block, including
/// the fence start/end lines themselves. Returns `None` when no
/// fence is present. When multiple fences exist (shouldn't happen in
/// practice but defensive) returns the first.
fn locate_block(content: &str) -> Option<(usize, usize)> {
    let start_idx = find_line_start(content, FENCE_START)?;
    let after_start_line_end = match content[start_idx..].find('\n') {
        Some(n) => start_idx + n + 1,
        None => return None,
    };
    let end_off = find_line_start(&content[after_start_line_end..], FENCE_END)?;
    let end_idx = after_start_line_end + end_off;
    let end_line_end = match content[end_idx..].find('\n') {
        Some(n) => end_idx + n + 1,
        None => content.len(),
    };
    Some((start_idx, end_line_end))
}

/// Find the byte offset where the next line beginning with `prefix`
/// starts. Skips leading whitespace on each line.
fn find_line_start(content: &str, prefix: &str) -> Option<usize> {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        if line.trim_start().starts_with(prefix) {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn fixture_spec() -> WrapperSpec {
        WrapperSpec {
            function_name: "aider",
            wrapper_path: PathBuf::from("/opt/trove/wrappers/trove-aider"),
            label: "trove::aider",
        }
    }

    #[test]
    fn fence_markers_match_yaml_toml_convention() {
        assert_eq!(FENCE_START, "# trove:start");
        assert_eq!(FENCE_END, "# trove:end");
    }

    #[test]
    fn upsert_block_appends_when_absent() {
        let original = "export FOO=bar\n";
        let block = "aider() { echo aider; }\n";
        let out = upsert_managed_block(original, block);
        assert!(out.contains("# trove:start\n"));
        assert!(out.contains("# trove:end\n"));
        assert!(out.contains("aider() { echo aider; }"));
        assert!(out.starts_with("export FOO=bar"));
    }

    #[test]
    fn upsert_block_replaces_existing_block_in_place() {
        let original = "before\n# trove:start\nold\n# trove:end\nafter\n";
        let block = "new\n";
        let out = upsert_managed_block(original, block);
        assert!(out.contains("new"));
        assert!(!out.contains("old"));
        assert!(out.starts_with("before\n"));
        assert!(out.ends_with("after\n"));
    }

    #[test]
    fn strip_block_removes_fence_and_body() {
        let original = "before\n# trove:start\nbody\n# trove:end\nafter\n";
        let stripped = strip_managed_block(original);
        assert_eq!(stripped, "before\nafter\n");
    }

    #[test]
    fn strip_block_is_noop_when_absent() {
        let original = "user-only-content\n";
        assert_eq!(strip_managed_block(original), original);
    }

    #[test]
    fn extract_returns_inner_block_text() {
        let original = "before\n# trove:start\nline-1\nline-2\n# trove:end\nafter\n";
        let inner = extract_managed_block(original).unwrap();
        assert_eq!(inner, "line-1\nline-2");
    }

    #[test]
    fn extract_returns_none_when_no_fence() {
        assert!(extract_managed_block("noop").is_none());
    }

    #[test]
    fn build_managed_block_includes_function_definition_with_wrapper_path() {
        let spec = fixture_spec();
        let block = build_managed_block(&spec, &ApplyOptions::default());
        assert!(block.contains("aider() {"));
        assert!(block.contains("/opt/trove/wrappers/trove-aider"));
        assert!(
            !block.contains("TROVE_LOG_USER_PROMPTS"),
            "the log-user-prompts toggle was removed; the env var must not appear"
        );
    }

    #[test]
    fn build_managed_block_emits_attribute_comments() {
        let spec = fixture_spec();
        let mut opts = ApplyOptions::default();
        opts.custom_attributes.insert("team".into(), "platform".into());
        let block = build_managed_block(&spec, &opts);
        assert!(block.contains("# attr: team=platform"));
    }

    #[test]
    fn apply_then_revert_is_byte_identical_to_original_user_content() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let zshrc = home.join(".zshrc");
        let original = "# user shell rc\nexport PATH=\"$HOME/bin:$PATH\"\n";
        fs::write(&zshrc, original).unwrap();

        let spec = fixture_spec();
        apply_to_primary_shell_rc(home, &spec, &ApplyOptions::default()).unwrap();
        let after_apply = fs::read_to_string(&zshrc).unwrap();
        assert_ne!(after_apply, original, "apply must change the file");
        assert!(after_apply.contains("# trove:start"));

        revert_primary_shell_rc(home).unwrap();
        let after_revert = fs::read_to_string(&zshrc).unwrap();
        assert_eq!(after_revert, original);
    }

    #[test]
    fn apply_is_idempotent_when_options_are_unchanged() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let zshrc = home.join(".zshrc");
        fs::write(&zshrc, "user content\n").unwrap();

        let spec = fixture_spec();
        apply_to_primary_shell_rc(home, &spec, &ApplyOptions::default()).unwrap();
        let after_first = fs::read_to_string(&zshrc).unwrap();
        apply_to_primary_shell_rc(home, &spec, &ApplyOptions::default()).unwrap();
        let after_second = fs::read_to_string(&zshrc).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn preview_status_is_fresh_for_an_unmanaged_rc() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let zshrc = home.join(".zshrc");
        fs::write(&zshrc, "user content\n").unwrap();
        let spec = fixture_spec();
        let preview = preview_for_primary_shell_rc(home, &spec, &ApplyOptions::default()).unwrap();
        assert!(matches!(preview.status, PreviewStatus::Fresh));
    }

    #[test]
    fn preview_status_is_idempotent_after_a_matching_apply() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let zshrc = home.join(".zshrc");
        fs::write(&zshrc, "").unwrap();
        let spec = fixture_spec();
        apply_to_primary_shell_rc(home, &spec, &ApplyOptions::default()).unwrap();
        let preview = preview_for_primary_shell_rc(home, &spec, &ApplyOptions::default()).unwrap();
        assert!(matches!(preview.status, PreviewStatus::Idempotent));
    }

    #[test]
    fn preview_status_is_conflict_when_user_edits_inside_block() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let zshrc = home.join(".zshrc");
        fs::write(
            &zshrc,
            "before\n# trove:start\nuser hand-edit\n# trove:end\nafter\n",
        )
        .unwrap();
        let spec = fixture_spec();
        let preview = preview_for_primary_shell_rc(home, &spec, &ApplyOptions::default()).unwrap();
        assert!(matches!(preview.status, PreviewStatus::Conflict));
    }

    #[test]
    fn apply_errors_when_no_shell_rc_exists() {
        let dir = tempdir().unwrap();
        let spec = fixture_spec();
        let err =
            apply_to_primary_shell_rc(dir.path(), &spec, &ApplyOptions::default()).unwrap_err();
        assert!(matches!(err, IpcError::Internal { .. }));
    }

    #[test]
    fn revert_when_no_rc_or_no_block_is_a_noop() {
        let dir = tempdir().unwrap();
        revert_primary_shell_rc(dir.path()).unwrap(); // no rc file at all
        let zshrc = dir.path().join(".zshrc");
        std::fs::write(&zshrc, "user-only\n").unwrap();
        revert_primary_shell_rc(dir.path()).unwrap(); // rc exists but no block
        assert_eq!(fs::read_to_string(&zshrc).unwrap(), "user-only\n");
    }
}
