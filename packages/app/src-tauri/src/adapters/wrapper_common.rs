//! Shared shell-rc patching for wrapper-style and env-var-export adapters.
//!
//! Two families of adapters write managed blocks into the user's primary
//! shell rc (`~/.zshrc`, `~/.bashrc`, `~/.config/fish/config.fish`):
//!
//! * **Wrapper adapters** (Aider, Copilot CLI, Cursor CLI) define shell
//!   *functions* (not aliases — aliases don't expand inside scripts)
//!   that exec a bundled wrapper script before the real tool.
//! * **Export adapters** (Droid) inject plain `export KEY=VALUE` lines
//!   so the tool picks them up at startup.
//!
//! Both families use the same namespaced fence format
//! (`# trove:{adapter_id}:start` / `# trove:{adapter_id}:end`) and the
//! same atomic-write + backup + legacy-migration machinery, exposed here
//! as three unified public functions: [`apply_to_primary_shell_rc`],
//! [`preview_for_primary_shell_rc`], [`revert_primary_shell_rc`].
//!
//! Callers pre-compute the block body via [`build_managed_block`] or
//! [`build_export_block`], then pass it along with a [`LegacyProbe`]
//! that tells the fence locator how to recognise a pre-namespace block
//! that belongs to this adapter.
//!
//! ## Why a bespoke patcher and not `safety::sentinels::Format::Yaml`
//!
//! YAML's validator parses the host file with `serde_yml`. A real shell
//! rc has lines like `export PATH="..."` and unbalanced quotes that
//! YAML rejects. So the YAML branch of the sentinels engine isn't a
//! good fit here, and using `Format::Yaml` for the [`TrovePatch`] we
//! report would break the conflict-payload IPC path (it dispatches to
//! `extract_region(Format::Yaml, ...)`, which validates the whole rc
//! as YAML and errors with `malformed yaml document`). We report
//! [`Format::Shell`] in the `TrovePatch` so that path uses the
//! `comment_fence::*_shell` trio, which shares the same fence syntax
//! but skips validation.
//!
//! The bespoke patcher in this module remains for the apply / preview /
//! revert flow (no parser, plain-text fence scan).
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

/// Trove's legacy fence markers in shell rc files. Pre-v0.5.1, all
/// wrapper adapters wrote the same un-namespaced fence pair, which
/// meant a second wrapper-style Enable would overwrite the first one's
/// block. The current code writes per-adapter fences (see
/// [`fence_start`] / [`fence_end`]); these legacy constants are kept
/// only so [`locate_block`] can detect an existing pre-namespace block
/// and migrate it forward on the next upsert.
const LEGACY_FENCE_START: &str = "# trove:start";
const LEGACY_FENCE_END: &str = "# trove:end";

/// Per-adapter fence start marker. Each adapter writes a block fenced
/// with its own id so multiple adapters can coexist in the same shell rc.
#[must_use]
fn fence_start(adapter_id: &str) -> String {
    format!("# trove:{adapter_id}:start")
}

/// Per-adapter fence end marker. See [`fence_start`].
#[must_use]
fn fence_end(adapter_id: &str) -> String {
    format!("# trove:{adapter_id}:end")
}

/// Specification for one shell-function install: the function name(s)
/// the user will invoke (`aider`, `copilot`, `gh-copilot`) and the
/// bundled wrapper script's path on disk.
#[derive(Clone, Debug)]
pub struct WrapperSpec {
    /// Harness id used to namespace the fence markers in the shell rc
    /// (`# trove:{adapter_id}:start` / `# trove:{adapter_id}:end`).
    /// Multiple wrapper adapters coexist by using distinct ids.
    pub adapter_id: &'static str,
    /// Names of the shell functions to define. Most adapters install a
    /// single name (`["aider"]`, `["cursor-agent"]`); `copilot-cli`
    /// installs both `["copilot", "gh-copilot"]` to cover the new
    /// standalone CLI and the deprecated gh-extension simultaneously.
    /// Empty slices are a programmer error and will panic at apply
    /// time — adapters set this as a `const`.
    pub function_names: &'static [&'static str],
    /// Absolute path of the bundled wrapper script on disk. Resolved
    /// at runtime via Tauri's `resource_dir()`.
    pub wrapper_path: PathBuf,
    /// Logical harness id used in commit-message-style comments inside
    /// the managed block (e.g. `trove::aider`).
    pub label: &'static str,
}

