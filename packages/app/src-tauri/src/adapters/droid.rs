//! Droid (factory.ai CLI) Tier 1 adapter.
//!
//! Droid emits OTLP metrics natively when `OTEL_TELEMETRY_ENDPOINT` is set.
//! This adapter writes that single env-var export into the user's primary
//! shell RC (`~/.zshrc`, `~/.bashrc`, `~/.config/fish/config.fish`).
//!
//! ## What we write
//!
//! Only `OTEL_TELEMETRY_ENDPOINT`. We do **not** write:
//!
//! * `OTEL_RESOURCE_ATTRIBUTES` — factory.ai's SDK ignores it.  The docs
//!   list zero configurable resource attributes; the value never appears in
//!   OTLP payloads even when the variable is present in the process
//!   environment.  Writing it is harmless but wasteful.
//!
//! * `OTEL_TELEMETRY_HEADERS` — these are transport-level HTTP auth
//!   headers.  The local Trove collector needs no auth.  If a user routes
//!   to a remote auth-gated collector they can add this variable manually.

use std::path::{Path, PathBuf};

use super::wrapper_common::{
    ExportSpec, LegacyProbe, apply_to_primary_shell_rc, build_export_block,
    preview_for_primary_shell_rc, primary_shell_rc, revert_primary_shell_rc,
};
use super::{ApplyOptions, PatchPreview, TrovePatch};
use crate::ipc::IpcError;

const SPEC: ExportSpec = ExportSpec {
    adapter_id: "droid",
    vars: &[("OTEL_TELEMETRY_ENDPOINT", "http://127.0.0.1:4318")],
    legacy_body_probe: Some("OTEL_TELEMETRY_ENDPOINT"),
};

/// Path to the file that Trove patches for this adapter.
///
/// Droid's adapter patches the primary shell RC (not a harness config
/// file), so this returns the shell RC path for display purposes.
/// Falls back to `~/.zshrc` when no shell RC file exists yet.
#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    primary_shell_rc(home).unwrap_or_else(|| home.join(".zshrc"))
}

pub fn preview(home: &Path, _opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    let body = build_export_block(&SPEC);
    preview_for_primary_shell_rc(
        home,
        SPEC.adapter_id,
        &body,
        SPEC.legacy_body_probe.map(LegacyProbe::BodyContains),
    )
}

pub fn apply(home: &Path, _opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    let body = build_export_block(&SPEC);
    apply_to_primary_shell_rc(
        home,
        SPEC.adapter_id,
        &body,
        SPEC.legacy_body_probe.map(LegacyProbe::BodyContains),
    )
}

pub fn revert(home: &Path) -> Result<(), IpcError> {
    revert_primary_shell_rc(
        home,
        SPEC.adapter_id,
        SPEC.legacy_body_probe.map(LegacyProbe::BodyContains),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_zshrc(home: &Path, content: &str) {
        let path = home.join(".zshrc");
        fs::write(&path, content).unwrap();
    }

    fn read_zshrc(home: &Path) -> String {
        fs::read_to_string(home.join(".zshrc")).unwrap()
    }

    const ORIGINAL: &str = "# ~/.zshrc\nexport PATH=\"$HOME/.local/bin:$PATH\"\n";

    fn opts() -> ApplyOptions {
        ApplyOptions::default()
    }

    #[test]
    fn apply_writes_export_line_to_shell_rc() {
        let dir = tempdir().unwrap();
        write_zshrc(dir.path(), ORIGINAL);
        apply(dir.path(), &opts()).unwrap();
        let rc = read_zshrc(dir.path());
        assert!(rc.contains("# trove:droid:start"), "fence start missing");
        assert!(rc.contains("# trove:droid:end"), "fence end missing");
        assert!(
            rc.contains("export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318"),
            "endpoint var missing"
        );
    }

    #[test]
    fn apply_is_idempotent() {
        let dir = tempdir().unwrap();
        write_zshrc(dir.path(), ORIGINAL);
        apply(dir.path(), &opts()).unwrap();
        let after_first = read_zshrc(dir.path());
        apply(dir.path(), &opts()).unwrap();
        let after_second = read_zshrc(dir.path());
        assert_eq!(after_first, after_second, "second apply should not change the file");
    }

    #[test]
    fn revert_removes_export_block() {
        let dir = tempdir().unwrap();
        write_zshrc(dir.path(), ORIGINAL);
        apply(dir.path(), &opts()).unwrap();
        revert(dir.path()).unwrap();
        let rc = read_zshrc(dir.path());
        assert!(
            !rc.contains("# trove:droid:start"),
            "fence should be gone after revert"
        );
        assert!(
            !rc.contains("OTEL_TELEMETRY_ENDPOINT"),
            "export var should be gone after revert"
        );
    }

    #[test]
    fn revert_is_noop_when_not_applied() {
        let dir = tempdir().unwrap();
        write_zshrc(dir.path(), ORIGINAL);
        revert(dir.path()).unwrap();
        assert_eq!(read_zshrc(dir.path()), ORIGINAL);
    }

    #[test]
    fn apply_then_revert_is_byte_identical() {
        let dir = tempdir().unwrap();
        write_zshrc(dir.path(), ORIGINAL);
        apply(dir.path(), &opts()).unwrap();
        revert(dir.path()).unwrap();
        assert_eq!(
            read_zshrc(dir.path()),
            ORIGINAL,
            "apply+revert must be byte-identical to original"
        );
    }

    #[test]
    fn migrates_legacy_fence_on_first_apply() {
        const LEGACY_ZSHRC: &str = concat!(
            "# ~/.zshrc\n",
            "export PATH=\"$HOME/.local/bin:$PATH\"\n",
            "\n",
            "# trove:start\n",
            "export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318\n",
            "export OTEL_RESOURCE_ATTRIBUTES=harness.id=droid,harness.name=Droid,service.name=droid\n",
            "# trove:end\n",
        );
        let dir = tempdir().unwrap();
        write_zshrc(dir.path(), LEGACY_ZSHRC);
        apply(dir.path(), &opts()).unwrap();
        let rc = read_zshrc(dir.path());
        assert!(rc.contains("# trove:droid:start"), "namespaced fence should be present");
        assert!(!rc.contains("# trove:start\n"), "legacy fence start should be gone");
        assert!(!rc.contains("# trove:end\n"), "legacy fence end should be gone");
        assert!(!rc.contains("OTEL_RESOURCE_ATTRIBUTES"), "resource attrs should be dropped");
        assert!(rc.contains("OTEL_TELEMETRY_ENDPOINT"), "endpoint var should still be present");
        assert!(rc.contains("export PATH="), "user's PATH export should be preserved");
    }

    #[test]
    fn does_not_write_otel_resource_attributes() {
        let dir = tempdir().unwrap();
        write_zshrc(dir.path(), ORIGINAL);
        apply(dir.path(), &opts()).unwrap();
        let rc = read_zshrc(dir.path());
        assert!(
            !rc.contains("OTEL_RESOURCE_ATTRIBUTES"),
            "OTEL_RESOURCE_ATTRIBUTES must never be written by the droid adapter"
        );
    }
}
