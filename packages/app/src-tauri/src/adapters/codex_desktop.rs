//! OpenAI Codex desktop app adapter — `/Applications/Codex.app` (bundle
//! id `com.openai.codex`). The desktop app is an Electron shell that
//! delegates to the same Rust `codex app-server` backend the CLI uses,
//! and both read `~/.codex/config.toml` at launch. That shared config
//! means a single `[otel.*]` block instruments both adapters.
//!
//! To let the user enable/disable the CLI and the desktop app
//! independently in the UI — while keeping the TOML valid (no
//! duplicate `[otel.exporter.<kind>]` tables) — both adapters write
//! into one shared managed region whose fence header carries
//! `deps=codex-cli,codex-desktop`.
//! See [`crate::safety::sentinels::comment_fence`] for the
//! reference-counted shared-block protocol: apply adds this adapter's
//! id to the deps list; revert removes it and strips the block only
//! when the last dep is gone.
//!
//! The OTLP payload here is byte-identical to [`super::codex_cli`]'s.
//! No new transform/harness-tag rule is needed in the Collector: the
//! Codex Rust backend always tags emitted signals with `service.name =
//! codex`, so a single existing rule routes them to the codex-cli
//! mappings regardless of which adapter row triggered the install.
//! codex-desktop's row in the dashboard tracks enablement state in
//! state.json rather than waiting for distinct backend telemetry.

use std::path::{Path, PathBuf};

use crate::ipc::IpcError;
use crate::safety::sentinels::{Format, ManagedRegion, SentinelError};

use super::common::{self, HarnessSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

const COLLECTOR_BASE: &str = "http://127.0.0.1:4318";

/// Adapter id used for shared-block dependency tracking in
/// `~/.codex/config.toml`. Coexists with `codex-cli`'s id in the
/// fence header's `deps=` list.
pub(crate) const ADAPTER_ID: &str = "codex-desktop";

const SPEC: HarnessSpec = HarnessSpec {
    adapter_id: ADAPTER_ID,
    config_dir: ".codex",
    config_file: "config.toml",
    format: Format::Toml,
    build_region,
};

/// Resolve the absolute path of the Codex desktop app's config file
/// under `home`. Identical to [`super::codex_cli::config_path`] — both
/// adapters share `~/.codex/config.toml`.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    common::config_path(&SPEC, home)
}

/// Compute the diff between the current file and what an apply with
/// `opts` would write for this adapter.
pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    common::preview(&SPEC, home, opts)
}

/// Apply the patch. The shared-block protocol means apply is
/// `Idempotent` only when the block already lists `codex-desktop` in
/// its `deps=` header; if the block exists but doesn't yet list us
/// (e.g. codex-cli enabled first), apply runs and re-renders the
/// fence with the updated deps list.
pub fn apply(home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    common::apply(&SPEC, home, opts)
}

/// Permissive revert — drops `codex-desktop` from the block's `deps=`
/// list and removes the block entirely when no other adapter still
/// depends on it.
pub fn revert(home: &Path) -> Result<(), IpcError> {
    common::revert(&SPEC, home)
}

