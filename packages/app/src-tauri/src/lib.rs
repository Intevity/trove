//! Trove — Tauri 2 desktop app entry point.
//!
//! Sprint 0 surface: a tray icon that toggles a single hidden-by-default
//! window, and intercepts the window's close request to hide instead of
//! quit. Future sprints layer on detection, adapters, the Collector
//! sidecar, and the dashboard UI.

mod tray;

use tauri::WindowEvent;

/// Returns the application version baked in by Cargo at build time.
#[must_use]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Mounts the Tauri application. Called from `main.rs` (and from
/// platform-specific entry points if we ever add mobile targets).
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            tray::setup(app.handle())?;
            Ok(())
        })
        // Intercept the close button (red-X / Cmd+W) on the tray window: hide
        // instead of close, so the app keeps running in the menubar. Mirrors
        // the claude-sentinel pattern. Scoped to the "main" window label only;
        // other windows (added in future sprints) get default close behavior.
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Trove");
}

#[cfg(test)]
mod tests {
    use super::app_version;

    #[test]
    fn app_version_matches_cargo_pkg_version() {
        assert_eq!(app_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn app_version_is_semver_shaped() {
        let v = app_version();
        let parts: Vec<&str> = v.split('.').collect();
        assert_eq!(parts.len(), 3, "expected MAJOR.MINOR.PATCH, got {v}");
        for part in parts {
            assert!(
                part.chars().all(|c| c.is_ascii_digit()),
                "non-numeric segment in version: {v}"
            );
        }
    }
}
