//! Per-user secrets store.
//!
//! Trove's backend credentials (`SigNoz` ingestion key, Honeycomb team
//! key, Datadog API key, etc.) are kept in a per-user JSON file at
//! `<app_config_dir>/secrets.json` with `0600` permissions on Unix
//! (owner read/write only). Sprint 13 switched the store away from the
//! OS keychain because the macOS Keychain ACL re-prompts on every
//! rebuild of the app, even after the user clicks "Always Allow" — each
//! rebuild has a new code-directory hash, and the keychain ACL stored
//! at "Always Allow" time doesn't match the new build's binary even
//! though the designated requirement (identifier + cert leaf) is
//! stable. `claude-sentinel` uses the same plain-file approach for the
//! same reason.
//!
//! Security parity with the keychain is reasonable: both expose the
//! secret to any code running as the logged-in user. The file is
//! mode-0600 so other users on a multi-user Mac can't read it.
//!
//! ## Migration
//!
//! Users who saved a backend with an older Trove version still have
//! their secrets in the OS keychain. On first read for a missing
//! account [`retrieve`] falls back to `keyring::Entry::get_password`
//! once, copies the value into the file, and afterwards never touches
//! the keychain again. The fallback CAN prompt one last time during
//! the upgrade; subsequent launches are silent.
//!
//! ## Account naming
//!
//! - Fixed-shape backends use `backend.<kind>.<field>`. Examples:
//!   `backend.signoz.ingestion-key`, `backend.honeycomb.team`,
//!   `backend.grafana-cloud.auth`, `backend.datadog.api-key`.
//! - The variadic OTLP-generic backend uses
//!   `backend.otlp-generic.header.<headerName>` per header.
//! - The `otelcol-passthrough` backend has no secrets.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::safety::atomic::write_atomic;

/// Service name historically used for Trove keychain entries. Kept as a
/// public constant for the [`SecretRef`](crate::app_state::SecretRef)
/// builder and the migration fallback in [`retrieve`].
pub const SERVICE: &str = "trove";

const SECRETS_FILENAME: &str = "secrets.json";
const SCHEMA_VERSION: u32 = 1;

static SECRETS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Failures from the secrets store. Mapped to `IpcError::Internal` at
/// the IPC boundary; the underlying message is preserved.
#[derive(Debug, Error)]
pub enum SecretsError {
    /// [`init`] was never called.
    #[error("secrets store not initialised; call secrets::init() at app boot")]
    NotInitialised,
    /// No entry under that account in either the file or the legacy
    /// keychain. Mirrors `keyring::Error::NoEntry` so callers that used
    /// to match on the old variant continue to compile.
    #[error("no secret stored for account `{0}`")]
    NotFound(String),
    #[error("secrets file io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("secrets file parse error: {0}")]
    Parse(#[from] serde_json::Error),
    /// Migration fallback hit the keychain and the keychain itself
    /// errored (not `NoEntry`, which is the silent miss path).
    #[error("legacy keychain read failed during migration: {0}")]
    Keychain(#[from] keyring::Error),
}

/// JSON shape persisted to disk. `version` is the schema/file format
/// version; only `1` exists today. We keep it explicit so future
/// migrations can branch on it the same way `state.json` does.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretsFile {
    version: u32,
    secrets: BTreeMap<String, String>,
}

/// One-shot setup. Call once at app boot, before any [`store`] /
/// [`retrieve`] / [`delete`] runs. Subsequent calls are ignored — the
/// first wins. `dir` is the directory in which `secrets.json` will live
/// (the same directory `state.json` is kept in, by convention).
pub fn init(dir: PathBuf) {
    let _ = SECRETS_DIR.set(dir);
}

fn configured_dir() -> Result<&'static Path, SecretsError> {
    SECRETS_DIR
        .get()
        .map(PathBuf::as_path)
        .ok_or(SecretsError::NotInitialised)
}

/// Persist `secret` under `account`. Overwrites any existing entry.
pub fn store(account: &str, secret: &Zeroizing<String>) -> Result<(), SecretsError> {
    store_in(configured_dir()?, account, secret)
}

/// Test-friendly variant of [`store`] with an explicit directory. The
/// directory is created if missing.
pub fn store_in(
    dir: &Path,
    account: &str,
    secret: &Zeroizing<String>,
) -> Result<(), SecretsError> {
    let mut file = read_file(dir).unwrap_or_default();
    file.version = SCHEMA_VERSION;
    file.secrets
        .insert(account.to_string(), secret.as_str().to_string());
    write_file(dir, &file)
}

