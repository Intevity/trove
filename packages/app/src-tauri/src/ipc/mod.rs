//! Tauri command surface and the wire-format error type.
//!
//! Every command returns `Result<T, IpcError>` where `IpcError` is
//! serialized as a tagged JSON discriminated union. The React side has
//! a Zod twin in `packages/shared/src/schemas.ts` so `kind`-based
//! branching stays type-safe across the wire.

pub mod collector_status;
pub mod commands;
pub mod test_export;

use serde::Serialize;
use thiserror::Error;

use crate::harness::HarnessId;

/// All errors the IPC layer surfaces to the UI. Discriminated by `kind`
/// in JSON; the TS side has a matching Zod union.
#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum IpcError {
    /// Host config file exists but cannot be parsed in its expected
    /// format. The UI shows the `reason` and disables apply.
    #[error("config at {path} could not be parsed: {reason}")]
    ConfigUnparseable { path: String, reason: String },

    /// A managed Trove region exists in the host file but its hash
    /// differs from the region we'd write — typically because the user
    /// edited inside the block by hand. Sprint 8 will replace the
    /// surface with a 3-way merge UI; Sprint 3 just refuses.
    #[error("existing managed region in {path} differs from the new patch; refusing to overwrite")]
    RegionConflict { path: String },

    /// Caller asked for a harness that wasn't detected on this machine.
    #[error("harness {id:?} was not detected on this machine")]
    HarnessNotDetected { id: HarnessId },

    /// Caller asked for a harness whose adapter is not yet implemented
    /// (codex-cli / qwen-code in Sprint 3 — those land in Sprint 4).
    #[error("harness {id:?} does not yet have an adapter implementation")]
    HarnessNotImplemented { id: HarnessId },

    /// Generic filesystem failure. `path` may be empty when no specific
    /// file is involved.
    #[error("io error at {path}: {reason}")]
    Io { path: String, reason: String },

    /// Catch-all for unexpected failures the UI should treat as a bug.
    #[error("internal error: {reason}")]
    Internal { reason: String },
}

impl From<crate::secrets::SecretsError> for IpcError {
    fn from(value: crate::secrets::SecretsError) -> Self {
        // Keychain operations don't carry a meaningful filesystem path —
        // map to Internal with the underlying message preserved. The
        // wizard surfaces these distinctly via the `kind` discriminator
        // on the TS side.
        IpcError::Internal {
            reason: format!("keychain: {value}"),
        }
    }
}

impl From<crate::app_state::AppStateError> for IpcError {
    fn from(value: crate::app_state::AppStateError) -> Self {
        match value {
            crate::app_state::AppStateError::Io { path, source } => IpcError::Io {
                path: path.display().to_string(),
                reason: source.to_string(),
            },
            other => IpcError::Internal {
                reason: other.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_unparseable_serializes_with_kind_and_camel_paths() {
        let err = IpcError::ConfigUnparseable {
            path: "/tmp/x".into(),
            reason: "expected `:`".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"config-unparseable\""));
        assert!(json.contains("\"path\":\"/tmp/x\""));
        assert!(json.contains("\"reason\":\"expected `:`\""));
    }

    #[test]
    fn region_conflict_carries_path_for_sprint_8() {
        let err = IpcError::RegionConflict {
            path: "/home/me/.claude/settings.json".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"region-conflict\""));
        assert!(json.contains("/home/me/.claude/settings.json"));
    }

    #[test]
    fn harness_not_implemented_includes_id() {
        let err = IpcError::HarnessNotImplemented {
            id: HarnessId::CodexCli,
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"harness-not-implemented\""));
        assert!(json.contains("\"id\":\"codex-cli\""));
    }

    #[test]
    fn io_error_carries_both_fields() {
        let err = IpcError::Io {
            path: "/tmp/x".into(),
            reason: "permission denied".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"io\""));
        assert!(json.contains("\"path\":\"/tmp/x\""));
    }

    #[test]
    fn internal_error_serializes() {
        let err = IpcError::Internal {
            reason: "unexpected".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"kind\":\"internal\""));
    }

    #[test]
    fn display_messages_are_useful() {
        let err = IpcError::HarnessNotDetected {
            id: HarnessId::ClaudeCode,
        };
        let msg = format!("{err}");
        assert!(msg.contains("ClaudeCode"));
    }
}
