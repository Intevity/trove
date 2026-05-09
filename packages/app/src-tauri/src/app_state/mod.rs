//! Persisted application state.
//!
//! Loads/saves the user's chosen backend and per-harness records to
//! `state.json` in the platform's config directory (XDG on Linux,
//! `~/Library/Application Support/com.intevity.trove/` on macOS,
//! `%APPDATA%\com.intevity.trove\` on Windows). Atomic writes via
//! [`crate::safety::atomic::write_atomic`].
//!
//! The Rust types in this module mirror the Zod schemas in
//! `packages/shared/src/schemas.ts` byte-for-byte on the wire (camelCase
//! field names, `kind`-tagged discriminated union for [`Backend`]).
//! Diverging the schemas silently breaks the React side at runtime, so
//! the schemas test in `packages/shared/src/schemas.test.ts` is the
//! cross-language anchor.
//!
//! ## Migration scaffold
//!
//! [`load_from_dir`] reads only `schemaVersion` from the file before
//! parsing the rest. The current schema is version `4`. Older versions
//! are migrated in-place by relying on `#[serde(default)]` for fields
//! introduced in later schemas:
//! - v2 → v4: `TrovePatch.lastWrittenRegionPayload` (Sprint 8) defaults
//!   to `""`; `AppState.autoUpdateEnabled` (Sprint 10) defaults to
//!   `false`.
//! - v3 → v4: `AppState.autoUpdateEnabled` defaults to `false`.
//!
//! After parse, the loader re-stamps
//! `schema_version = CURRENT_SCHEMA_VERSION` on the in-memory value so
//! the next save persists v4 to disk. v1 was never written in the wild
//! and remains an explicit error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use thiserror::Error;

use crate::adapters::{ApplyOptions, TrovePatch};
use crate::harness::HarnessId;
use crate::safety::atomic::write_atomic;

/// Filename inside the app config directory.
pub const STATE_FILENAME: &str = "state.json";

/// Current schema version. Bumped any time the persisted shape changes.
/// See module docs for the migration scaffold.
pub const CURRENT_SCHEMA_VERSION: u32 = 4;

/// Opaque keychain handle. The actual secret never leaves the OS
/// keychain. Mirrors `SecretRef` in `packages/shared/src/schemas.ts`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRef {
    pub service: String,
    pub account: String,
}

impl SecretRef {
    /// Build a `SecretRef` from a keychain account string. The service
    /// is always [`crate::secrets::SERVICE`].
    #[must_use]
    pub fn for_account(account: impl Into<String>) -> Self {
        Self {
            service: crate::secrets::SERVICE.to_string(),
            account: account.into(),
        }
    }
}

/// Wire-protocol over which the OTLP-generic backend talks to the user's
/// upstream. Mirrors `BackendOtlpProtocol` in the TS Zod union.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OtlpProtocol {
    Grpc,
    Http,
}