/// Spec for shell-RC adapters that inject plain `export KEY=VALUE` lines
/// rather than shell-function wrappers.
///
/// `legacy_body_probe`: when `Some`, enables migration of a legacy
/// un-namespaced `# trove:start` block whose body contains the probe
/// string. On the next `apply`, that block is replaced in-place with the
/// namespaced `# trove:{adapter_id}:start` form.
#[derive(Clone, Debug)]
pub struct ExportSpec {
    /// Fence namespace: `# trove:{adapter_id}:start` / `# trove:{adapter_id}:end`.
    pub adapter_id: &'static str,
    /// Key-value pairs to write as `export K=V` lines (in order).
    pub vars: &'static [(&'static str, &'static str)],
    /// If `Some`, adopt a legacy un-namespaced block whose body contains
    /// this string. `None` skips legacy detection.
    pub legacy_body_probe: Option<&'static str>,
}

/// Identifies an un-namespaced legacy block that belongs to this adapter,
/// enabling in-place migration on the next apply. Pass `None` to skip
/// legacy detection entirely.
///
/// Pre-v0.5.1, all wrapper adapters wrote the same un-namespaced
/// `# trove:start` fence; [`LegacyProbe`] tells the locator how to
/// confirm a candidate un-namespaced block belongs to *this* adapter
/// rather than a different one.
#[derive(Clone, Copy, Debug)]
pub enum LegacyProbe<'a> {
    /// Adopt a legacy block if its body defines a shell function named
    /// `name() {` for any of the given names (`WrapperSpec` adapters).
    FunctionNames(&'a [&'static str]),
    /// Adopt a legacy block if its body contains this literal string
    /// (`ExportSpec` adapters).
    BodyContains(&'a str),
}

/// Render the Trove-managed block for `spec` and `opts`. Same payload
/// → same hash → idempotent re-apply. When `spec.function_names` has
/// more than one entry, one shell-function definition is emitted per
/// name, all pointing at the same wrapper.
#[must_use]
pub fn build_managed_block(spec: &WrapperSpec, opts: &ApplyOptions) -> String {
    assert!(
        !spec.function_names.is_empty(),
        "WrapperSpec.function_names must contain at least one name",
    );
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

    let mut out = String::new();
    for name in spec.function_names {
        let _ = writeln!(out, "{name}() {{ \"{path}\" \"$@\"; }}");
    }
    out.push_str(&attrs);
    out
}

/// Render the managed export block for `spec` — one `export K=V\n` per var.
#[must_use]
pub fn build_export_block(spec: &ExportSpec) -> String {
    let mut out = String::new();
    for (k, v) in spec.vars {
        let _ = writeln!(out, "export {k}={v}");
    }
    out
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

/// Apply a managed block to the primary shell rc file. Atomic write +
/// backup. Returns the resulting `TrovePatch` (or `IpcError::Internal`
/// if no shell rc exists).
///
/// `body` is the pre-computed content between the fence markers (e.g.
/// from [`build_managed_block`] or [`build_export_block`]).
/// `legacy` controls whether and how a pre-namespace block is adopted;
/// pass `None` to skip legacy migration.
pub fn apply_to_primary_shell_rc(
    home: &Path,
    adapter_id: &str,
    body: &str,
    legacy: Option<LegacyProbe<'_>>,
) -> Result<TrovePatch, IpcError> {
    let path = primary_shell_rc(home).ok_or_else(|| IpcError::Internal {
        reason: "no shell rc file (~/.zshrc, ~/.bashrc, fish/config.fish) exists; create one before enabling".into(),
    })?;
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let new_content = upsert_managed_block(&current, body, adapter_id, legacy);

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
        managed_block_hash: sha256_hex(body.as_bytes()),
        file_hash_at_last_write: sha256_hex(new_content.as_bytes()),
        // Shell rc shares the # comment fence with YAML/TOML but isn't
        // a parseable document, so the conflict-payload path must use
        // the Shell branch of the sentinels engine (no YAML validate).
        format: Format::Shell,
        last_written_region_payload: body.to_string(),
    })
}