/// Build the [`ManagedRegion`] for Codex's `[otel]` block. The payload
/// is identical to [`super::codex_cli`]'s — both adapters write the
/// same exporter slots into the shared region. The fence's `deps=`
/// list distinguishes ownership.
//
// `Result` return matches the JSON adapters' fallible
// `for_json_patches`; `for_text_block` itself is infallible.
#[allow(clippy::unnecessary_wraps)]
fn build_region(_opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    use std::fmt::Write as _;

    let mut payload = String::new();

    // See codex_cli::build_region for the schema-evolution note.
    payload.push_str("[otel.exporter.otlp-http]\n");
    let _ = writeln!(payload, "endpoint = \"{COLLECTOR_BASE}/v1/logs\"");
    payload.push_str("protocol = \"binary\"\n\n");

    payload.push_str("[otel.trace_exporter.otlp-http]\n");
    let _ = writeln!(payload, "endpoint = \"{COLLECTOR_BASE}/v1/traces\"");
    payload.push_str("protocol = \"binary\"\n\n");

    payload.push_str("[otel.metrics_exporter.otlp-http]\n");
    let _ = writeln!(payload, "endpoint = \"{COLLECTOR_BASE}/v1/metrics\"");
    payload.push_str("protocol = \"binary\"\n");

    let keys = vec![
        "otel.exporter.otlp-http".to_string(),
        "otel.trace_exporter.otlp-http".to_string(),
        "otel.metrics_exporter.otlp-http".to_string(),
    ];

    Ok(ManagedRegion::for_text_block(payload, keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{PreviewStatus, codex_cli};
    use std::fs;
    use tempfile::tempdir;

    fn read_config(home: &Path) -> String {
        fs::read_to_string(config_path(home)).unwrap()
    }

    #[test]
    fn fresh_install_creates_a_valid_config_file() {
        let home = tempdir().unwrap();
        let patch = apply(home.path(), &ApplyOptions::default()).unwrap();

        let written = read_config(home.path());
        let doc: toml_edit::DocumentMut = written.parse().unwrap();
        let exporter = doc["otel"]["exporter"]["otlp-http"].as_table().unwrap();
        assert_eq!(
            exporter["endpoint"].as_str(),
            Some("http://127.0.0.1:4318/v1/logs")
        );
        assert!(written.contains("# trove:start"));
        assert!(written.contains("deps=codex-desktop"));
        assert_eq!(patch.format, Format::Toml);
    }

    #[test]
    fn idempotent_reapply_does_not_change_the_file() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_first = read_config(home.path());
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_second = read_config(home.path());
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn config_path_matches_codex_cli() {
        let home = tempdir().unwrap();
        assert_eq!(config_path(home.path()), codex_cli::config_path(home.path()));
    }

    #[test]
    fn revert_strips_block_when_only_dep() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        revert(home.path()).unwrap();
        let after = read_config(home.path());
        assert!(!after.contains("# trove:start"));
    }

    #[test]
    fn coexists_with_codex_cli_in_one_config_toml() {
        // Apply codex-cli first; then codex-desktop. Both rows live in
        // the same fenced block via dep-tracking.
        let home = tempdir().unwrap();
        codex_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_cli = read_config(home.path());
        assert!(after_cli.contains("deps=codex-cli"));
        assert!(!after_cli.contains("codex-desktop"));

        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after_both = read_config(home.path());
        assert!(after_both.contains("deps=codex-cli,codex-desktop"));

        // Payload is unchanged between the two applies — only the deps
        // list grows. The block appears once.
        assert_eq!(after_both.matches("[otel.exporter.otlp-http]").count(), 1);
    }

    #[test]
    fn user_keys_outside_block_survive_apply() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[user]\nname = \"jeff\"\n",
        )
        .unwrap();

        apply(home.path(), &ApplyOptions::default()).unwrap();
        let after = read_config(home.path());
        let doc: toml_edit::DocumentMut = after.parse().unwrap();
        assert_eq!(doc["user"]["name"].as_str(), Some("jeff"));
    }

    #[test]
    fn editing_inside_the_managed_block_yields_conflict() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();

        let path = config_path(home.path());
        let written = read_config(home.path());
        let edited = written.replace(
            "http://127.0.0.1:4318/v1/logs",
            "http://attacker.example.com/v1/logs",
        );
        fs::write(&path, &edited).unwrap();

        let result = apply(home.path(), &ApplyOptions::default());
        match result {
            Err(IpcError::RegionConflict { path: p }) => {
                assert_eq!(p, path.display().to_string());
            }
            other => panic!("expected RegionConflict, got {other:?}"),
        }
    }

    #[test]
    fn malformed_file_is_unparseable_error() {
        let home = tempdir().unwrap();
        let path = config_path(home.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not = toml [unclosed").unwrap();

        let err = apply(home.path(), &ApplyOptions::default()).unwrap_err();
        assert!(matches!(err, IpcError::ConfigUnparseable { .. }));
    }

    #[test]
    fn preview_on_missing_file_returns_fresh_status() {
        let home = tempdir().unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Fresh);
        assert!(preview.after.contains("[otel.exporter.otlp-http]"));
    }

    #[test]
    fn preview_after_apply_returns_idempotent_status() {
        let home = tempdir().unwrap();
        apply(home.path(), &ApplyOptions::default()).unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Idempotent);
    }

    #[test]
    fn preview_when_block_lacks_our_dep_returns_fresh() {
        // codex-cli has applied; codex-desktop preview should classify
        // as Fresh because the dep list doesn't yet include us — apply
        // will mutate the deps even though the payload matches.
        let home = tempdir().unwrap();
        codex_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
        let preview = preview(home.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(preview.status, PreviewStatus::Fresh);
    }
}
