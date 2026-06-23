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
//! parsing the rest. The current schema is version `8`. Older versions
//! are migrated in-place by relying on `#[serde(default)]` for fields
//! introduced in later schemas:
//! - v2..=v5 → v6: see prior fields backfilled via `#[serde(default)]`.
//! - v6 → v7 (multi-platform refactor): the single nullable `backend`
//!   slot is rewritten as a one-element `backends` list (or empty when
//!   the slot was `None`), with a freshly-generated UUID id. Existing
//!   keychain entries keep their kind-keyed account names — the legacy
//!   single instance gets the empty-suffix variant so secrets resolve
//!   without re-prompting the user.
//! - v7 → v8 (launch-at-startup opt-out): adds
//!   `launch_at_startup_enabled` defaulting to `true`. Existing installs
//!   inherit the opt-out on first launch after upgrade; the
//!   `reconcile_launch_at_startup` hook in the IPC layer registers
//!   the OS-side login item lazily on that same first launch.
//! - All earlier versions migrate forward through the same chain.
//!
//! After parse, the loader re-stamps
//! `schema_version = CURRENT_SCHEMA_VERSION` on the in-memory value so
//! the next save persists v8 to disk. v1 was never written in the wild
//! and remains an explicit error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use thiserror::Error;

use crate::adapters::{ApplyOptions, TrovePatch};
use crate::harness::HarnessId;
use crate::mappings::{default_state as default_mapping_state, MappingState};
use crate::safety::atomic::write_atomic;

/// Filename inside the app config directory.
pub const STATE_FILENAME: &str = "state.json";

/// Current schema version. Bumped any time the persisted shape changes.
/// See module docs for the migration scaffold.
pub const CURRENT_SCHEMA_VERSION: u32 = 11;