/// Compute the diff for `apply_to_primary_shell_rc` without writing.
/// Status: `Idempotent` if the existing block is byte-identical to
/// the proposed block; `Conflict` if a Trove block is present with
/// different bytes; `Fresh` otherwise.
pub fn preview_for_primary_shell_rc(
    home: &Path,
    adapter_id: &str,
    body: &str,
    legacy: Option<LegacyProbe<'_>>,
) -> Result<PatchPreview, IpcError> {
    let path = primary_shell_rc(home).ok_or_else(|| IpcError::Internal {
        reason: "no shell rc file exists; create ~/.zshrc or ~/.bashrc before enabling".into(),
    })?;
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let new_content = upsert_managed_block(&current, body, adapter_id, legacy);

    let status = if let Some(existing) = extract_managed_block(&current, adapter_id, legacy) {
        if existing.trim() == body.trim() {
            PreviewStatus::Idempotent
        } else {
            PreviewStatus::Conflict
        }
    } else {
        PreviewStatus::Fresh
    };

    Ok(PatchPreview {
        config_path: path,
        format: Format::Shell,
        before: current,
        after: new_content,
        status,
    })
}

/// Strip Trove's managed block for one adapter from the primary shell
/// rc file. No-op when no block is present for `adapter_id`. Per-adapter
/// scoping means reverting (say) aider leaves an enabled copilot-cli's
/// block intact.
pub fn revert_primary_shell_rc(
    home: &Path,
    adapter_id: &str,
    legacy: Option<LegacyProbe<'_>>,
) -> Result<(), IpcError> {
    let Some(path) = primary_shell_rc(home) else {
        return Ok(());
    };
    let Ok(current) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    if locate_block(&current, adapter_id, legacy).is_none() {
        return Ok(());
    }
    let stripped = strip_managed_block(&current, adapter_id, legacy);
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

/// Insert or replace this adapter's managed block in `content`. The
/// fence is namespaced (`# trove:{adapter_id}:start`) so multiple
/// adapters coexist. When `legacy` is `Some` and no namespaced fence is
/// present, a matching un-namespaced block is migrated in place.
/// Idempotent.
#[must_use]
pub fn upsert_managed_block(
    content: &str,
    block: &str,
    adapter_id: &str,
    legacy: Option<LegacyProbe<'_>>,
) -> String {
    if let Some((start, end)) = locate_block(content, adapter_id, legacy) {
        let mut out = String::with_capacity(content.len() + block.len());
        out.push_str(&content[..start]);
        out.push_str(&render_fenced(block, adapter_id));
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
        out.push_str(&render_fenced(block, adapter_id));
        out
    }
}

/// Strip this adapter's managed block from `content`. No-op when
/// absent. Removes the visual-separator blank line
/// `upsert_managed_block` inserts above the fence so an
/// apply-then-revert sequence is byte-identical to the original.
#[must_use]
pub fn strip_managed_block(
    content: &str,
    adapter_id: &str,
    legacy: Option<LegacyProbe<'_>>,
) -> String {
    let Some((start, end)) = locate_block(content, adapter_id, legacy) else {
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
/// excluding the markers themselves) for `adapter_id` if present.
#[must_use]
pub fn extract_managed_block(
    content: &str,
    adapter_id: &str,
    legacy: Option<LegacyProbe<'_>>,
) -> Option<String> {
    let (start, end) = locate_block(content, adapter_id, legacy)?;
    let block = &content[start..end];
    let mut lines: Vec<&str> = block.lines().collect();
    let opens_with_fence = lines.first().is_some_and(|l| {
        let t = l.trim_start();
        t.starts_with(&fence_start(adapter_id)) || t.starts_with(LEGACY_FENCE_START)
    });
    let closes_with_fence = lines.last().is_some_and(|l| {
        let t = l.trim_start();
        t.starts_with(&fence_end(adapter_id)) || t.starts_with(LEGACY_FENCE_END)
    });
    if opens_with_fence && closes_with_fence {
        // Drop fence start + fence end lines.
        lines.remove(0);
        lines.pop();
    }
    Some(lines.join("\n"))
}

/// Wrap `block` in this adapter's namespaced fence markers, ensuring a
/// single trailing newline.
fn render_fenced(block: &str, adapter_id: &str) -> String {
    let body = block.trim_end_matches('\n');
    format!(
        "{start}\n{body}\n{end}\n",
        start = fence_start(adapter_id),
        end = fence_end(adapter_id),
    )
}

/// Locate the byte range covering this adapter's managed block,
/// including the fence start/end lines themselves. Tries the namespaced
/// fence first; falls back to a legacy un-namespaced block matched via
/// `legacy` (when `Some`). Returns `None` when no matching block is found.
fn locate_block(
    content: &str,
    adapter_id: &str,
    legacy: Option<LegacyProbe<'_>>,
) -> Option<(usize, usize)> {
    if let Some(span) =
        locate_fence_pair(content, &fence_start(adapter_id), &fence_end(adapter_id))
    {
        return Some(span);
    }
    let legacy = legacy?;
    let legacy_span = locate_fence_pair(content, LEGACY_FENCE_START, LEGACY_FENCE_END)?;
    let body = &content[legacy_span.0..legacy_span.1];
    let owns = match legacy {
        LegacyProbe::FunctionNames(names) => {
            // Empty slice: no function match possible, don't adopt.
            !names.is_empty()
                && names.iter().any(|name| body.contains(&format!("{name}() {{")))
        }
        LegacyProbe::BodyContains(probe) => body.contains(probe),
    };
    if owns { Some(legacy_span) } else { None }
}

/// Locate the byte range covering a fence pair (`start_line..=end_line`
/// inclusive of trailing newline). Returns `None` when either fence is
/// absent or the end appears before the start.
fn locate_fence_pair(content: &str, start_marker: &str, end_marker: &str) -> Option<(usize, usize)> {
    let start_idx = find_line_start(content, start_marker)?;
    let after_start_line_end = match content[start_idx..].find('\n') {
        Some(n) => start_idx + n + 1,
        None => return None,
    };
    let end_off = find_line_start(&content[after_start_line_end..], end_marker)?;
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

    const AIDER_NAMES: &[&str] = &["aider"];
    const COPILOT_NAMES: &[&str] = &["copilot", "gh-copilot"];

    #[allow(clippy::unnecessary_wraps)]
    fn aider_legacy() -> Option<LegacyProbe<'static>> {
        Some(LegacyProbe::FunctionNames(AIDER_NAMES))
    }

    #[allow(clippy::unnecessary_wraps)]
    fn copilot_legacy() -> Option<LegacyProbe<'static>> {
        Some(LegacyProbe::FunctionNames(COPILOT_NAMES))
    }

    fn fixture_spec() -> WrapperSpec {
        WrapperSpec {
            adapter_id: "aider",
            function_names: AIDER_NAMES,
            wrapper_path: PathBuf::from("/opt/trove/wrappers/trove-aider"),
            label: "trove::aider",
        }
    }

    fn fixture_spec_two_names() -> WrapperSpec {
        WrapperSpec {
            adapter_id: "copilot-cli",
            function_names: COPILOT_NAMES,
            wrapper_path: PathBuf::from("/opt/trove/wrappers/trove-copilot"),
            label: "trove::copilot-cli",
        }
    }

    #[test]
    fn fence_markers_namespace_per_adapter() {
        assert_eq!(fence_start("aider"), "# trove:aider:start");
        assert_eq!(fence_end("aider"), "# trove:aider:end");
        assert_eq!(fence_start("copilot-cli"), "# trove:copilot-cli:start");
        assert_eq!(fence_end("copilot-cli"), "# trove:copilot-cli:end");
        // Legacy markers preserved for migration detection.
        assert_eq!(LEGACY_FENCE_START, "# trove:start");
        assert_eq!(LEGACY_FENCE_END, "# trove:end");
    }

    #[test]
    fn upsert_block_appends_when_absent() {
        let original = "export FOO=bar\n";
        let block = "aider() { echo aider; }\n";
        let out = upsert_managed_block(original, block, "aider", aider_legacy());
        assert!(out.contains("# trove:aider:start\n"));
        assert!(out.contains("# trove:aider:end\n"));
        assert!(out.contains("aider() { echo aider; }"));
        assert!(out.starts_with("export FOO=bar"));
    }

    #[test]
    fn upsert_block_replaces_existing_block_in_place() {
        let original =
            "before\n# trove:aider:start\nold\n# trove:aider:end\nafter\n";
        let block = "new\n";
        let out = upsert_managed_block(original, block, "aider", aider_legacy());
        assert!(out.contains("new"));
        assert!(!out.contains("old"));
        assert!(out.starts_with("before\n"));
        assert!(out.ends_with("after\n"));
    }

    #[test]
    fn strip_block_removes_fence_and_body() {
        let original =
            "before\n# trove:aider:start\nbody\n# trove:aider:end\nafter\n";
        let stripped = strip_managed_block(original, "aider", aider_legacy());
        assert_eq!(stripped, "before\nafter\n");
    }

    #[test]
    fn strip_block_is_noop_when_absent() {
        let original = "user-only-content\n";
        assert_eq!(
            strip_managed_block(original, "aider", aider_legacy()),
            original
        );
    }

    #[test]
    fn extract_returns_inner_block_text() {
        let original =
            "before\n# trove:aider:start\nline-1\nline-2\n# trove:aider:end\nafter\n";
        let inner = extract_managed_block(original, "aider", aider_legacy()).unwrap();
        assert_eq!(inner, "line-1\nline-2");
    }

    #[test]
    fn extract_returns_none_when_no_fence() {
        assert!(extract_managed_block("noop", "aider", aider_legacy()).is_none());
    }

    // --- Bug B: per-adapter fences + legacy migration ----------------------

    #[test]
    fn two_adapters_coexist_in_same_rc() {
        // Apply aider, then apply copilot-cli on top: both blocks
        // should be present and intact. Bug B: pre-fix, the second
        // apply overwrote the first.
        let dir = tempdir().unwrap();
        let zshrc = dir.path().join(".zshrc");
        fs::write(&zshrc, "user content\n").unwrap();

        let aider = fixture_spec();
        let body_a = build_managed_block(&aider, &ApplyOptions::default());
        apply_to_primary_shell_rc(dir.path(), aider.adapter_id, &body_a, aider_legacy()).unwrap();

        let copilot = fixture_spec_two_names();
        let body_c = build_managed_block(&copilot, &ApplyOptions::default());
        apply_to_primary_shell_rc(dir.path(), copilot.adapter_id, &body_c, copilot_legacy()).unwrap();

        let after = fs::read_to_string(&zshrc).unwrap();
        assert!(after.contains("# trove:aider:start"), "aider block missing: {after}");
        assert!(after.contains("# trove:aider:end"));
        assert!(after.contains("aider() { "));
        assert!(after.contains("# trove:copilot-cli:start"));
        assert!(after.contains("# trove:copilot-cli:end"));
        assert!(after.contains("copilot() { "));
        assert!(after.contains("gh-copilot() { "));
    }

    #[test]
    fn strip_block_is_scoped_to_adapter() {
        // After both adapters are applied, reverting aider must leave
        // copilot-cli's block untouched.
        let dir = tempdir().unwrap();
        let zshrc = dir.path().join(".zshrc");
        fs::write(&zshrc, "user content\n").unwrap();

        let aider = fixture_spec();
        let body_a = build_managed_block(&aider, &ApplyOptions::default());
        apply_to_primary_shell_rc(dir.path(), aider.adapter_id, &body_a, aider_legacy()).unwrap();

        let copilot = fixture_spec_two_names();
        let body_c = build_managed_block(&copilot, &ApplyOptions::default());
        apply_to_primary_shell_rc(dir.path(), copilot.adapter_id, &body_c, copilot_legacy()).unwrap();

        revert_primary_shell_rc(dir.path(), "aider", aider_legacy()).unwrap();

        let after = fs::read_to_string(&zshrc).unwrap();
        assert!(!after.contains("# trove:aider:start"));
        assert!(!after.contains("aider() { "));
        assert!(after.contains("# trove:copilot-cli:start"), "copilot block removed: {after}");
        assert!(after.contains("copilot() { "));
    }

    #[test]
    fn legacy_fence_is_migrated_on_next_upsert() {
        // A user on pre-v0.5.1 has an un-namespaced block that defines
        // their `aider` function. On the next apply, upsert must
        // recognize it as ours (function-name match), replace it with
        // the namespaced form, and leave the surrounding content alone.
        let original = "before\n# trove:start\naider() { echo OLD; }\n# trove:end\nafter\n";
        let block = "aider() { echo NEW; }\n";
        let out = upsert_managed_block(original, block, "aider", aider_legacy());
        assert!(out.contains("# trove:aider:start"), "namespaced fence missing: {out}");
        assert!(out.contains("# trove:aider:end"));
        assert!(out.contains("aider() { echo NEW; }"));
        assert!(!out.contains("aider() { echo OLD; }"));
        // No orphan legacy fence should remain.
        assert!(!out.contains("# trove:start\n"));
        assert!(!out.contains("# trove:end\n"));
        assert!(out.starts_with("before\n"));
        assert!(out.ends_with("after\n"));
    }

    #[test]
    fn legacy_fence_with_other_adapters_function_is_not_adopted() {
        // A legacy block defining a different adapter's function must
        // NOT be claimed by ours — leave it where it is and append a
        // fresh namespaced block.
        let original =
            "before\n# trove:start\ncopilot() { echo C; }\n# trove:end\nafter\n";
        let block = "aider() { echo A; }\n";
        let out = upsert_managed_block(original, block, "aider", aider_legacy());
        // Legacy block stays.
        assert!(out.contains("# trove:start\ncopilot() { echo C; }\n# trove:end"));
        // New namespaced aider block appended.
        assert!(out.contains("# trove:aider:start"));
        assert!(out.contains("aider() { echo A; }"));
    }

    #[test]
    fn legacy_body_contains_probe_adopts_matching_block() {
        // An ExportSpec adapter (e.g. Droid) has a pre-namespace block
        // detected via a probe string.
        let original = concat!(
            "before\n",
            "# trove:start\n",
            "export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318\n",
            "# trove:end\n",
            "after\n",
        );
        let block = "export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318\n";
        let legacy = Some(LegacyProbe::BodyContains("OTEL_TELEMETRY_ENDPOINT"));
        let out = upsert_managed_block(original, block, "droid", legacy);
        assert!(out.contains("# trove:droid:start"), "namespaced fence missing: {out}");
        assert!(!out.contains("# trove:start\n"), "legacy fence should be gone");
        assert!(out.starts_with("before\n"));
        assert!(out.ends_with("after\n"));
    }

    #[test]
    fn legacy_body_contains_probe_does_not_adopt_non_matching_block() {
        // A legacy block whose body doesn't contain the probe is left
        // untouched; a fresh namespaced block is appended.
        let original = concat!(
            "before\n",
            "# trove:start\n",
            "aider() { echo aider; }\n",
            "# trove:end\n",
            "after\n",
        );
        let block = "export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318\n";
        let legacy = Some(LegacyProbe::BodyContains("OTEL_TELEMETRY_ENDPOINT"));
        let out = upsert_managed_block(original, block, "droid", legacy);
        // Legacy block (different adapter) stays.
        assert!(out.contains("# trove:start\naider() { echo aider; }"));
        // New namespaced droid block appended.
        assert!(out.contains("# trove:droid:start"));
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
    fn build_managed_block_emits_one_function_def_per_name() {
        // copilot-cli installs both `copilot` (new standalone CLI) and
        // `gh-copilot` (deprecated gh-extension) so users on either
        // path are observed. The block must contain both definitions,
        // each pointing at the same wrapper.
        let spec = fixture_spec_two_names();
        let block = build_managed_block(&spec, &ApplyOptions::default());
        assert!(
            block.contains("copilot() { \"/opt/trove/wrappers/trove-copilot\" \"$@\"; }"),
            "missing `copilot()` definition: {block}",
        );
        assert!(
            block.contains("gh-copilot() { \"/opt/trove/wrappers/trove-copilot\" \"$@\"; }"),
            "missing `gh-copilot()` definition: {block}",
        );
    }

    #[test]
    fn build_export_block_renders_export_lines() {
        let spec = ExportSpec {
            adapter_id: "droid",
            vars: &[("OTEL_TELEMETRY_ENDPOINT", "http://127.0.0.1:4318")],
            legacy_body_probe: None,
        };
        let block = build_export_block(&spec);
        assert_eq!(block, "export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318\n");
    }

    #[test]
    fn apply_then_revert_is_byte_identical_to_original_user_content() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let zshrc = home.join(".zshrc");
        let original = "# user shell rc\nexport PATH=\"$HOME/bin:$PATH\"\n";
        fs::write(&zshrc, original).unwrap();

        let spec = fixture_spec();
        let body = build_managed_block(&spec, &ApplyOptions::default());
        apply_to_primary_shell_rc(home, spec.adapter_id, &body, aider_legacy()).unwrap();
        let after_apply = fs::read_to_string(&zshrc).unwrap();
        assert_ne!(after_apply, original, "apply must change the file");
        assert!(after_apply.contains("# trove:aider:start"));

        revert_primary_shell_rc(home, spec.adapter_id, aider_legacy()).unwrap();
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
        let body = build_managed_block(&spec, &ApplyOptions::default());
        apply_to_primary_shell_rc(home, spec.adapter_id, &body, aider_legacy()).unwrap();
        let after_first = fs::read_to_string(&zshrc).unwrap();
        apply_to_primary_shell_rc(home, spec.adapter_id, &body, aider_legacy()).unwrap();
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
        let body = build_managed_block(&spec, &ApplyOptions::default());
        let preview =
            preview_for_primary_shell_rc(home, spec.adapter_id, &body, aider_legacy()).unwrap();
        assert!(matches!(preview.status, PreviewStatus::Fresh));
    }

    #[test]
    fn preview_status_is_idempotent_after_a_matching_apply() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let zshrc = home.join(".zshrc");
        fs::write(&zshrc, "").unwrap();
        let spec = fixture_spec();
        let body = build_managed_block(&spec, &ApplyOptions::default());
        apply_to_primary_shell_rc(home, spec.adapter_id, &body, aider_legacy()).unwrap();
        let preview =
            preview_for_primary_shell_rc(home, spec.adapter_id, &body, aider_legacy()).unwrap();
        assert!(matches!(preview.status, PreviewStatus::Idempotent));
    }

    #[test]
    fn preview_status_is_conflict_when_user_edits_inside_block() {
        // A namespaced aider block whose body has been hand-edited
        // (e.g. user redirected the wrapper somewhere weird) — the
        // canonical apply would produce something different, so the
        // preview must surface a Conflict.
        let dir = tempdir().unwrap();
        let home = dir.path();
        let zshrc = home.join(".zshrc");
        fs::write(
            &zshrc,
            "before\n# trove:aider:start\naider() { echo HAND_EDIT; }\n# trove:aider:end\nafter\n",
        )
        .unwrap();
        let spec = fixture_spec();
        let body = build_managed_block(&spec, &ApplyOptions::default());
        let preview =
            preview_for_primary_shell_rc(home, spec.adapter_id, &body, aider_legacy()).unwrap();
        assert!(
            matches!(preview.status, PreviewStatus::Conflict),
            "expected Conflict, got {:?}",
            preview.status
        );
    }

    #[test]
    fn apply_errors_when_no_shell_rc_exists() {
        let dir = tempdir().unwrap();
        let spec = fixture_spec();
        let body = build_managed_block(&spec, &ApplyOptions::default());
        let err =
            apply_to_primary_shell_rc(dir.path(), spec.adapter_id, &body, aider_legacy())
                .unwrap_err();
        assert!(matches!(err, IpcError::Internal { .. }));
    }

    #[test]
    fn revert_when_no_rc_or_no_block_is_a_noop() {
        let dir = tempdir().unwrap();
        // no rc file at all
        revert_primary_shell_rc(dir.path(), "aider", aider_legacy()).unwrap();
        let zshrc = dir.path().join(".zshrc");
        std::fs::write(&zshrc, "user-only\n").unwrap();
        // rc exists but no block
        revert_primary_shell_rc(dir.path(), "aider", aider_legacy()).unwrap();
        assert_eq!(fs::read_to_string(&zshrc).unwrap(), "user-only\n");
    }
}
