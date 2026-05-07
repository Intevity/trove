//! OS keychain wrapper. Trove never writes raw backend credentials to
//! disk — every secret-bearing field on the [`Backend`](crate::adapters)
//! type is replaced with an opaque [`SecretRef`](crate::adapters) handle
//! in `state.json`, and the actual value is stored under the OS keychain
//! (macOS Keychain / Linux Secret Service / Windows Credential Manager).
//!
//! This module is intentionally tiny: a thin shim over [`keyring::Entry`]
//! that pins the service name to `"trove"` and standardises the account
//! convention. Callers (`save_backend`, `clear_backend`, codegen) hold a
//! [`Zeroizing<String>`] for the duration the value is live in process
//! memory and never log it.
//!
//! ## Account naming
//!
//! - Fixed-shape backends use `backend.<kind>.<field>`. Examples:
//!   `backend.signoz.ingestion-key`, `backend.honeycomb.team`,
//!   `backend.grafana-cloud.auth`, `backend.datadog.api-key`.
//! - The variadic OTLP-generic backend uses
//!   `backend.otlp-generic.header.<headerName>` per header. The set of
//!   header names is persisted as the keys of `Backend.headers` in
//!   `state.json`, so `clear_backend` can iterate them.
//! - The `otelcol-passthrough` backend has no secrets.
//!
//! Helpers in [`accounts`] return the canonical strings so the wizard
//! UI, codegen, and tests stay in sync.

use thiserror::Error;
use zeroize::Zeroizing;

/// Service name used for every Trove keychain entry. Surfaces in the
/// system keychain UI under "trove" so the user can audit and revoke.
pub const SERVICE: &str = "trove";

/// Failures from the keychain layer. Mapped to `IpcError::Internal`
/// at the IPC boundary; the underlying [`keyring::Error`] message is
/// preserved as `reason` for diagnostics.
#[derive(Debug, Error)]
pub enum SecretsError {
    /// The platform keychain rejected the operation (locked vault on
    /// Linux, missing entitlement on macOS, etc.).
    #[error("keychain operation failed: {0}")]
    Keychain(#[from] keyring::Error),
}

/// Store `secret` under (`SERVICE`, `account`). Overwrites any existing
/// entry without warning — callers that need the prior value should
/// [`retrieve`] first.
pub fn store(account: &str, secret: &Zeroizing<String>) -> Result<(), SecretsError> {
    let entry = keyring::Entry::new(SERVICE, account)?;
    entry.set_password(secret.as_str())?;
    Ok(())
}

/// Retrieve the value previously stored for `account`. Returns the
/// secret wrapped in [`Zeroizing`] so the in-process buffer is wiped on
/// drop. Returns [`SecretsError::Keychain`] wrapping
/// [`keyring::Error::NoEntry`] when the account is unknown — callers
/// that treat that as "no secret stored" should match on it.
pub fn retrieve(account: &str) -> Result<Zeroizing<String>, SecretsError> {
    let entry = keyring::Entry::new(SERVICE, account)?;
    let value = entry.get_password()?;
    Ok(Zeroizing::new(value))
}

/// Delete the entry for `account`. Idempotent: `keyring::Error::NoEntry`
/// is swallowed so a redundant delete (e.g. clearing a half-saved
/// backend) is a no-op. Other errors propagate.
pub fn delete(account: &str) -> Result<(), SecretsError> {
    let entry = keyring::Entry::new(SERVICE, account)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
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

    /// `backend.otlp-generic.header.<headerName>`. The header name is
    /// passed through verbatim; the wizard validates it client-side.
    #[must_use]
    pub fn otlp_generic_header(header_name: &str) -> String {
        format!("backend.otlp-generic.header.{header_name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // The service name surfaces in the OS keychain UI. Renaming it
        // would orphan every existing user's stored credentials behind
        // a name they couldn't recognise — so this constant is part of
        // the persistence contract.
        assert_eq!(SERVICE, "trove");
    }
}