/// Persisted backend record. Secret-bearing fields are [`SecretRef`]
/// handles only. Mirrors the `Backend` Zod discriminated union.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Backend {
    #[serde(rename_all = "camelCase")]
    Signoz {
        region: String,
        ingestion_key: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Honeycomb {
        team: SecretRef,
        dataset: String,
    },
    #[serde(rename_all = "camelCase")]
    GrafanaCloud {
        endpoint: String,
        auth: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Datadog {
        site: String,
        api_key: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    OtlpGeneric {
        endpoint: String,
        protocol: OtlpProtocol,
        /// Header name -> [`SecretRef`]. The set of header names is part
        /// of the persisted state so [`crate::secrets::delete`] can
        /// iterate them when clearing the backend.
        headers: BTreeMap<String, SecretRef>,
    },
    #[serde(rename_all = "camelCase")]
    OtelcolPassthrough { endpoint: String },
}

/// Wire-format draft of [`Backend`] with raw secret values inline.
/// Used **only** for the `save_backend` IPC payload — never persisted.
/// The IPC handler stores each secret in the keychain, replaces it with
/// a [`SecretRef`], and writes the resulting [`Backend`] to state.json.
///
/// Defining the raw-secret schema explicitly (rather than reusing
/// [`Backend`] with a generic) keeps the IPC boundary unmistakable: any
/// call site that constructs or matches `BackendDraft` is by definition
/// holding a raw secret in memory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BackendDraft {
    #[serde(rename_all = "camelCase")]
    Signoz {
        region: String,
        ingestion_key: String,
    },
    #[serde(rename_all = "camelCase")]
    Honeycomb { team: String, dataset: String },
    #[serde(rename_all = "camelCase")]
    GrafanaCloud { endpoint: String, auth: String },
    #[serde(rename_all = "camelCase")]
    Datadog { site: String, api_key: String },
    #[serde(rename_all = "camelCase")]
    OtlpGeneric {
        endpoint: String,
        protocol: OtlpProtocol,
        headers: BTreeMap<String, String>,
    },
    #[serde(rename_all = "camelCase")]
    OtelcolPassthrough { endpoint: String },
}

/// What Trove records about each harness it has touched. Mirrors the
/// Zod `HarnessConfig`. Reuses [`ApplyOptions`] for the `options` inline
/// object since the shape and serde representation are identical.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessConfig {
    pub id: HarnessId,
    pub enabled: bool,
    pub config_path: String,
    /// RFC3339 timestamp of the last successful apply. Updated on every
    /// re-apply (idempotent or otherwise).
    pub last_patched_at: String,
    pub trove_patch: TrovePatch,
    pub options: ApplyOptions,
}

/// Persisted application state. Secrets are referenced via [`SecretRef`]
/// only. Mirrors the Zod `AppState` schema.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    /// Pinned to [`CURRENT_SCHEMA_VERSION`] (currently `4`). Older or
    /// newer values surface as [`AppStateError::UnknownSchemaVersion`].
    pub schema_version: u32,
    pub backend: Option<Backend>,
    pub harnesses: Vec<HarnessConfig>,
    /// Sprint 10 — opt-in auto-updater. Default `false`; flipped via the
    /// `set_auto_update_enabled` IPC command. Gates only the
    /// background-on-launch update probe; the user-facing
    /// `check_for_updates` IPC command is an explicit action and runs
    /// regardless. Trove never contacts GitHub Releases without either
    /// this flag set, or a click in the UI.
    #[serde(default)]
    pub auto_update_enabled: bool,
}

impl Default for AppState {
    /// The state a fresh launch sees when no `state.json` exists yet.
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            backend: None,
            harnesses: Vec::new(),
            auto_update_enabled: false,
        }
    }
}

/// Failures from the persistence layer. Mapped to [`crate::ipc::IpcError`]
/// at the IPC boundary.
#[derive(Debug, Error)]
pub enum AppStateError {
    #[error("io at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("state.json parse failure: {reason}")]
    Parse { reason: String },
    #[error("unknown state.json schemaVersion {0}; expected {CURRENT_SCHEMA_VERSION}")]
    UnknownSchemaVersion(u32),
}

/// Minimal preamble we read before a full parse, so [`load_from_dir`]
/// can decide whether to migrate or to fail with a versioned error.
#[derive(Deserialize)]
struct VersionPreamble {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
}

/// Locate `state.json` inside `config_dir`.
#[must_use]
pub fn state_path_in(config_dir: &Path) -> PathBuf {
    config_dir.join(STATE_FILENAME)
}