/// Serde helper that defaults a `bool` field to `true`. Used for opt-out
/// settings whose absence in a migrated document should be interpreted
/// as "user wants this feature on" rather than the language default
/// `false`.
fn default_true() -> bool {
    true
}

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
        endpoint: String,
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
    #[serde(rename_all = "camelCase")]
    NewRelic {
        /// `"us"` or `"eu"` — selects the OTLP intake host.
        region: String,
        license_key: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    SplunkObservability {
        realm: String,
        access_token: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Dynatrace {
        environment_url: String,
        api_token: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Elastic {
        endpoint: String,
        api_key: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Opensearch {
        endpoint: String,
        /// Literal `Authorization` header value the user supplied
        /// (typically `Basic <base64(user:pass)>`).
        authorization: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Openobserve {
        endpoint: String,
        organization: String,
        authorization: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Clickstack {
        endpoint: String,
        ingestion_key: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Chronosphere {
        tenant: String,
        api_token: SecretRef,
    },
    #[serde(rename_all = "camelCase")]
    Sentry {
        endpoint: String,
        auth_header: SecretRef,
    },
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
        endpoint: String,
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
    #[serde(rename_all = "camelCase")]
    NewRelic {
        region: String,
        license_key: String,
    },
    #[serde(rename_all = "camelCase")]
    SplunkObservability {
        realm: String,
        access_token: String,
    },
    #[serde(rename_all = "camelCase")]
    Dynatrace {
        environment_url: String,
        api_token: String,
    },
    #[serde(rename_all = "camelCase")]
    Elastic {
        endpoint: String,
        api_key: String,
    },
    #[serde(rename_all = "camelCase")]
    Opensearch {
        endpoint: String,
        authorization: String,
    },
    #[serde(rename_all = "camelCase")]
    Openobserve {
        endpoint: String,
        organization: String,
        authorization: String,
    },
    #[serde(rename_all = "camelCase")]
    Clickstack {
        endpoint: String,
        ingestion_key: String,
    },
    #[serde(rename_all = "camelCase")]
    Chronosphere {
        tenant: String,
        api_token: String,
    },
    #[serde(rename_all = "camelCase")]
    Sentry {
        endpoint: String,
        auth_header: String,
    },
}

/// One configured forwarding destination. Multi-platform support wraps
/// each [`Backend`] with a stable UUID id (used to scope keychain
/// accounts and identify the row in IPC edit/remove calls) and an
/// optional display label. Mirrors the Zod `BackendInstance`.
///
/// `enabled` (introduced in schema v10) gates whether the collector
/// pipeline forwards to this instance and whether it appears on the
/// Overview Data flow chart. Disabled instances persist in the state
/// so the user can keep their configuration and re-enable later;
/// pre-v10 documents migrate by defaulting to `true` via the
/// `default_true` serde fallback.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInstance {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub backend: Backend,
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

/// Identity attribution for outgoing telemetry. Opt-in; off by
/// default. When enabled, the collector pipeline tags every signal
/// with `user.name` and `user.email` resource attributes via a
/// dynamically-rendered `resource/identity` processor. Source:
///
/// - `Auto`: resolve from the lowest-friction probe at runtime —
///   detected harness configs (when one exposes identity), then git
///   global config (`git config --global user.name/email`).
/// - `Manual`: use the persisted [`Self::name`] / [`Self::email`]
///   verbatim.
///
/// Empty resolved values cause the processor to be omitted entirely
/// rather than tagging signals with empty strings.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub source: IdentitySource,
    /// User-entered override values. Read only when `source == Manual`;
    /// retained when toggling back to `Auto` so the user does not have
    /// to re-type after a round-trip.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentitySource {
    /// Probe ladder: harness configs first, then git config.
    #[default]
    Auto,
    /// Use the persisted name/email verbatim.
    Manual,
}

/// Persisted application state. Secrets are referenced via [`SecretRef`]
/// only. Mirrors the Zod `AppState` schema.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    /// Pinned to [`CURRENT_SCHEMA_VERSION`] (currently `7`). Older or
    /// newer values surface as [`AppStateError::UnknownSchemaVersion`].
    pub schema_version: u32,
    /// Configured forwarding destinations. Every signal is broadcast to
    /// every configured backend (no per-platform routing rules). Empty
    /// list means "no platforms configured" — the supervisor runs the
    /// smoke config and waits.
    #[serde(default)]
    pub backends: Vec<BackendInstance>,
    pub harnesses: Vec<HarnessConfig>,
    /// Sprint 10 — opt-in auto-updater. Default `false`; flipped via the
    /// `set_auto_update_enabled` IPC command. Gates only the
    /// background-on-launch update probe; the user-facing
    /// `check_for_updates` IPC command is an explicit action and runs
    /// regardless. Trove never contacts GitHub Releases without either
    /// this flag set, or a click in the UI.
    #[serde(default)]
    pub auto_update_enabled: bool,
    /// Opt-OUT auto-launch (introduced in v8). When true, Trove registers
    /// itself with the OS login-items mechanism (`LaunchAgent` on macOS,
    /// Run registry key on Windows, `.desktop` autostart on Linux) so
    /// harness telemetry is captured from session start. Mutated via
    /// `set_launch_at_startup_enabled`, which also drives the OS-side
    /// registration through `tauri-plugin-autostart`. Default `true`;
    /// v7 documents missing the field migrate to `true` via
    /// `default_true` (the reconciler in the IPC layer then registers
    /// the login item on first launch after upgrade).
    #[serde(default = "default_true")]
    pub launch_at_startup_enabled: bool,
    /// Sprint 12 — opt-in `user.name`/`user.email` attribution for
    /// outgoing telemetry. Off by default; flipped via
    /// `set_identity_enabled` and `set_identity_manual`. See
    /// [`Identity`].
    #[serde(default)]
    pub identity: Identity,
    /// Sprint 13 — per-harness Tier A mapping rows. Auto-populated with
    /// [`crate::mappings::default_state`] when missing, so v5 documents
    /// migrate forward with full Tier A coverage rather than an empty
    /// mapping table the user would otherwise have to populate by hand.
    /// See [`crate::mappings`] for the schema and per-harness defaults.
    #[serde(default = "default_mapping_state")]
    pub mappings: MappingState,
    /// Sticky per-harness "first telemetry observed at" timestamps
    /// (Unix seconds). Once any harness has emitted telemetry that the
    /// collector or an adapter watcher counted, its entry lands here
    /// and persists across app restarts. The Harnesses dashboard reads
    /// this to keep the pill green even when the live counter resets
    /// (e.g. process restart, harness idle for hours). Forward-compat
    /// via `#[serde(default)]`: v7 state docs without this field load
    /// as an empty map and accrete entries on the next observation.
    #[serde(default)]
    pub telemetry_observed: BTreeMap<HarnessId, i64>,
}

impl Default for AppState {
    /// The state a fresh launch sees when no `state.json` exists yet.
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            backends: Vec::new(),
            harnesses: Vec::new(),
            auto_update_enabled: false,
            launch_at_startup_enabled: true,
            identity: Identity::default(),
            mappings: default_mapping_state(),
            telemetry_observed: BTreeMap::new(),
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
        // v2..=v11 migration. Earlier sprints added autoUpdateEnabled,
        // identity, and mappings — all use `#[serde(default)]` so older
        // documents deserialize cleanly. v7 (multi-platform refactor)
        // replaces the single `backend` slot with a `backends` list; the
        // pre-deserialization shim below rewrites the legacy field when
        // present. v8 adds launch_at_startup_enabled defaulting to true
        // via `default_true`. v9 (release-blockers commit train) flips
        // `trovePatch.format` from `"yaml"` to `"shell"` for wrapper
        // adapter entries whose `configPath` is a shell rc. v10 adds
        // per-instance `enabled` on every `BackendInstance` so a
        // platform can be paused without deletion — v9 documents that
        // omit the field default to `true` via the `default_true` serde
        // fallback on the struct, so no value-level shim is required.
        // v11 removes stale `claude-desktop` harness entries written by
        // the pre-fix `autoenable_claude_desktop_on_first_run` on
        // Linux/Windows machines where Desktop is not installed.
        // We re-stamp schema_version on the returned struct so the
        // next save persists the current version to disk.
        2..=11 => {
            // Field-shape migrations applied at the JSON-value level so
            // we don't need to keep parallel legacy deserializer types
            // around. Applied in chronological order so a v2 document
            // walks through every fix before final typed deserialization:
            //   (1) SigNoz `region` → `endpoint` (rename inside one
            //       discriminated-union variant).
            //   (2) v6 → v7: `backend: Backend | null` → `backends: [BackendInstance]`.
            //   (3) v8 → v9: wrapper-adapter `trovePatch.format` `"yaml"`
            //       → `"shell"` (the new `Format::Shell` variant).
            //   (4) v10 → v11: remove phantom `claude-desktop` entries
            //       whose `configPath` doesn't exist on disk.
            let mut value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| AppStateError::Parse {
                    reason: e.to_string(),
                })?;
            migrate_signoz_region_to_endpoint(&mut value);
            migrate_backend_to_backends(&mut value);
            migrate_wrapper_format_yaml_to_shell(&mut value);
            migrate_remove_phantom_claude_desktop(&mut value);
            let mut state: AppState =
                serde_json::from_value(value).map_err(|e| AppStateError::Parse {
                    reason: e.to_string(),
                })?;
            state.schema_version = CURRENT_SCHEMA_VERSION;
            // The inner `MappingState.schema_version` is independent of
            // the outer AppState version. Run the inner migration here
            // so v1 mapping docs (no `metrics` field) pick up the
            // builtin catalog before validators or codegen run. When
            // the migration mutates the document (e.g. coerces a
            // legacy `error.kind=routing` to `network`, or splits the
            // legacy claude-desktop api_request fan-out into per-facet
            // keys), persist the fixed-up state back to disk so the
            // user never sees the stale validation errors on next
            // launch. Best-effort save: a write failure here doesn't
            // block the load itself.
            let mappings_migrated = state.mappings.migrate_to_current();
            backfill_missing_harness_mappings(&mut state);
            if mappings_migrated {
                if let Err(e) = save_to_dir(config_dir, &state) {
                    tracing::warn!(
                        error = %e,
                        "failed to persist migrated mappings — will retry on next save"
                    );
                }
            }
            Ok(state)
        }
        // v1 was never persisted in the wild — Sprint 4 bumped the shape
        // mid-development, before state.json was ever written by Trove.
        // Hitting this branch means the user hand-crafted a v1 file, which
        // we treat as a real bug they should escalate.
        v => Err(AppStateError::UnknownSchemaVersion(v)),
    }
}

/// Reshape an in-memory `state.json` JSON value from the v2..=v5
/// `SigNoz` `region`-keyed form to the current `endpoint`-keyed form.
/// Idempotent: a no-op when the backend variant is not signoz or when
/// the document already uses `endpoint`. The backend may live under
/// either the legacy single-slot `backend` key or the v7+ `backends`
/// list — both forms are handled.
fn migrate_signoz_region_to_endpoint(value: &mut serde_json::Value) {
    let fixup = |backend: &mut serde_json::Map<String, serde_json::Value>| {
        let is_signoz = backend
            .get("kind")
            .and_then(serde_json::Value::as_str)
            == Some("signoz");
        if is_signoz && backend.contains_key("region") && !backend.contains_key("endpoint") {
            let region = backend
                .remove("region")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            backend.insert(
                "endpoint".to_string(),
                serde_json::Value::String(format!("ingest.{region}.signoz.cloud:443")),
            );
        }
    };
    if let Some(backend) = value
        .get_mut("backend")
        .and_then(serde_json::Value::as_object_mut)
    {
        fixup(backend);
    }
    if let Some(list) = value
        .get_mut("backends")
        .and_then(serde_json::Value::as_array_mut)
    {
        for entry in list {
            if let Some(backend) = entry
                .get_mut("backend")
                .and_then(serde_json::Value::as_object_mut)
            {
                fixup(backend);
            }
        }
    }
}

/// Reshape a v2..=v6 document's single `backend` slot into the v7
/// `backends` list. When `backend` is non-null, wrap it in a one-element
/// list with a freshly-generated UUID id and no label. When null or
/// absent, materialise an empty list. Idempotent: a no-op when the
/// document already carries `backends`.
fn migrate_backend_to_backends(value: &mut serde_json::Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    if obj.contains_key("backends") {
        return;
    }
    let legacy = obj.remove("backend");
    let list = match legacy {
        Some(serde_json::Value::Object(backend)) => {
            let id = uuid::Uuid::new_v4().to_string();
            let mut instance = serde_json::Map::new();
            instance.insert("id".to_string(), serde_json::Value::String(id));
            instance.insert("backend".to_string(), serde_json::Value::Object(backend));
            vec![serde_json::Value::Object(instance)]
        }
        _ => Vec::new(),
    };
    obj.insert("backends".to_string(), serde_json::Value::Array(list));
}

/// Reshape a v8 document's wrapper-adapter `trovePatch.format` values
/// from the legacy `"yaml"` token to the new `"shell"` token. Pre-v9
/// builds reused `Format::Yaml` for shell rc patches even though the
/// sentinels engine's YAML branch validates the host as YAML — which
/// erupts as `malformed yaml document` when the conflict-payload IPC
/// path runs `extract_region` on `~/.zshrc`. v9 introduces a dedicated
/// `Format::Shell` variant; this migration flips persisted entries
/// over so existing users don't need to re-Enable each wrapper.
///
/// A wrapper entry is identified by a `configPath` whose final segment
/// is a known shell rc name (`.zshrc`, `.bashrc`, `.bash_profile`, or
/// `config.fish`). The migration is idempotent: entries already on
/// `"shell"` are unchanged, and non-shell `"yaml"` entries are
/// untouched.
fn migrate_wrapper_format_yaml_to_shell(value: &mut serde_json::Value) {
    let Some(list) = value
        .get_mut("harnesses")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for entry in list {
        let Some(obj) = entry.as_object_mut() else {
            continue;
        };
        let is_shell_rc = obj
            .get("configPath")
            .and_then(serde_json::Value::as_str)
            .is_some_and(is_shell_rc_path);
        if !is_shell_rc {
            continue;
        }
        let Some(patch) = obj
            .get_mut("trovePatch")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        let needs_flip = patch
            .get("format")
            .and_then(serde_json::Value::as_str)
            == Some("yaml");
        if needs_flip {
            patch.insert(
                "format".to_string(),
                serde_json::Value::String("shell".to_string()),
            );
        }
    }
}

/// v10 → v11: remove any `claude-desktop` harness entries whose
/// `configPath` does not exist on disk. These were written by the
/// pre-fix `autoenable_claude_desktop_on_first_run` on Linux/Windows
/// machines where Desktop is not installed (the function called
/// `claude_desktop::apply()` unconditionally, which always succeeds and
/// records a macOS-specific path). Safe on macOS — the real sessions
/// root exists → entry is kept. No-op when the harnesses array is
/// absent or already clean.
fn migrate_remove_phantom_claude_desktop(v: &mut serde_json::Value) {
    let Some(harnesses) = v.get_mut("harnesses").and_then(|h| h.as_array_mut()) else {
        return;
    };
    harnesses.retain(|entry| {
        let is_desktop = entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            == Some("claude-desktop");
        if !is_desktop {
            return true;
        }
        let path = entry
            .get("configPath")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        std::path::Path::new(path).exists()
    });
}

/// Heuristic for detecting shell rc paths in `configPath` strings. The
/// migration only flips wrapper-adapter entries (aider, copilot-cli,
/// cursor-cli) — every other format that touches a shell rc would
/// already be a coding bug we'd want to surface, not silently rewrite.
fn is_shell_rc_path(path: &str) -> bool {
    let tail = path.rsplit('/').next().unwrap_or(path);
    matches!(
        tail,
        ".zshrc" | ".bashrc" | ".bash_profile" | "config.fish"
    )
}

/// Backfill `mappings.harnesses` with the default entry for any
/// [`HarnessId`] that's missing from a freshly-loaded state. This
/// guarantees newly-introduced harnesses pick up their default
/// mapping rows on the next launch without forcing a manual "Reset to
/// defaults" click. Existing entries are left untouched so user
/// customizations survive (the `enabled` flag, attribute tweaks, etc.).
fn backfill_missing_harness_mappings(state: &mut AppState) {
    let existing: std::collections::HashSet<HarnessId> = state
        .mappings
        .harnesses
        .iter()
        .map(|h| h.harness_id)
        .collect();
    for id in HarnessId::all() {
        if !existing.contains(id) {
            state
                .mappings
                .harnesses
                .push(crate::mappings::defaults_for(*id));
        }
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

/// Mark every id in `ids` as having been observed (Unix seconds = `now`).
/// Idempotent: ids that already have a timestamp keep their original
/// "first observed at"; only newly-seen ids are inserted. Returns the
/// set of ids whose entry was added (useful for tests / logging). The
/// save is skipped entirely when nothing changed, so the common-case
/// poll (every harness already sticky) is a single state.json read.
pub fn record_telemetry_observed_in(
    config_dir: &Path,
    ids: impl IntoIterator<Item = HarnessId>,
    now_unix_seconds: i64,
) -> Result<Vec<HarnessId>, AppStateError> {
    let mut state = load_from_dir(config_dir)?;
    let mut added = Vec::new();
    for id in ids {
        if let std::collections::btree_map::Entry::Vacant(e) = state.telemetry_observed.entry(id) {
            e.insert(now_unix_seconds);
            added.push(id);
        }
    }
    if added.is_empty() {
        return Ok(added);
    }
    save_to_dir(config_dir, &state)?;
    Ok(added)
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
///
/// `instance_id` scopes the keychain account names so two instances of
/// the same kind never collide on a single keychain entry. Pass the
/// stable UUID of the [`BackendInstance`] that owns this draft.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn drain_secrets_from_draft(
    draft: BackendDraft,
    instance_id: &str,
) -> (Backend, Vec<DraftSecret>) {
    use crate::secrets::accounts;

    let mut secrets: Vec<DraftSecret> = Vec::new();

    let backend = match draft {
        BackendDraft::Signoz {
            endpoint,
            ingestion_key,
        } => {
            let account = accounts::signoz_ingestion_key(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(ingestion_key),
            });
            Backend::Signoz {
                endpoint,
                ingestion_key: SecretRef::for_account(account),
            }
        }
        BackendDraft::Honeycomb { team, dataset } => {
            let account = accounts::honeycomb_team(instance_id);
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
            let account = accounts::grafana_cloud_auth(instance_id);
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
            let account = accounts::datadog_api_key(instance_id);
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
                let account = accounts::otlp_generic_header(instance_id, &name);
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
        BackendDraft::NewRelic {
            region,
            license_key,
        } => {
            let account = accounts::new_relic_license_key(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(license_key),
            });
            Backend::NewRelic {
                region,
                license_key: SecretRef::for_account(account),
            }
        }
        BackendDraft::SplunkObservability {
            realm,
            access_token,
        } => {
            let account = accounts::splunk_observability_access_token(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(access_token),
            });
            Backend::SplunkObservability {
                realm,
                access_token: SecretRef::for_account(account),
            }
        }
        BackendDraft::Dynatrace {
            environment_url,
            api_token,
        } => {
            let account = accounts::dynatrace_api_token(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(api_token),
            });
            Backend::Dynatrace {
                environment_url,
                api_token: SecretRef::for_account(account),
            }
        }
        BackendDraft::Elastic { endpoint, api_key } => {
            let account = accounts::elastic_api_key(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(api_key),
            });
            Backend::Elastic {
                endpoint,
                api_key: SecretRef::for_account(account),
            }
        }
        BackendDraft::Opensearch {
            endpoint,
            authorization,
        } => {
            let account = accounts::opensearch_authorization(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(authorization),
            });
            Backend::Opensearch {
                endpoint,
                authorization: SecretRef::for_account(account),
            }
        }
        BackendDraft::Openobserve {
            endpoint,
            organization,
            authorization,
        } => {
            let account = accounts::openobserve_authorization(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(authorization),
            });
            Backend::Openobserve {
                endpoint,
                organization,
                authorization: SecretRef::for_account(account),
            }
        }
        BackendDraft::Clickstack {
            endpoint,
            ingestion_key,
        } => {
            let account = accounts::clickstack_ingestion_key(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(ingestion_key),
            });
            Backend::Clickstack {
                endpoint,
                ingestion_key: SecretRef::for_account(account),
            }
        }
        BackendDraft::Chronosphere { tenant, api_token } => {
            let account = accounts::chronosphere_api_token(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(api_token),
            });
            Backend::Chronosphere {
                tenant,
                api_token: SecretRef::for_account(account),
            }
        }
        BackendDraft::Sentry {
            endpoint,
            auth_header,
        } => {
            let account = accounts::sentry_auth_header(instance_id);
            secrets.push(DraftSecret {
                account: account.clone(),
                value: zeroize::Zeroizing::new(auth_header),
            });
            Backend::Sentry {
                endpoint,
                auth_header: SecretRef::for_account(account),
            }
        }
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
#[allow(clippy::match_same_arms)]
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
        Backend::NewRelic { license_key, .. } => out.push(license_key.account.clone()),
        Backend::SplunkObservability { access_token, .. } => out.push(access_token.account.clone()),
        Backend::Dynatrace { api_token, .. } => out.push(api_token.account.clone()),
        Backend::Elastic { api_key, .. } => out.push(api_key.account.clone()),
        Backend::Opensearch { authorization, .. } => out.push(authorization.account.clone()),
        Backend::Openobserve { authorization, .. } => out.push(authorization.account.clone()),
        Backend::Clickstack { ingestion_key, .. } => out.push(ingestion_key.account.clone()),
        Backend::Chronosphere { api_token, .. } => out.push(api_token.account.clone()),
        Backend::Sentry { auth_header, .. } => out.push(auth_header.account.clone()),
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
            endpoint: "ingest.us.signoz.cloud:443".into(),
            ingestion_key: SecretRef::for_account("backend.signoz.ingestion-key"),
        }
    }

    fn sample_signoz_instance() -> BackendInstance {
        BackendInstance {
            id: "11111111-1111-1111-1111-111111111111".into(),
            label: None,
            enabled: true,
            backend: sample_signoz(),
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
    fn default_is_current_schema_version_with_empty_state() {
        let s = AppState::default();
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(s.schema_version, 11);
        assert!(s.backends.is_empty());
        assert!(s.harnesses.is_empty());
        assert!(!s.auto_update_enabled);
        assert!(s.launch_at_startup_enabled);
        assert!(!s.identity.enabled);
        assert_eq!(s.identity.source, IdentitySource::Auto);
        assert!(s.identity.name.is_empty());
        assert!(s.identity.email.is_empty());
        // Mappings auto-populated with per-harness defaults so users
        // see Tier A signals on first launch.
        assert!(!s.mappings.harnesses.is_empty());
    }

    #[test]
    fn backend_serializes_kebab_case_kind_and_camel_case_fields() {
        let json = serde_json::to_string(&sample_signoz()).unwrap();
        assert!(json.contains("\"kind\":\"signoz\""));
        assert!(json.contains("\"endpoint\":\"ingest.us.signoz.cloud:443\""));
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
            schema_version: CURRENT_SCHEMA_VERSION,
            backends: vec![sample_signoz_instance()],
            harnesses: vec![sample_harness(HarnessId::ClaudeCode)],
            auto_update_enabled: false,
            launch_at_startup_enabled: true,
            identity: Identity::default(),
            mappings: default_mapping_state(),
            telemetry_observed: BTreeMap::new(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let revived: AppState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, revived);
    }

    #[test]
    fn app_state_round_trips_with_auto_update_enabled_true() {
        let state = AppState {
            schema_version: CURRENT_SCHEMA_VERSION,
            backends: Vec::new(),
            harnesses: Vec::new(),
            auto_update_enabled: true,
            launch_at_startup_enabled: true,
            identity: Identity::default(),
            mappings: default_mapping_state(),
            telemetry_observed: BTreeMap::new(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"autoUpdateEnabled\":true"));
        let revived: AppState = serde_json::from_str(&json).unwrap();
        assert!(revived.auto_update_enabled);
    }

    #[test]
    fn app_state_round_trips_with_launch_at_startup_disabled() {
        // Confirm the opt-out field serializes when toggled off and
        // survives a round trip.
        let state = AppState {
            schema_version: CURRENT_SCHEMA_VERSION,
            backends: Vec::new(),
            harnesses: Vec::new(),
            auto_update_enabled: false,
            launch_at_startup_enabled: false,
            identity: Identity::default(),
            mappings: default_mapping_state(),
            telemetry_observed: BTreeMap::new(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"launchAtStartupEnabled\":false"));
        let revived: AppState = serde_json::from_str(&json).unwrap();
        assert!(!revived.launch_at_startup_enabled);
    }

    #[test]
    fn load_backfills_new_harness_mappings_from_old_v6_state() {
        // Reproduce a real-world state.json frozen at schema v6: 10
        // harnesses, no claude-desktop entry. After load, the migration
        // should backfill claude-desktop with its default mapping rows
        // so the codegen overlay knows to emit the Cowork pipeline.
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path());
        let v6 = serde_json::json!({
            "schemaVersion": 6,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": true,
            "identity": { "enabled": false, "source": "auto", "name": "", "email": "" },
            "mappings": {
                "schemaVersion": 1,
                "harnesses": [
                    { "harnessId": "claude-code", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "gemini-cli", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "codex-cli", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "qwen-code", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "opencode", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "cursor-ide", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "cursor-cli", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "cline", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "aider", "enabled": true, "sources": [], "costOverrides": {} },
                    { "harnessId": "copilot-cli", "enabled": true, "sources": [], "costOverrides": {} }
                ]
            }
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&v6).unwrap()).unwrap();

        let state = load_from_dir(dir.path()).unwrap();

        let ids: Vec<HarnessId> = state
            .mappings
            .harnesses
            .iter()
            .map(|h| h.harness_id)
            .collect();
        assert!(
            ids.contains(&HarnessId::ClaudeDesktop),
            "load did not backfill claude-desktop: {ids:?}"
        );
        // The backfilled entry should carry the HookRule default sources
        // (so the codegen overlay emits signaltometrics/cowork).
        let cd = state
            .mappings
            .harnesses
            .iter()
            .find(|h| h.harness_id == HarnessId::ClaudeDesktop)
            .unwrap();
        assert!(!cd.sources.is_empty(), "backfilled claude-desktop has no sources");
    }

    #[test]
    fn backfill_preserves_existing_harness_customizations() {
        // If a v6 state has claude-code's mapping explicitly disabled,
        // the backfill must NOT clobber that — only missing entries are
        // populated.
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path());
        let v6 = serde_json::json!({
            "schemaVersion": 6,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": true,
            "identity": { "enabled": false, "source": "auto", "name": "", "email": "" },
            "mappings": {
                "schemaVersion": 1,
                "harnesses": [
                    { "harnessId": "claude-code", "enabled": false, "sources": [], "costOverrides": {} }
                ]
            }
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&v6).unwrap()).unwrap();

        let state = load_from_dir(dir.path()).unwrap();

        let cc = state
            .mappings
            .harnesses
            .iter()
            .find(|h| h.harness_id == HarnessId::ClaudeCode)
            .unwrap();
        assert!(!cc.enabled, "claude-code customization was clobbered");
    }

    #[test]
    fn missing_file_loads_default() {
        let dir = tempfile::tempdir().unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state, AppState::default());
    }

    #[test]
    fn record_telemetry_observed_inserts_new_ids_and_skips_save_when_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        // First call: inserts both, returns both.
        let added = record_telemetry_observed_in(
            dir.path(),
            [HarnessId::ClaudeDesktop, HarnessId::ClaudeCode],
            1_715_000_000,
        )
        .unwrap();
        assert_eq!(added.len(), 2);
        assert!(added.contains(&HarnessId::ClaudeDesktop));
        assert!(added.contains(&HarnessId::ClaudeCode));

        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(
            state.telemetry_observed.get(&HarnessId::ClaudeDesktop),
            Some(&1_715_000_000)
        );
        assert_eq!(
            state.telemetry_observed.get(&HarnessId::ClaudeCode),
            Some(&1_715_000_000)
        );

        // Second call with an overlapping id and a NEW id: only the new
        // one is added; the existing timestamp is preserved.
        let added = record_telemetry_observed_in(
            dir.path(),
            [HarnessId::ClaudeDesktop, HarnessId::GeminiCli],
            1_715_999_999,
        )
        .unwrap();
        assert_eq!(added, vec![HarnessId::GeminiCli]);
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(
            state.telemetry_observed.get(&HarnessId::ClaudeDesktop),
            Some(&1_715_000_000),
            "existing timestamp must be preserved (sticky 'first observed')",
        );
        assert_eq!(
            state.telemetry_observed.get(&HarnessId::GeminiCli),
            Some(&1_715_999_999)
        );
    }

    #[test]
    fn record_telemetry_observed_is_idempotent_for_already_seen_ids() {
        let dir = tempfile::tempdir().unwrap();
        record_telemetry_observed_in(dir.path(), [HarnessId::Cline], 100).unwrap();
        // Re-record the same id with a different timestamp.
        let added = record_telemetry_observed_in(dir.path(), [HarnessId::Cline], 200).unwrap();
        assert!(added.is_empty(), "no new ids; nothing should be reported as added");
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.telemetry_observed.get(&HarnessId::Cline), Some(&100));
    }

    #[test]
    fn telemetry_observed_field_is_forward_compatible_with_v7_state_lacking_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path());
        // A v7 doc written before this field existed — no
        // telemetryObserved key. Other optional fields are also
        // omitted so the test exercises the same serde `default`
        // pathway a real upgrade-in-place would hit.
        std::fs::write(
            &path,
            br#"{
                "schemaVersion": 7,
                "backends": [],
                "harnesses": []
            }"#,
        )
        .unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert!(state.telemetry_observed.is_empty());
    }

    #[test]
    fn v8_state_migrates_wrapper_format_yaml_to_shell() {
        // Pre-v9, wrapper adapters (aider, copilot-cli, cursor-cli)
        // persisted `trovePatch.format` as `"yaml"` because the
        // sentinels engine had no `Shell` variant. v9 introduces it;
        // the migration must flip the legacy token for entries whose
        // `configPath` is a shell rc, and leave other `"yaml"` entries
        // (real YAML hosts) alone.
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path());
        std::fs::write(
            &path,
            br#"{
                "schemaVersion": 8,
                "backends": [],
                "harnesses": [
                    {
                        "id": "aider",
                        "enabled": true,
                        "configPath": "/Users/dev/.zshrc",
                        "lastPatchedAt": "2026-05-01T00:00:00Z",
                        "trovePatch": {
                            "managedBlockHash": "abc",
                            "fileHashAtLastWrite": "def",
                            "format": "yaml",
                            "lastWrittenRegionPayload": "aider() { echo aider; }"
                        },
                        "options": { "customAttributes": {} }
                    },
                    {
                        "id": "copilot-cli",
                        "enabled": true,
                        "configPath": "/home/dev/.bashrc",
                        "lastPatchedAt": "2026-05-01T00:00:00Z",
                        "trovePatch": {
                            "managedBlockHash": "abc",
                            "fileHashAtLastWrite": "def",
                            "format": "yaml",
                            "lastWrittenRegionPayload": ""
                        },
                        "options": { "customAttributes": {} }
                    }
                ]
            }"#,
        )
        .unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        for entry in &state.harnesses {
            assert_eq!(
                entry.trove_patch.format,
                crate::safety::sentinels::Format::Shell,
                "{:?} should have migrated to Format::Shell",
                entry.id,
            );
        }
    }

    #[test]
    fn migrate_wrapper_format_helper_leaves_non_shell_yaml_alone() {
        // A purely-YAML host (e.g. a hypothetical adapter targeting
        // `~/.config/foo/config.yaml`) must keep `format: "yaml"`. The
        // migration only touches entries whose configPath is a known
        // shell rc name.
        let mut value = serde_json::json!({
            "harnesses": [
                {
                    "id": "hypothetical-yaml-adapter",
                    "configPath": "/Users/dev/.config/foo/config.yaml",
                    "trovePatch": {
                        "managedBlockHash": "a",
                        "fileHashAtLastWrite": "b",
                        "format": "yaml",
                        "lastWrittenRegionPayload": ""
                    }
                }
            ]
        });
        migrate_wrapper_format_yaml_to_shell(&mut value);
        assert_eq!(
            value["harnesses"][0]["trovePatch"]["format"], "yaml",
        );
    }

    #[test]
    fn migrate_wrapper_format_helper_is_idempotent_on_already_shell_entries() {
        let mut value = serde_json::json!({
            "harnesses": [
                {
                    "id": "aider",
                    "configPath": "/Users/dev/.zshrc",
                    "trovePatch": {
                        "managedBlockHash": "a",
                        "fileHashAtLastWrite": "b",
                        "format": "shell",
                        "lastWrittenRegionPayload": ""
                    }
                }
            ]
        });
        migrate_wrapper_format_yaml_to_shell(&mut value);
        assert_eq!(
            value["harnesses"][0]["trovePatch"]["format"], "shell",
        );
    }

    // ── migrate_remove_phantom_claude_desktop ─────────────────────────────

    #[test]
    fn migrate_remove_phantom_helper_strips_nonexistent_path() {
        // An entry whose configPath points to a path that doesn't exist
        // on the current machine is a phantom — the migration must remove
        // it so the Overview FlowChart and health counter stay accurate.
        let mut value = serde_json::json!({
            "harnesses": [
                {
                    "id": "claude-desktop",
                    "configPath": "/nonexistent/path/that/cannot/possibly/exist/abc123"
                }
            ]
        });
        migrate_remove_phantom_claude_desktop(&mut value);
        assert!(
            value["harnesses"].as_array().unwrap().is_empty(),
            "phantom claude-desktop entry should have been removed"
        );
    }

    #[test]
    fn migrate_remove_phantom_helper_keeps_entry_when_path_exists() {
        // When the configPath points to a real directory (macOS with Desktop
        // installed, or a test-provided tempdir), the entry must be kept.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let mut value = serde_json::json!({
            "harnesses": [
                {
                    "id": "claude-desktop",
                    "configPath": path
                }
            ]
        });
        migrate_remove_phantom_claude_desktop(&mut value);
        assert_eq!(
            value["harnesses"].as_array().unwrap().len(), 1,
            "real claude-desktop entry should be preserved"
        );
    }

    #[test]
    fn migrate_remove_phantom_helper_leaves_other_harnesses_alone() {
        // Non-claude-desktop entries must pass through regardless of whether
        // their configPath exists, so we don't accidentally disturb other harnesses.
        let mut value = serde_json::json!({
            "harnesses": [
                {
                    "id": "claude-code",
                    "configPath": "/nonexistent/path/that/cannot/possibly/exist/abc123"
                },
                {
                    "id": "gemini-cli",
                    "configPath": "/another/nonexistent/path"
                }
            ]
        });
        migrate_remove_phantom_claude_desktop(&mut value);
        assert_eq!(
            value["harnesses"].as_array().unwrap().len(), 2,
            "non-claude-desktop harnesses must not be touched"
        );
    }

    // ── v10 → v11 round-trip ──────────────────────────────────────────────

    #[test]
    fn v10_state_removes_phantom_claude_desktop_entry_on_migration() {
        // A v10 state.json written by the pre-fix autoenable path on Linux
        // or Windows contains a claude-desktop entry with a macOS configPath
        // that doesn't exist on the machine.  On load the v11 migration must
        // strip the entry and re-stamp the version; on re-save the stale row
        // must be absent from disk so the Overview and health counter are
        // immediately correct after the first post-upgrade launch.
        let dir = tempfile::tempdir().unwrap();
        let v10_doc = serde_json::json!({
            "schemaVersion": 10,
            "backends": [],
            "harnesses": [
                {
                    "id": "claude-desktop",
                    "enabled": true,
                    "configPath": "/Users/dev/Library/Application Support/Claude/local-agent-mode-sessions",
                    "lastPatchedAt": "2026-01-01T00:00:00Z",
                    "trovePatch": {
                        "managedBlockHash": "c".repeat(64),
                        "fileHashAtLastWrite": "",
                        "format": "json",
                        "lastWrittenRegionPayload": r#"{"harness":"claude-desktop"}"#
                    },
                    "options": {
                        "logUserPrompts": false,
                        "customAttributes": {}
                    }
                }
            ],
            "autoUpdateEnabled": false,
            "launchAtStartupEnabled": true,
            "identity": { "name": "", "email": "" },
            "telemetryObserved": {}
        });
        std::fs::write(
            dir.path().join("state.json"),
            serde_json::to_vec_pretty(&v10_doc).unwrap(),
        )
        .unwrap();

        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(
            state.harnesses.is_empty(),
            "phantom claude-desktop entry should be stripped by v11 migration"
        );

        // Persist so we can inspect the on-disk representation.
        save_to_dir(dir.path(), &state).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
        assert!(on_disk.contains("\"schemaVersion\": 11"));
        // The harnesses list must not contain a claude-desktop entry.
        // Note: mappings.harnesses still carries a "claude-desktop" mapping
        // key (backfilled for all known IDs), so we check for the absence
        // of the HarnessConfig `id` field specifically, not the string globally.
        assert!(
            !on_disk.contains("\"id\": \"claude-desktop\""),
            "stale claude-desktop HarnessConfig row must not be written back to disk"
        );
    }

    #[test]
    fn v7_state_migrates_to_v8_with_launch_at_startup_defaulting_true() {
        // Existing v7 documents predate the launch_at_startup opt-out.
        // They must load cleanly, default the new field to `true`, and
        // re-stamp the version to v8 on the in-memory struct.
        let dir = tempfile::tempdir().unwrap();
        let path = state_path_in(dir.path());
        std::fs::write(
            &path,
            br#"{
                "schemaVersion": 7,
                "backends": [],
                "harnesses": [],
                "autoUpdateEnabled": false
            }"#,
        )
        .unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(state.launch_at_startup_enabled);
    }

    #[test]
    fn save_then_load_is_identity() {
        let dir = tempfile::tempdir().unwrap();
        let original = AppState {
            schema_version: CURRENT_SCHEMA_VERSION,
            backends: vec![sample_signoz_instance()],
            harnesses: vec![sample_harness(HarnessId::GeminiCli)],
            auto_update_enabled: false,
            launch_at_startup_enabled: true,
            identity: Identity::default(),
            mappings: default_mapping_state(),
            telemetry_observed: BTreeMap::new(),
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
        assert!(on_disk.contains("\"schemaVersion\": 11"));
        assert!(on_disk.contains("\"autoUpdateEnabled\": false"));
    }

    #[test]
    fn v3_state_is_migrated_to_current_with_auto_update_off() {
        // Sprint 10 added AppState.autoUpdateEnabled. v3 documents from
        // Sprint 8/9 lack the field — serde(default) backfills `false`
        // and the loader re-stamps schema_version to the current
        // version.
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
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(!state.auto_update_enabled);
        assert_eq!(state.harnesses.len(), 1);
        // Field carried through migration unchanged.
        assert_eq!(
            state.harnesses[0].trove_patch.last_written_region_payload,
            "{\"telemetry\":{}}"
        );
    }

    #[test]
    fn v3_state_round_trips_to_current_on_disk_after_save() {
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
        assert!(on_disk.contains("\"schemaVersion\": 11"));
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
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(state.auto_update_enabled);
    }

    #[test]
    fn v4_state_migrates_to_current_with_identity_off() {
        // Sprint 12 added AppState.identity. v4 documents lack the
        // field — serde(default) backfills `Identity::default()` (off,
        // auto, empty strings) and the loader re-stamps schema_version
        // to the current version.
        let dir = tempfile::tempdir().unwrap();
        let v4_doc = br#"{
            "schemaVersion": 4,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": false
        }"#;
        std::fs::write(dir.path().join("state.json"), v4_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(!state.identity.enabled);
        assert_eq!(state.identity.source, IdentitySource::Auto);
        assert!(state.identity.name.is_empty());
        assert!(state.identity.email.is_empty());
    }

    #[test]
    fn v5_state_migrates_to_current_with_mappings_defaults() {
        // Sprint 13 added AppState.mappings. v5 documents lack the
        // field — serde(default = default_mapping_state) backfills the
        // per-harness Tier A defaults and the loader re-stamps
        // schema_version to v6. The user immediately sees Tier A
        // coverage rather than an empty mapping table.
        let dir = tempfile::tempdir().unwrap();
        let v5_doc = br#"{
            "schemaVersion": 5,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": false,
            "identity": {
                "enabled": false,
                "source": "auto",
                "name": "",
                "email": ""
            }
        }"#;
        std::fs::write(dir.path().join("state.json"), v5_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        // Every supported harness ID appears in the auto-populated
        // mappings table.
        assert_eq!(
            state.mappings.harnesses.len(),
            18,
            "v5→v6 migration should populate per-harness defaults (12 mapped + 6 detection-only)",
        );
    }

    #[test]
    fn v5_state_round_trips_to_v6_on_disk_after_save() {
        let dir = tempfile::tempdir().unwrap();
        let v5_doc = br#"{
            "schemaVersion": 5,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": false,
            "identity": {
                "enabled": false,
                "source": "auto",
                "name": "",
                "email": ""
            }
        }"#;
        std::fs::write(dir.path().join("state.json"), v5_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        save_to_dir(dir.path(), &state).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
        assert!(on_disk.contains("\"schemaVersion\": 11"));
        assert!(on_disk.contains("\"mappings\""));
    }

    #[test]
    fn v5_state_with_manual_identity_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let v5_doc = br#"{
            "schemaVersion": 5,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": false,
            "identity": {
                "enabled": true,
                "source": "manual",
                "name": "Ada Lovelace",
                "email": "ada@example.com"
            }
        }"#;
        std::fs::write(dir.path().join("state.json"), v5_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert!(state.identity.enabled);
        assert_eq!(state.identity.source, IdentitySource::Manual);
        assert_eq!(state.identity.name, "Ada Lovelace");
        assert_eq!(state.identity.email, "ada@example.com");
        // Re-save round-trips cleanly without losing the manual fields.
        save_to_dir(dir.path(), &state).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
        assert!(on_disk.contains("\"source\": \"manual\""));
        assert!(on_disk.contains("\"email\": \"ada@example.com\""));
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
    fn v6_state_migrates_backend_slot_into_backends_list() {
        // v7 (multi-platform refactor) replaces the single nullable
        // `backend` slot with a list of `BackendInstance` rows. The
        // loader wraps the legacy single backend into a one-element
        // list with a freshly-generated UUID id so the existing
        // keychain entries keep resolving.
        let dir = tempfile::tempdir().unwrap();
        let v6_doc = br#"{
            "schemaVersion": 6,
            "backend": {
                "kind": "signoz",
                "endpoint": "ingest.us.signoz.cloud:443",
                "ingestionKey": {
                    "service": "trove",
                    "account": "backend.signoz.ingestion-key"
                }
            },
            "harnesses": [],
            "autoUpdateEnabled": false,
            "identity": {
                "enabled": false,
                "source": "auto",
                "name": "",
                "email": ""
            }
        }"#;
        std::fs::write(dir.path().join("state.json"), v6_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(state.backends.len(), 1);
        assert!(!state.backends[0].id.is_empty());
        match &state.backends[0].backend {
            Backend::Signoz { endpoint, .. } => {
                assert_eq!(endpoint, "ingest.us.signoz.cloud:443");
            }
            other => panic!("expected signoz, got {other:?}"),
        }
    }

    #[test]
    fn v6_state_with_null_backend_migrates_to_empty_backends_list() {
        let dir = tempfile::tempdir().unwrap();
        let v6_doc = br#"{
            "schemaVersion": 6,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": false,
            "identity": {
                "enabled": false,
                "source": "auto",
                "name": "",
                "email": ""
            }
        }"#;
        std::fs::write(dir.path().join("state.json"), v6_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(state.backends.is_empty());
    }

    #[test]
    fn v6_state_round_trips_to_v7_on_disk_after_save() {
        let dir = tempfile::tempdir().unwrap();
        let v6_doc = br#"{
            "schemaVersion": 6,
            "backend": null,
            "harnesses": [],
            "autoUpdateEnabled": false,
            "identity": {
                "enabled": false,
                "source": "auto",
                "name": "",
                "email": ""
            }
        }"#;
        std::fs::write(dir.path().join("state.json"), v6_doc).unwrap();
        let state = load_from_dir(dir.path()).unwrap();
        save_to_dir(dir.path(), &state).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
        assert!(on_disk.contains("\"schemaVersion\": 11"));
        assert!(on_disk.contains("\"backends\""));
        assert!(!on_disk.contains("\"backend\":"));
    }

    #[test]
    fn signoz_region_state_is_migrated_to_endpoint_on_load() {
        // Pre-fix builds persisted SigNoz as `{"kind":"signoz","region":"X",...}`
        // and codegen built `ingest.{region}.signoz.cloud:443`. New builds
        // collect the full endpoint string; on load, transform legacy state
        // into the new field shape using the prior format string verbatim
        // so the user's hostname doesn't silently change meaning.
        let dir = tempfile::tempdir().unwrap();
        let legacy_doc = br#"{
            "schemaVersion": 4,
            "backend": {
                "kind": "signoz",
                "region": "us",
                "ingestionKey": {
                    "service": "trove",
                    "account": "backend.signoz.ingestion-key"
                }
            },
            "harnesses": [],
            "autoUpdateEnabled": false
        }"#;
        std::fs::write(dir.path().join("state.json"), legacy_doc).unwrap();

        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.backends.len(), 1);
        let Backend::Signoz {
            ref endpoint,
            ref ingestion_key,
        } = state.backends[0].backend
        else {
            panic!("expected a SigNoz backend, got {:?}", state.backends);
        };
        assert_eq!(endpoint, "ingest.us.signoz.cloud:443");
        assert_eq!(ingestion_key.account, "backend.signoz.ingestion-key");

        save_to_dir(dir.path(), &state).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
        assert!(on_disk.contains("\"endpoint\": \"ingest.us.signoz.cloud:443\""));
        assert!(!on_disk.contains("\"region\""));
    }

    #[test]
    fn signoz_state_already_in_endpoint_form_is_unchanged_on_load() {
        // Idempotency: a state.json that already uses `endpoint` must load
        // verbatim without the migration accidentally rewriting it.
        let dir = tempfile::tempdir().unwrap();
        let new_doc = br#"{
            "schemaVersion": 4,
            "backend": {
                "kind": "signoz",
                "endpoint": "ingest.eu.signoz.cloud:443",
                "ingestionKey": {
                    "service": "trove",
                    "account": "backend.signoz.ingestion-key"
                }
            },
            "harnesses": [],
            "autoUpdateEnabled": false
        }"#;
        std::fs::write(dir.path().join("state.json"), new_doc).unwrap();

        let state = load_from_dir(dir.path()).unwrap();
        assert_eq!(state.backends.len(), 1);
        let Backend::Signoz { ref endpoint, .. } = state.backends[0].backend else {
            panic!("expected a SigNoz backend, got {:?}", state.backends);
        };
        assert_eq!(endpoint, "ingest.eu.signoz.cloud:443");
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