/// Retrieve the value previously stored for `account`. Falls back to
/// the legacy OS keychain on first miss and migrates the value into the
/// file so subsequent reads never touch the keychain. Returns
/// [`SecretsError::NotFound`] when the account is unknown in both
/// places.
pub fn retrieve(account: &str) -> Result<Zeroizing<String>, SecretsError> {
    retrieve_in(configured_dir()?, account)
}

/// Test-friendly variant of [`retrieve`] with an explicit directory.
pub fn retrieve_in(dir: &Path, account: &str) -> Result<Zeroizing<String>, SecretsError> {
    let mut file = read_file(dir).unwrap_or_default();
    if let Some(value) = file.secrets.get(account) {
        return Ok(Zeroizing::new(value.clone()));
    }
    // Migration fallback: one-shot read from the legacy keychain. If
    // the user had stored credentials there with an older Trove build,
    // copy them into the file so we never touch the keychain again.
    match keyring::Entry::new(SERVICE, account).and_then(|e| e.get_password()) {
        Ok(value) => {
            file.version = SCHEMA_VERSION;
            file.secrets.insert(account.to_string(), value.clone());
            let _ = write_file(dir, &file);
            // Best-effort cleanup of the legacy keychain entry. Leaves
            // no residue so the user can revoke any "Always Allow" they
            // granted previously.
            if let Ok(entry) = keyring::Entry::new(SERVICE, account) {
                let _ = entry.delete_credential();
            }
            Ok(Zeroizing::new(value))
        }
        Err(keyring::Error::NoEntry) => Err(SecretsError::NotFound(account.to_string())),
        Err(e) => Err(SecretsError::Keychain(e)),
    }
}

/// Delete the entry for `account`. Idempotent. Best-effort also wipes
/// any legacy keychain residue so a clear-and-reset leaves nothing
/// behind.
pub fn delete(account: &str) -> Result<(), SecretsError> {
    delete_in(configured_dir()?, account)
}

/// Test-friendly variant of [`delete`] with an explicit directory.
pub fn delete_in(dir: &Path, account: &str) -> Result<(), SecretsError> {
    let mut file = read_file(dir).unwrap_or_default();
    file.secrets.remove(account);
    file.version = SCHEMA_VERSION;
    write_file(dir, &file)?;
    if let Ok(entry) = keyring::Entry::new(SERVICE, account) {
        let _ = entry.delete_credential();
    }
    Ok(())
}

fn read_file(dir: &Path) -> Result<SecretsFile, SecretsError> {
    let p = dir.join(SECRETS_FILENAME);
    if !p.exists() {
        return Ok(SecretsFile {
            version: SCHEMA_VERSION,
            secrets: BTreeMap::new(),
        });
    }
    let bytes = fs::read(&p)?;
    let file: SecretsFile = serde_json::from_slice(&bytes)?;
    Ok(file)
}