/// Load app state from `config_dir`. Returns [`AppState::default`] when
/// no file exists yet (first launch). Surfaces parse errors and unknown
/// schema versions verbatim — never silently returns the default on a
/// real failure.
pub fn load_from_dir(config_dir: &Path) -> Result<AppState, AppStateError> {
    let path = state_path_in(config_dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppState::default());
        }
        Err(e) => return Err(AppStateError::Io { path, source: e }),
    };

    let preamble: VersionPreamble =
        serde_json::from_slice(&bytes).map_err(|e| AppStateError::Parse {
            reason: format!("preamble: {e}"),
        })?;

    match preamble.schema_version {
        // v2 → v4 / v3 → v4 migration. Sprint 8 added
        // `TrovePatch.lastWrittenRegionPayload` (defaults to ""); Sprint
        // 10 added `AppState.autoUpdateEnabled` (defaults to false). Both
        // use `#[serde(default)]`, so older documents deserialize cleanly
        // into the v4 in-memory shape with sensible defaults. We re-stamp
        // schema_version on the returned struct so the next save persists
        // v4 to disk and the matching loader hits the v4 branch from then
        // on.
        2..=4 => {
            let mut state: AppState =
                serde_json::from_slice(&bytes).map_err(|e| AppStateError::Parse {
                    reason: e.to_string(),
                })?;
            state.schema_version = CURRENT_SCHEMA_VERSION;
            Ok(state)
        }
        // v1 was never persisted in the wild — Sprint 4 bumped the shape
        // mid-development, before state.json was ever written by Trove.
        // Hitting this branch means the user hand-crafted a v1 file, which
        // we treat as a real bug they should escalate.
        v => Err(AppStateError::UnknownSchemaVersion(v)),
    }
}

/// Persist `state` to `state.json` inside `config_dir`. Creates the
/// parent directory if missing. Atomic via [`write_atomic`].
pub fn save_to_dir(config_dir: &Path, state: &AppState) -> Result<(), AppStateError> {
    if let Err(e) = std::fs::create_dir_all(config_dir) {
        return Err(AppStateError::Io {
            path: config_dir.to_path_buf(),
            source: e,
        });
    }
    let path = state_path_in(config_dir);
    let body = serde_json::to_vec_pretty(state).map_err(|e| AppStateError::Parse {
        reason: e.to_string(),
    })?;
    write_atomic(&path, &body).map_err(|e| AppStateError::Io { path, source: e })
}

/// Upsert `harness` into `config_dir`'s state.json by id. Loads, mutates,
/// saves. Re-applying the same `HarnessConfig` is idempotent except for
/// the `last_patched_at` timestamp, which moves forward with each call.
pub fn upsert_harness_in(config_dir: &Path, harness: HarnessConfig) -> Result<(), AppStateError> {
    let mut state = load_from_dir(config_dir)?;
    if let Some(slot) = state.harnesses.iter_mut().find(|h| h.id == harness.id) {
        *slot = harness;
    } else {
        state.harnesses.push(harness);
    }
    save_to_dir(config_dir, &state)
}

/// Remove the entry for `id` from `config_dir`'s state.json. No-op when
/// no entry exists for `id`.
pub fn remove_harness_in(config_dir: &Path, id: HarnessId) -> Result<(), AppStateError> {
    let mut state = load_from_dir(config_dir)?;
    state.harnesses.retain(|h| h.id != id);
    save_to_dir(config_dir, &state)
}

// ---------------------------------------------------------------------------
// BackendDraft <-> Backend translation
// ---------------------------------------------------------------------------

/// One secret pulled out of a [`BackendDraft`] alongside the keychain
/// account name it should land under. Returned by
/// [`drain_secrets_from_draft`] so callers can persist them in a
/// uniform loop without per-variant branching.
pub struct DraftSecret {
    pub account: String,
    pub value: zeroize::Zeroizing<String>,
}

