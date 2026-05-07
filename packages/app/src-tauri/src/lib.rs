//! Trove — Tauri 2 desktop app entry point.
//!
//! Sprint 0 surface: a tray icon that toggles a single hidden-by-default
//! window, and intercepts the window's close request to hide instead of
//! quit. Sprint 1 layers on the bundled `trove-otelcol` sidecar — spawned
//! during `setup`, supervised on a background tokio task, and shut down
//! cleanly on `RunEvent::ExitRequested`.

pub mod adapters;
pub mod app_state;
pub mod collector;
pub mod detect;
pub mod harness;
pub mod ipc;
pub mod safety;
pub mod secrets;
mod tray;

use std::path::PathBuf;

use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

use crate::collector::{Supervisor, SupervisorHandle, SupervisorOptions};

/// The smoke-test Collector configuration. Sprint 1 ships this baked into
/// the binary; Sprint 5's wizard codegens a backend-specific YAML over the
/// top of this default.
const SMOKE_CONFIG_YAML: &str = include_str!("../../../../resources/otelcol/smoke-config.yaml");

/// Returns the application version baked in by Cargo at build time.
#[must_use]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Mounts the Tauri application. Called from `main.rs` (and from
/// platform-specific entry points if we ever add mobile targets).
pub fn run() {
    init_tracing();

    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ipc::commands::list_detected_harnesses,
            ipc::commands::preview_patch,
            ipc::commands::apply_patch,
            ipc::commands::revert_patch,
            ipc::commands::get_app_state,
            ipc::commands::save_backend,
            ipc::commands::clear_backend,
        ])
        .setup(|app| {
            tray::setup(app.handle())?;

            match start_collector(app.handle()) {
                Ok(handle) => {
                    app.manage(handle);
                }
                Err(e) => {
                    // The collector is critical to Trove's job, but a
                    // missing binary in dev (no `pnpm bundle:sidecar` run
                    // yet) shouldn't prevent the UI from launching.
                    // Sprint 6's dashboard surfaces the failure to the
                    // user; Sprint 1 just logs and continues.
                    tracing::error!(error = %e, "could not start collector supervisor");
                }
            }

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
        .build(tauri::generate_context!())
        .expect("error while building Trove");

    install_signal_handlers(app.handle());

    app.run(|handle, event| {
        if let RunEvent::ExitRequested { .. } = event {
            shutdown_collector(handle);
        }
    });
}

/// Bridges SIGINT / SIGTERM to `app.exit(0)` so the supervisor's
/// `RunEvent::ExitRequested` shutdown path runs even when the app is
/// killed externally (CI, kill -TERM, terminal Ctrl-C in dev). Without
/// this, the OS reaps the parent immediately and the Collector child
/// orphans onto PID 1.
#[cfg(unix)]
fn install_signal_handlers(app: &AppHandle) {
    use tokio::signal::unix::{signal, SignalKind};
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let term = signal(SignalKind::terminate());
        let int = signal(SignalKind::interrupt());
        let (Ok(mut term), Ok(mut int)) = (term, int) else {
            tracing::warn!("could not install termination signal handlers");
            return;
        };
        tokio::select! {
            _ = term.recv() => tracing::info!("SIGTERM received; exiting"),
            _ = int.recv() => tracing::info!("SIGINT received; exiting"),
        }
        app.exit(0);
    });
}

#[cfg(not(unix))]
fn install_signal_handlers(_app: &AppHandle) {}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let _ = fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .try_init();
}

fn start_collector(app: &AppHandle) -> Result<SupervisorHandle, CollectorBootError> {
    let binary_path = sidecar_binary_path()?;
    let config_path = ensure_collector_config(app)?;
    let log_path = ensure_log_path(app)?;

    tracing::info!(
        ?binary_path,
        ?config_path,
        ?log_path,
        "starting trove-otelcol supervisor",
    );

    let opts = SupervisorOptions::new(binary_path, config_path, log_path);
    let handle = Supervisor::start(opts)?;
    Ok(handle)
}

fn shutdown_collector(handle: &AppHandle) {
    let Some(state) = handle.try_state::<SupervisorHandle>() else {
        return;
    };
    tracing::info!("shutting down trove-otelcol supervisor");
    tauri::async_runtime::block_on(state.shutdown());
}

fn sidecar_binary_path() -> Result<PathBuf, CollectorBootError> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or(CollectorBootError::ExeWithoutParent)?
        .to_path_buf();
    let name = if cfg!(target_os = "windows") {
        "trove-otelcol.exe"
    } else {
        "trove-otelcol"
    };
    Ok(dir.join(name))
}

fn ensure_collector_config(app: &AppHandle) -> Result<PathBuf, CollectorBootError> {
    let dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("collector.yaml");
    if !path.exists() {
        std::fs::write(&path, SMOKE_CONFIG_YAML)?;
    }
    Ok(path)
}

fn ensure_log_path(app: &AppHandle) -> Result<PathBuf, CollectorBootError> {
    let dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("collector.log"))
}

#[derive(Debug, thiserror::Error)]
enum CollectorBootError {
    #[error("could not resolve sidecar binary path: current_exe has no parent")]
    ExeWithoutParent,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
    #[error(transparent)]
    Start(#[from] crate::collector::StartError),
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
