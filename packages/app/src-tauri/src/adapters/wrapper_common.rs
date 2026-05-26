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

/// Per-adapter fence start marker. Each wrapper adapter writes a block
/// fenced with its own id so multiple wrapper adapters (aider,
/// copilot-cli, cursor-cli) can coexist in the same shell rc.
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
    let new_content = upsert_managed_block(&current, &block, spec.adapter_id, spec.function_names);

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
        // Shell rc shares the # comment fence with YAML/TOML but isn't
        // a parseable document, so the conflict-payload path must use
        // the Shell branch of the sentinels engine (no YAML validate).
        format: Format::Shell,
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
    let new_content =
        upsert_managed_block(&current, &proposed, spec.adapter_id, spec.function_names);

    let status =
        if let Some(existing) = extract_managed_block(&current, spec.adapter_id, spec.function_names)
        {
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
        format: Format::Shell,
        before: current,
        after: new_content,
        status,
    })
}

/// Strip Trove's managed block for one adapter from the primary shell
/// rc file. No-op when no block is present for `adapter_id`. Per-adapter
/// scoping means reverting (say) aider leaves an enabled copilot-cli's
/// block intact. Also migrates a legacy un-namespaced block that
/// belongs to this adapter (detected by `function_names` matching the
/// shell-function definition inside the fenced body), so a user who
/// last enabled the wrapper on a pre-v0.5.1 build can still cleanly
/// disable it without leaving an orphan fence behind.
pub fn revert_primary_shell_rc(
    home: &Path,
    adapter_id: &str,
    function_names: &[&'static str],
) -> Result<(), IpcError> {
    let Some(path) = primary_shell_rc(home) else {
        return Ok(());
    };
    let Ok(current) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    if extract_managed_block(&current, adapter_id, function_names).is_none() {
        return Ok(());
    }
    let stripped = strip_managed_block(&current, adapter_id, function_names);
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
/// wrapper adapters coexist. When `function_names` is provided AND no
/// namespaced fence is present but a legacy un-namespaced block exists
/// whose body defines a function from `function_names`, the legacy
/// block is migrated in place (replaced with the namespaced form).
/// Idempotent.
#[must_use]
pub fn upsert_managed_block(
    content: &str,
    block: &str,
    adapter_id: &str,
    function_names: &[&'static str],
) -> String {
    if let Some((start, end)) = locate_block(content, adapter_id, function_names) {
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
/// apply-then-revert sequence is byte-identical to the original. Other
/// adapters' blocks are untouched.
#[must_use]
pub fn strip_managed_block(
    content: &str,
    adapter_id: &str,
    function_names: &[&'static str],
) -> String {
    let Some((start, end)) = locate_block(content, adapter_id, function_names) else {
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
/// excluding the markers themselves) for `adapter_id` if present. Also
/// matches a legacy un-namespaced block whose body defines a function
/// from `function_names`, so a pre-v0.5.1 enable is still detected as
/// "already applied" for status-classification purposes.
#[must_use]
pub fn extract_managed_block(
    content: &str,
    adapter_id: &str,
    function_names: &[&'static str],
) -> Option<String> {
    let (start, end) = locate_block(content, adapter_id, function_names)?;
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
/// including the fence start/end lines themselves. Tries the
/// namespaced fence first; falls back to a legacy un-namespaced block
/// whose body defines a function from `function_names` (so a
/// pre-v0.5.1 block can be migrated in place by [`upsert_managed_block`]).
/// Returns `None` when no matching block is found.
fn locate_block(
    content: &str,
    adapter_id: &str,
    function_names: &[&'static str],
) -> Option<(usize, usize)> {
    if let Some(span) = locate_fence_pair(content, &fence_start(adapter_id), &fence_end(adapter_id))
    {
        return Some(span);
    }
    // Legacy un-namespaced fence — only adopt it if its body looks like
    // a shell-function definition for one of our names. Otherwise it's
    // a different adapter's legacy block and we mustn't touch it.
    let legacy = locate_fence_pair(content, LEGACY_FENCE_START, LEGACY_FENCE_END)?;
    if function_names.is_empty() {
        return None;
    }
    let body = &content[legacy.0..legacy.1];
    let owns_block = function_names
        .iter()
        .any(|name| body.contains(&format!("{name}() {{")));
    if owns_block { Some(legacy) } else { None }
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

// ---------------------------------------------------------------------------
// ExportSpec — plain `export KEY=VALUE` shell-rc adapters
// ---------------------------------------------------------------------------
//
// Some harnesses (e.g. Droid) are configured exclusively via environment
// variables; they need no shell-function wrapper, just a line like:
//
//   export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318
//
// `ExportSpec` drives a parallel set of public functions that follow the
// same atomic-write + backup + fence pattern as the `WrapperSpec` trio,
// but render `export K=V` lines instead of function definitions.
//
// Legacy migration: if the user has a pre-namespace `# trove:start` block
// whose body contains `legacy_body_probe`, the first `apply` replaces it
// in-place with the namespaced fence — dropping any vars that the new
// adapter intentionally does not write (e.g. `OTEL_RESOURCE_ATTRIBUTES`).

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

/// Render the managed export block for `spec` — one `export K=V\n` per var.
#[must_use]
pub fn build_export_block(spec: &ExportSpec) -> String {
    let mut out = String::new();
    for (k, v) in spec.vars {
        let _ = writeln!(out, "export {k}={v}");
    }
    out
}

/// Insert or replace this adapter's managed export block in `content`.
/// Uses `locate_export_block` so a matching legacy block is migrated.
#[must_use]
fn upsert_export_block(content: &str, block: &str, spec: &ExportSpec) -> String {
    if let Some((start, end)) = locate_export_block(content, spec) {
        let mut out = String::with_capacity(content.len() + block.len());
        out.push_str(&content[..start]);
        out.push_str(&render_fenced(block, spec.adapter_id));
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
        out.push_str(&render_fenced(block, spec.adapter_id));
        out
    }
}

/// Apply Trove's managed export block to the primary shell rc file.
/// Atomic write + backup. Returns the resulting `TrovePatch` (or
/// `IpcError::Internal` if no shell rc exists).
pub fn apply_export_to_primary_shell_rc(
    home: &Path,
    spec: &ExportSpec,
    _opts: &ApplyOptions,
) -> Result<TrovePatch, IpcError> {
    let path = primary_shell_rc(home).ok_or_else(|| IpcError::Internal {
        reason: "no shell rc file (~/.zshrc, ~/.bashrc, fish/config.fish) exists; create one before enabling".into(),
    })?;
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let block = build_export_block(spec);
    let new_content = upsert_export_block(&current, &block, spec);

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
        format: Format::Shell,
        last_written_region_payload: block,
    })
}

/// Compute the diff for `apply_export_to_primary_shell_rc` without writing.
/// Status: `Idempotent` if the existing block is byte-identical to the
/// proposed block; `Conflict` if a Trove block is present with different
/// bytes; `Fresh` otherwise.
pub fn preview_export_for_primary_shell_rc(
    home: &Path,
    spec: &ExportSpec,
    _opts: &ApplyOptions,
) -> Result<PatchPreview, IpcError> {
    let path = primary_shell_rc(home).ok_or_else(|| IpcError::Internal {
        reason: "no shell rc file exists; create ~/.zshrc or ~/.bashrc before enabling".into(),
    })?;
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let proposed = build_export_block(spec);
    let new_content = upsert_export_block(&current, &proposed, spec);

    let status = if let Some(existing) = extract_export_block(&current, spec) {
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
        format: Format::Shell,
        before: current,
        after: new_content,
        status,
    })
}

/// Strip Trove's managed export block for one adapter from the primary
/// shell rc file. No-op when no block is present. Also strips a matching
/// legacy un-namespaced block via `locate_export_block`.
pub fn revert_export_primary_shell_rc(home: &Path, spec: &ExportSpec) -> Result<(), IpcError> {
    let Some(path) = primary_shell_rc(home) else {
        return Ok(());
    };
    let Ok(current) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    if locate_export_block(&current, spec).is_none() {
        return Ok(());
    }
    let stripped = strip_export_block(&current, spec);
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
// ExportSpec private helpers
// ---------------------------------------------------------------------------

/// Return the inner export block text for `spec` if present.
#[must_use]
fn extract_export_block(content: &str, spec: &ExportSpec) -> Option<String> {
    let (start, end) = locate_export_block(content, spec)?;
    let block = &content[start..end];
    let mut lines: Vec<&str> = block.lines().collect();
    let opens = lines.first().is_some_and(|l| {
        let t = l.trim_start();
        t.starts_with(&fence_start(spec.adapter_id)) || t.starts_with(LEGACY_FENCE_START)
    });
    let closes = lines.last().is_some_and(|l| {
        let t = l.trim_start();
        t.starts_with(&fence_end(spec.adapter_id)) || t.starts_with(LEGACY_FENCE_END)
    });
    if opens && closes {
        lines.remove(0);
        lines.pop();
    }
    Some(lines.join("\n"))
}

/// Strip this adapter's managed export block from `content`. Mirrors
/// `strip_managed_block` but uses `locate_export_block`.
#[must_use]
fn strip_export_block(content: &str, spec: &ExportSpec) -> String {
    let Some((start, end)) = locate_export_block(content, spec) else {
        return content.to_string();
    };
    let prefix = &content[..start];
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

/// Locate the byte range covering this adapter's managed export block,
/// including the fence start/end lines. Tries the namespaced fence first;
/// falls back to a legacy un-namespaced block whose body contains
/// `spec.legacy_body_probe` (when `Some`).
fn locate_export_block(content: &str, spec: &ExportSpec) -> Option<(usize, usize)> {
    if let Some(span) =
        locate_fence_pair(content, &fence_start(spec.adapter_id), &fence_end(spec.adapter_id))
    {
        return Some(span);
    }
    // Legacy block adoption: adopt a pre-namespace block only when its
    // body contains the probe string (confirming it belongs to this adapter).
    let probe = spec.legacy_body_probe?;
    let legacy = locate_fence_pair(content, LEGACY_FENCE_START, LEGACY_FENCE_END)?;
    let body = &content[legacy.0..legacy.1];
    if body.contains(probe) { Some(legacy) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    const AIDER_NAMES: &[&str] = &["aider"];
    const COPILOT_NAMES: &[&str] = &["copilot", "gh-copilot"];

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
        let out = upsert_managed_block(original, block, "aider", AIDER_NAMES);
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
        let out = upsert_managed_block(original, block, "aider", AIDER_NAMES);
        assert!(out.contains("new"));
        assert!(!out.contains("old"));
        assert!(out.starts_with("before\n"));
        assert!(out.ends_with("after\n"));
    }

    #[test]
    fn strip_block_removes_fence_and_body() {
        let original =
            "before\n# trove:aider:start\nbody\n# trove:aider:end\nafter\n";
        let stripped = strip_managed_block(original, "aider", AIDER_NAMES);
        assert_eq!(stripped, "before\nafter\n");
    }

    #[test]
    fn strip_block_is_noop_when_absent() {
        let original = "user-only-content\n";
        assert_eq!(
            strip_managed_block(original, "aider", AIDER_NAMES),
            original
        );
    }

    #[test]
    fn extract_returns_inner_block_text() {
        let original =
            "before\n# trove:aider:start\nline-1\nline-2\n# trove:aider:end\nafter\n";
        let inner = extract_managed_block(original, "aider", AIDER_NAMES).unwrap();
        assert_eq!(inner, "line-1\nline-2");
    }

    #[test]
    fn extract_returns_none_when_no_fence() {
        assert!(extract_managed_block("noop", "aider", AIDER_NAMES).is_none());
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

        apply_to_primary_shell_rc(
            dir.path(),
            &fixture_spec(),
            &ApplyOptions::default(),
        )
        .unwrap();
        apply_to_primary_shell_rc(
            dir.path(),
            &fixture_spec_two_names(),
            &ApplyOptions::default(),
        )
        .unwrap();

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
        apply_to_primary_shell_rc(
            dir.path(),
            &fixture_spec(),
            &ApplyOptions::default(),
        )
        .unwrap();
        apply_to_primary_shell_rc(
            dir.path(),
            &fixture_spec_two_names(),
            &ApplyOptions::default(),
        )
        .unwrap();

        revert_primary_shell_rc(dir.path(), "aider", AIDER_NAMES).unwrap();

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
        let out = upsert_managed_block(original, block, "aider", AIDER_NAMES);
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
        let out = upsert_managed_block(original, block, "aider", AIDER_NAMES);
        // Legacy block stays.
        assert!(out.contains("# trove:start\ncopilot() { echo C; }\n# trove:end"));
        // New namespaced aider block appended.
        assert!(out.contains("# trove:aider:start"));
        assert!(out.contains("aider() { echo A; }"));
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
        assert!(after_apply.contains("# trove:aider:start"));

        revert_primary_shell_rc(home, spec.adapter_id, spec.function_names).unwrap();
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
        let preview = preview_for_primary_shell_rc(home, &spec, &ApplyOptions::default()).unwrap();
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
        let err =
            apply_to_primary_shell_rc(dir.path(), &spec, &ApplyOptions::default()).unwrap_err();
        assert!(matches!(err, IpcError::Internal { .. }));
    }

    #[test]
    fn revert_when_no_rc_or_no_block_is_a_noop() {
        let dir = tempdir().unwrap();
        // no rc file at all
        revert_primary_shell_rc(dir.path(), "aider", AIDER_NAMES).unwrap();
        let zshrc = dir.path().join(".zshrc");
        std::fs::write(&zshrc, "user-only\n").unwrap();
        // rc exists but no block
        revert_primary_shell_rc(dir.path(), "aider", AIDER_NAMES).unwrap();
        assert_eq!(fs::read_to_string(&zshrc).unwrap(), "user-only\n");
    }
}