/// Translate a [`BackendDraft`] into the persisted [`Backend`] plus the
/// list of secrets to store in the OS keychain. Consumes the draft so
/// the raw values can be moved into [`zeroize::Zeroizing`] (and wiped
/// when each `DraftSecret` drops). The returned `Backend` is safe to
/// persist in `state.json` — every secret-bearing field is now a
/// [`SecretRef`].
#[must_use]
pub fn drain_secrets_from_draft(draft: BackendDraft) -> (Backend, Vec<DraftSecret>) {
    use crate::secrets::accounts;

    let mut secrets: Vec<DraftSecret> = Vec::new();

    let backend = match draft {
        BackendDraft::Signoz {
            region,
            ingestion_key,
        } => {
            let account = accounts::signoz_ingestion_key();
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(ingestion_key),
            });
            Backend::Signoz {
                region,
                ingestion_key: SecretRef::for_account(account),
            }
        }
        BackendDraft::Honeycomb { team, dataset } => {
            let account = accounts::honeycomb_team();
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(team),
            });
            Backend::Honeycomb {
                team: SecretRef::for_account(account),
                dataset,
            }
        }
        BackendDraft::GrafanaCloud { endpoint, auth } => {
            let account = accounts::grafana_cloud_auth();
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(auth),
            });
            Backend::GrafanaCloud {
                endpoint,
                auth: SecretRef::for_account(account),
            }
        }
        BackendDraft::Datadog { site, api_key } => {
            let account = accounts::datadog_api_key();
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(api_key),
            });
            Backend::Datadog {
                site,
                api_key: SecretRef::for_account(account),
            }
        }
        BackendDraft::OtlpGeneric {
            endpoint,
            protocol,
            headers,
        } => {
            let mut header_refs: BTreeMap<String, SecretRef> = BTreeMap::new();
            for (name, raw) in headers {
                let account = accounts::otlp_generic_header(&name);
                secrets.push(DraftSecret {
                    account: account.clone(),
                    value: zeroize::Zeroizing::new(raw),
                });
                header_refs.insert(name, SecretRef::for_account(account));
            }
            Backend::OtlpGeneric {
                endpoint,
                protocol,
                headers: header_refs,
            }
        }
        BackendDraft::OtelcolPassthrough { endpoint } => Backend::OtelcolPassthrough { endpoint },
    };

    (backend, secrets)
}

/// Build the [`HarnessConfig`] entry the IPC layer should upsert after a
/// successful `apply_patch`. Centralised so the IPC command and the
/// integration tests construct the value the same way.
#[must_use]
pub fn harness_config_from_apply(
    id: HarnessId,
    config_path: &Path,
    options: ApplyOptions,
    patch: TrovePatch,
) -> HarnessConfig {
    HarnessConfig {
        id,
        enabled: true,
        config_path: config_path.display().to_string(),
        last_patched_at: chrono::Utc::now().to_rfc3339(),
        trove_patch: patch,
        options,
    }
}

/// Enumerate the keychain account names that hold secrets for `backend`.
/// Used by `clear_backend` to wipe every entry the previous `save_backend`
/// installed without re-deriving the names per variant.
#[must_use]
pub fn backend_secret_accounts(backend: &Backend) -> Vec<String> {
    let mut out = Vec::new();
    match backend {
        Backend::Signoz { ingestion_key, .. } => out.push(ingestion_key.account.clone()),
        Backend::Honeycomb { team, .. } => out.push(team.account.clone()),
        Backend::GrafanaCloud { auth, .. } => out.push(auth.account.clone()),
        Backend::Datadog { api_key, .. } => out.push(api_key.account.clone()),
        Backend::OtlpGeneric { headers, .. } => {
            for r in headers.values() {
                out.push(r.account.clone());
            }
        }
        Backend::OtelcolPassthrough { .. } => {}
    }
    out
}

// ---------------------------------------------------------------------------
// AppHandle convenience layer
// ---------------------------------------------------------------------------

/// Resolve the platform's per-app config directory. The directory is
/// created lazily by [`save`]; callers that only [`load`] don't need
/// it to exist (a missing directory is treated as "no state yet").
fn config_dir(app: &AppHandle) -> Result<PathBuf, AppStateError> {
    app.path().app_config_dir().map_err(|e| AppStateError::Io {
        path: PathBuf::new(),
        source: std::io::Error::other(e.to_string()),
    })
}

/// Load app state for the running Tauri instance.
pub fn load(app: &AppHandle) -> Result<AppState, AppStateError> {
    let dir = config_dir(app)?;
    load_from_dir(&dir)
}

