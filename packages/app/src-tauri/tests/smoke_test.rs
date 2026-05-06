//! Sprint 0 smoke test: confirms the lib compiles and that the version helper
//! returns the value baked in by Cargo. Sprint 1 replaces this with the real
//! sidecar integration test (spawn the bundled `trove-otelcol`, hit `:13133`,
//! assert health).

use trove_app::app_version;

#[test]
fn app_version_returns_pkg_version() {
    assert_eq!(app_version(), env!("CARGO_PKG_VERSION"));
}