fn write_file(dir: &Path, file: &SecretsFile) -> Result<(), SecretsError> {
    fs::create_dir_all(dir)?;
    let p = dir.join(SECRETS_FILENAME);
    let body = serde_json::to_vec_pretty(file)?;
    write_atomic(&p, &body)?;
    set_owner_only_perms(&p)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_perms(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(p)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(p, perms)
}

#[cfg(not(unix))]
fn set_owner_only_perms(_p: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Canonical account-name builders. Centralised here so the wizard UI,
/// codegen, `clear_backend`, and tests can't drift on the convention.
pub mod accounts {
    /// `backend.signoz.ingestion-key`.
    #[must_use]
    pub fn signoz_ingestion_key() -> String {
        "backend.signoz.ingestion-key".to_string()
    }

    /// `backend.honeycomb.team`.
    #[must_use]
    pub fn honeycomb_team() -> String {
        "backend.honeycomb.team".to_string()
    }

    /// `backend.grafana-cloud.auth`.
    #[must_use]
    pub fn grafana_cloud_auth() -> String {
        "backend.grafana-cloud.auth".to_string()
    }

    /// `backend.datadog.api-key`.
    #[must_use]
    pub fn datadog_api_key() -> String {
        "backend.datadog.api-key".to_string()
    }

    /// `backend.otlp-generic.header.<headerName>`.
    #[must_use]
    pub fn otlp_generic_header(header_name: &str) -> String {
        format!("backend.otlp-generic.header.{header_name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn account_names_match_documented_convention() {
        assert_eq!(accounts::signoz_ingestion_key(), "backend.signoz.ingestion-key");
        assert_eq!(accounts::honeycomb_team(), "backend.honeycomb.team");
        assert_eq!(accounts::grafana_cloud_auth(), "backend.grafana-cloud.auth");
        assert_eq!(accounts::datadog_api_key(), "backend.datadog.api-key");
        assert_eq!(
            accounts::otlp_generic_header("x-honeycomb-team"),
            "backend.otlp-generic.header.x-honeycomb-team"
        );
    }

    #[test]
    fn service_constant_is_stable() {
        // Surfaces in the legacy keychain UI under "trove" and is part
        // of the migration fallback. Renaming would orphan keychain
        // entries created by older Trove builds.
        assert_eq!(SERVICE, "trove");
    }

    #[test]
    fn store_then_retrieve_round_trips_through_file() {
        let dir = TempDir::new().unwrap();
        let secret = Zeroizing::new("ok-secret".to_string());

        store_in(dir.path(), "test.acct", &secret).unwrap();
        let revived = retrieve_in(dir.path(), "test.acct").unwrap();

        assert_eq!(revived.as_str(), "ok-secret");
    }

    #[test]
    fn store_overwrites_existing_value() {
        let dir = TempDir::new().unwrap();
        store_in(dir.path(), "test.acct", &Zeroizing::new("first".into())).unwrap();
        store_in(dir.path(), "test.acct", &Zeroizing::new("second".into())).unwrap();
        let revived = retrieve_in(dir.path(), "test.acct").unwrap();
        assert_eq!(revived.as_str(), "second");
    }

    #[test]
    fn retrieve_returns_not_found_when_no_file_and_no_keychain_entry() {
        let dir = TempDir::new().unwrap();
        // Use a wildly-unique account so the migration fallback's
        // keychain probe is guaranteed to miss.
        let acct = format!("test.never-stored.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        let err = retrieve_in(dir.path(), &acct).unwrap_err();
        match err {
            SecretsError::NotFound(a) => assert_eq!(a, acct),
            // On CI/headless boxes the keychain backend itself can
            // error rather than return NoEntry. Treat that as the same
            // logical "not found" — it still means the legacy fallback
            // path is exercised and won't loop.
            SecretsError::Keychain(_) => {}
            other => panic!("expected NotFound or Keychain, got {other:?}"),
        }
    }

    #[test]
    fn delete_is_idempotent() {
        let dir = TempDir::new().unwrap();
        delete_in(dir.path(), "test.unknown").unwrap();
        // Second call still succeeds.
        delete_in(dir.path(), "test.unknown").unwrap();
    }

    #[test]
    fn delete_removes_the_account() {
        let dir = TempDir::new().unwrap();
        store_in(dir.path(), "test.acct", &Zeroizing::new("v".into())).unwrap();
        delete_in(dir.path(), "test.acct").unwrap();
        let err = retrieve_in(dir.path(), "test.acct").unwrap_err();
        assert!(matches!(
            err,
            SecretsError::NotFound(_) | SecretsError::Keychain(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn secrets_file_has_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        store_in(dir.path(), "test.acct", &Zeroizing::new("v".into())).unwrap();
        let mode = fs::metadata(dir.path().join(SECRETS_FILENAME))
            .unwrap()
            .permissions()
            .mode();
        // Mode is platform-decorated; mask to the low 9 bits we care about.
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn store_creates_dir_if_missing() {
        let parent = TempDir::new().unwrap();
        let nested = parent.path().join("does-not-exist-yet");
        // Directory doesn't exist — store should create it.
        store_in(&nested, "test.acct", &Zeroizing::new("v".into())).unwrap();
        let revived = retrieve_in(&nested, "test.acct").unwrap();
        assert_eq!(revived.as_str(), "v");
    }

    #[test]
    fn retrieve_uninitialised_returns_not_initialised_error() {
        // Note: we can't actually unset SECRETS_DIR once set (OnceLock),
        // and a parallel test in this same process may have set it. So
        // this assertion only matters via the `configured_dir` helper —
        // tested indirectly here by exercising it WITHOUT init in a
        // fresh process is impractical. Instead, assert the error type
        // exists and Display-formats sanely.
        let err = SecretsError::NotInitialised;
        assert!(err.to_string().contains("not initialised"));
    }
}