/// Persist `state` for the running Tauri instance.
pub fn save(app: &AppHandle, state: &AppState) -> Result<(), AppStateError> {
    let dir = config_dir(app)?;
    save_to_dir(&dir, state)
}

/// Upsert `harness` for the running Tauri instance.
pub fn upsert_harness(app: &AppHandle, harness: HarnessConfig) -> Result<(), AppStateError> {
    let dir = config_dir(app)?;
    upsert_harness_in(&dir, harness)
}

/// Remove the entry for `id` for the running Tauri instance.
pub fn remove_harness(app: &AppHandle, id: HarnessId) -> Result<(), AppStateError> {
    let dir = config_dir(app)?;
    remove_harness_in(&dir, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::safety::sentinels::Format;

    fn sample_signoz() -> Backend {
        Backend::Signoz {
            region: "us-east".into(),
            ingestion_key: SecretRef::for_account("backend.signoz.ingestion-key"),
        }
    }

    fn sample_harness(id: HarnessId) -> HarnessConfig {
        HarnessConfig {
            id,
            enabled: true,
            config_path: "/tmp/x".into(),
            last_patched_at: "2026-05-07T00:00:00Z".into(),
            trove_patch: TrovePatch {
                managed_block_hash: "a".repeat(64),
                file_hash_at_last_write: "b".repeat(64),
                format: Format::Json,
                last_written_region_payload: r#"{"env":{"OTEL_FOO":"bar"}}"#.into(),
            },
            options: ApplyOptions::default(),
        }
    }

    #[test]
    fn default_is_v4_with_null_backend_no_harnesses_and_auto_update_off() {
        let s = AppState::default();
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(s.schema_version, 4);
        assert!(s.backend.is_none());
        assert!(s.harnesses.is_empty());
        assert!(!s.auto_update_enabled);
    }

    #[test]
    fn backend_serializes_kebab_case_kind_and_camel_case_fields() {
        let json = serde_json::to_string(&sample_signoz()).unwrap();
        assert!(json.contains("\"kind\":\"signoz\""));
        assert!(json.contains("\"region\":\"us-east\""));
        assert!(json.contains("\"ingestionKey\""));
        assert!(!json.contains("ingestion_key"));
    }

    #[test]
    fn backend_kebab_case_for_compound_kinds() {
        let b = Backend::GrafanaCloud {
            endpoint: "https://otlp.grafana.net".into(),
            auth: SecretRef::for_account("backend.grafana-cloud.auth"),
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"kind\":\"grafana-cloud\""));

        let b = Backend::OtelcolPassthrough {
            endpoint: "127.0.0.1:4318".into(),
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"kind\":\"otelcol-passthrough\""));
    }

    #[test]
    fn backend_draft_round_trips_with_raw_secret() {
        let d = BackendDraft::Datadog {
            site: "datadoghq.eu".into(),
            api_key: "raw-secret-DO-NOT-PERSIST".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let revived: BackendDraft = serde_json::from_str(&json).unwrap();
        assert_eq!(d, revived);
    }

    #[test]
    fn app_state_round_trips_through_serde() {
        let state = AppState {
            schema_version: 4,
            backend: Some(sample_signoz()),
            harnesses: vec![sample_harness(HarnessId::ClaudeCode)],
            auto_update_enabled: false,
        };
        let json = serde_json::to_string(&state).unwrap();
        let revived: AppState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, revived);
    }

    #[test]
    fn app_state_round_trips_with_auto_update_enabled_true() {
        let state = AppState {
            schema_version: 4,
            backend: None,
            harnesses: Vec::new(),
            auto_update_enabled: true,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"autoUpdateEnabled\":true"));
        let revived: AppState = serde_json::from_str(&json).unwrap();
        assert!(revived.auto_update_enabled);
    }

    #[test]
    fn missing_file_loads_default() {
        let dir = tempfile::tempdir().unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state, AppState::default());
    }

    #[test]
    fn save_then_load_is_identity() {
        let dir = tempfile::tempdir().unwrap();
        let original = AppState {
            schema_version: 4,
            backend: Some(sample_signoz()),
            harnesses: vec![sample_harness(HarnessId::GeminiCli)],
            auto_update_enabled: false,
        };
        save_to_dir(dir.path(), &original).unwrap();
        let revived = load_from_dir(dir.path()).unwrap();
        assert_eq!(original, revived);
    }

    #[test]
    fn unknown_schema_version_surfaces_explicitly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            br#"{"schemaVersion": 99, "backend": null, "harnesses": []}"#,
        )
        .unwrap();
        let err = load_from_dir(dir.path()).unwrap_err();
        match err {
            AppStateError::UnknownSchemaVersion(v) => assert_eq!(v, 99),
            other => panic!("expected UnknownSchemaVersion, got {other:?}"),
        }
    }

    #[test]
    fn v2_state_is_loaded_and_migrated_to_current() {
        // Sprint 8 introduced TrovePatch.lastWrittenRegionPayload. Sprint
        // 10 added AppState.autoUpdateEnabled. v2 documents predate both
        // and must load cleanly: serde defaults backfill missing fields
        // (`""` for the patch payload, `false` for autoUpdateEnabled),
        // and the loader re-stamps schema_version to v4 so the next save
        // persists v4 to disk.
        let dir = tempfile::tempdir().unwrap();
        let v2_doc = br#"{
            "schemaVersion": 2,
            "backend": null,
            "harnesses": [
                {
                    "id": "claude-code",
                    "enabled": true,
                    "configPath": "/home/u/.claude/settings.json",
                    "lastPatchedAt": "2026-05-01T00:00:00Z",
                    "trovePatch": {
                        "managedBlockHash": "aaaa",
                        "fileHashAtLastWrite": "bbbb",
                        "format": "json"
                    },
                    "options": {
                        "logUserPrompts": false,
                        "customAttributes": {}
                    }
                }
            ]
        }"#;
        std::fs::write(dir.path().join("state.json"), v2_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(state.schema_version, 4);
        assert_eq!(state.harnesses.len(), 1);
        assert_eq!(state.harnesses[0].trove_patch.last_written_region_payload, "");
        assert!(!state.auto_update_enabled);
    }

    #[test]
    fn v2_state_round_trips_to_current_on_disk_after_save() {
        // Re-saving a migrated state must persist the current schemaVersion
        // with the new field present. Confirms the in-memory migration is
        // reflected back to disk on the next save.
        let dir = tempfile::tempdir().unwrap();
        let v2_doc = br#"{
            "schemaVersion": 2,
            "backend": null,
            "harnesses": []
        }"#;
        std::fs::write(dir.path().join("state.json"), v2_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        save_to_dir(dir.path(), &state).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
        assert!(on_disk.contains("\"schemaVersion\": 4"));
        assert!(on_disk.contains("\"autoUpdateEnabled\": false"));
    }

    #[test]
    fn v3_state_is_migrated_to_v4_with_auto_update_off() {
        // Sprint 10 added AppState.autoUpdateEnabled. v3 documents from
        // Sprint 8/9 lack the field — serde(default) backfills `false` and
        // the loader re-stamps schema_version to v4.
        let dir = tempfile::tempdir().unwrap();
        let v3_doc = br#"{
            "schemaVersion": 3,
            "backend": null,
            "harnesses": [
                {
                    "id": "gemini-cli",
                    "enabled": true,
                    "configPath": "/home/u/.gemini/settings.json",
                    "lastPatchedAt": "2026-05-08T00:00:00Z",
                    "trovePatch": {
                        "managedBlockHash": "cccc",
                        "fileHashAtLastWrite": "dddd",
                        "format": "json",
                        "lastWrittenRegionPayload": "{\"telemetry\":{}}"
                    },
                    "options": {
                        "logUserPrompts": false,
                        "customAttributes": {}
                    }
                }
            ]
        }"#;
        std::fs::write(dir.path().join("state.json"), v3_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, 4);
        assert!(!state.auto_update_enabled);
        assert_eq!(state.harnesses.len(), 1);
        // Field carried through migration unchanged.
        assert_eq!(
            state.harnesses[0].trove_patch.last_written_region_payload,
            "{\"telemetry\":{}}"
        );
    }

    #[test]
    fn v3_state_round_trips_to_v4_on_disk_after_save() {
        let dir = tempfile::tempdir().unwrap();
        let v3_doc = br#"{
            "schemaVersion": 3,
            "backend": null,
            "harnesses": []
        }"#;
        std::fs::write(dir.path().join("state.json"), v3_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        save_to_dir(dir.path(), &state).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
        assert!(on_disk.contains("\"schemaVersion\": 4"));
        assert!(on_disk.contains("\"autoUpdateEnabled\": false"));
    }

    #[test]
    fn v4_state_with_auto_update_true_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let v4_doc = br#"{
            "schemaVersion": 4,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": true
        }"#;
        std::fs::write(dir.path().join("state.json"), v4_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, 4);
        assert!(state.auto_update_enabled);
    }

    #[test]
    fn v1_state_is_explicitly_rejected_not_silently_upgraded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            br#"{"schemaVersion": 1, "backend": null, "harnesses": []}"#,
        )
        .unwrap();
        match load_from_dir(dir.path()).unwrap_err() {
            AppStateError::UnknownSchemaVersion(1) => {}
            other => panic!("expected UnknownSchemaVersion(1), got {other:?}"),
        }
    }

    #[test]
    fn upsert_replaces_by_id_not_appends() {
        let dir = tempfile::tempdir().unwrap();
        let h1 = sample_harness(HarnessId::ClaudeCode);
        upsert_harness_in(dir.path(), h1.clone()).unwrap();

        let mut h2 = sample_harness(HarnessId::ClaudeCode);
        h2.last_patched_at = "2026-05-08T00:00:00Z".into();
        upsert_harness_in(dir.path(), h2.clone()).unwrap();

        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.harnesses.len(), 1);
        assert_eq!(state.harnesses[0].last_patched_at, "2026-05-08T00:00:00Z");
    }

    #[test]
    fn upsert_appends_new_ids() {
        let dir = tempfile::tempdir().unwrap();
        upsert_harness_in(dir.path(), sample_harness(HarnessId::ClaudeCode)).unwrap();
        upsert_harness_in(dir.path(), sample_harness(HarnessId::GeminiCli)).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.harnesses.len(), 2);
    }

    #[test]
    fn remove_is_noop_for_missing_id() {
        let dir = tempfile::tempdir().unwrap();
        remove_harness_in(dir.path(), HarnessId::ClaudeCode).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert!(state.harnesses.is_empty());
    }

    #[test]
    fn remove_removes_only_the_targeted_id() {
        let dir = tempfile::tempdir().unwrap();
        upsert_harness_in(dir.path(), sample_harness(HarnessId::ClaudeCode)).unwrap();
        upsert_harness_in(dir.path(), sample_harness(HarnessId::GeminiCli)).unwrap();
        remove_harness_in(dir.path(), HarnessId::ClaudeCode).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.harnesses.len(), 1);
        assert_eq!(state.harnesses[0].id, HarnessId::GeminiCli);
    }

    #[test]
    fn save_creates_missing_parent_dir() {
        let outer = tempfile::tempdir().unwrap();
        let nested = outer.path().join("nope/yet");
        save_to_dir(&nested, &AppState::default()).unwrap();
        assert!(nested.join("state.json").exists());
    }

    #[test]
    fn parse_error_surfaces_not_swallowed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("state.json"), b"{ not json").unwrap();
        match load_from_dir(dir.path()).unwrap_err() {
            AppStateError::Parse { .. } => {}
            other => panic!("expected Parse, got {other:?}"),
        }
    }
}
