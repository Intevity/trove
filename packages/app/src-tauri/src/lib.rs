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
pub mod identity;
pub mod ipc;
pub mod log_watcher;
pub mod mappings;
pub mod otlp_emit;
pub mod safety;
pub mod secrets;
pub mod tier3_watchers;
mod tray;
mod tray_icon_render;

use std::path::PathBuf;

use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

use std::sync::Mutex;

use crate::collector::{
    CollectorState, MetricsTap, MetricsTapHandle, MetricsTapOptions, Supervisor,
    SupervisorChannels, SupervisorHandle, SupervisorOptions, SupervisorState,
};
use crate::tier3_watchers::TierThreeWatchers;

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
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            ipc::commands::list_detected_harnesses,
            ipc::commands::preview_patch,
            ipc::commands::apply_patch,
            ipc::commands::revert_patch,
            ipc::commands::resolve_conflict,
            ipc::commands::get_app_state,
            ipc::commands::save_backend,
            ipc::commands::clear_backend,
            ipc::commands::test_export,
            ipc::commands::set_auto_update_enabled,
            ipc::commands::check_for_updates,
            ipc::commands::set_identity_enabled,
            ipc::commands::set_identity_manual,
            ipc::commands::set_identity_auto,
            ipc::commands::resolve_identity_preview,
            ipc::commands::apply_mappings,
            ipc::commands::reset_mappings_to_defaults,
            ipc::collector_status::get_collector_status,
            ipc::collector_status::get_metrics_snapshot,
            ipc::collector_status::get_collector_log_tail,
            ipc::collector_status::dev_set_tray_color,
        ])
        .setup(|app| {
            // Resolve the per-user config dir (same dir that holds
            // `state.json`) and hand it to the secrets module BEFORE any
            // code that might `secrets::retrieve` / `secrets::store` runs.
            // `prepare_collector_runtime` in `start_collector` below reads
            // the saved backend's credentials via this path.
            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|e| std::io::Error::other(format!("app_config_dir: {e}")))?;
            crate::secrets::init(config_dir);

            // SupervisorChannels lives outside the SupervisorHandle so
            // dashboard subscribers (tray, IPC) survive reload_collector:
            // every reload constructs a fresh handle but keeps publishing
            // into the same watch::Sender / broadcast::Sender. The
            // metrics tap follows the same pattern — its watch sender is
            // owned by the long-lived MetricsTapHandle, never recycled.
            let channels = SupervisorChannels::new();
            app.manage::<SupervisorChannels>(channels.clone());

            let metrics = MetricsTap::start(MetricsTapOptions::default());
            app.manage::<MetricsTapHandle>(metrics);

            // Sprint 9 PR 1: registry slot for Tier 3 watchers. Empty
            // until Sprint 9 PR 2/3 wire `apply_patch` to spawn
            // adapter-specific watchers; pre-registered so the IPC
            // commands can `app.state::<TierThreeWatchers>()` without
            // a presence check.
            app.manage::<TierThreeWatchers>(TierThreeWatchers::new());

            // Always register a SupervisorState slot — even when the
            // initial spawn fails (missing sidecar binary in dev, etc.).
            // save_backend can spawn a fresh supervisor into the empty
            // slot once the user finishes the wizard.
            let initial = match start_collector(app.handle(), channels.clone()) {
                Ok(handle) => Some(handle),
                Err(e) => {
                    tracing::error!(error = %e, "could not start collector supervisor");
                    None
                }
            };
            app.manage::<SupervisorState>(Mutex::new(initial));

            // Background tasks that forward state/metrics/log updates
            // from the long-lived channels onto Tauri's emit channel.
            // Must run after all managed-state slots are registered
            // because the pumps look them up via app.state::<...>().
            ipc::collector_status::spawn_event_pumps(app.handle());

            // Tray setup runs after the Supervisor* slots are registered
            // so PR 2 can subscribe to the watch / broadcast channels
            // from inside `tray::setup` without a deferred lookup.
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

fn start_collector(
    app: &AppHandle,
    channels: SupervisorChannels,
) -> Result<SupervisorHandle, CollectorBootError> {
    let binary_path = sidecar_binary_path()?;
    let (config_path, env) = prepare_collector_runtime(app)?;
    let log_path = ensure_log_path(app)?;
    let pid_path = collector_pid_path(app)?;

    tracing::info!(
        ?binary_path,
        ?config_path,
        ?log_path,
        ?pid_path,
        env_keys = ?env.keys().collect::<Vec<_>>(),
        "starting trove-otelcol supervisor",
    );

    let opts = SupervisorOptions::new(binary_path, config_path, log_path)
        .with_pid_file_path(pid_path)
        .with_env(env);
    let handle = Supervisor::start(opts, channels)?;
    Ok(handle)
}

/// Atomically rewrite `collector.yaml` and recycle the supervised
/// sidecar. Sprint 5 PR 2 calls this from `save_backend` (with codegen
/// output) and `clear_backend` (with the smoke config). The shutdown
/// of the previous child happens outside the [`SupervisorState`] lock
/// so we never hold a [`std::sync::Mutex`] across an await.
///
/// The ~200ms gap between the old child exiting and the new child
/// passing its health check is bounded by [`SupervisorOptions::shutdown_grace`]
/// and the OS spawn cost. Harness OTLP clients buffer and retry through
/// a gap of that size, so signals already in flight are not dropped in
/// practice.
pub fn reload_collector<S: std::hash::BuildHasher>(
    app: &AppHandle,
    yaml: &str,
    env: std::collections::HashMap<String, String, S>,
) -> Result<(), CollectorBootError> {
    let binary_path = sidecar_binary_path()?;
    let config_path = collector_config_path(app)?;
    let log_path = ensure_log_path(app)?;
    let pid_path = collector_pid_path(app)?;

    safety::atomic::write_atomic(&config_path, yaml.as_bytes())?;

    // Take the existing handle out, drop the lock, await shutdown,
    // start fresh, then re-acquire the lock to insert the new handle.
    // Holding the std::sync::Mutex across `block_on(...shutdown())`
    // would make the whole thing a !Send mess.
    let previous = {
        let state = app.state::<SupervisorState>();
        let mut guard = state.lock().expect("supervisor state poisoned");
        guard.take()
    };
    if let Some(prev) = previous {
        tauri::async_runtime::block_on(prev.shutdown());
    }

    tracing::info!(
        ?binary_path,
        ?config_path,
        ?log_path,
        env_keys = ?env.keys().collect::<Vec<_>>(),
        "restarting trove-otelcol with new backend config",
    );
    // Re-collect into the supervisor's default-hasher HashMap. The
    // `S: BuildHasher` parameter exists only to keep callers flexible
    // (clippy::implicit_hasher); SupervisorOptions stores a concrete
    // map and doesn't propagate the generic.
    let env: std::collections::HashMap<String, String> = env.into_iter().collect();
    let opts = SupervisorOptions::new(binary_path, config_path, log_path)
        .with_pid_file_path(pid_path)
        .with_env(env);
    // Reuse the long-lived channels so existing subscribers (tray,
    // dashboard hooks) keep observing transitions across the reload.
    let channels = app.state::<SupervisorChannels>().inner().clone();
    let new_handle = Supervisor::start(opts, channels)?;
    // Wait for the new child to bind its OTLP ports before returning so
    // a follow-up call (notably `test_export` from the wizard) doesn't
    // race the spawn and hit a connection-refused on 127.0.0.1:4318.
    // The supervise_loop publishes Running once the health probe sees a
    // 200 on `:13133/health`; Failed/Crashed are also terminal here.
    tauri::async_runtime::block_on(wait_until_ready(
        new_handle.subscribe(),
        READY_WAIT_TIMEOUT,
    ))?;
    {
        let state = app.state::<SupervisorState>();
        let mut guard = state.lock().expect("supervisor state poisoned");
        *guard = Some(new_handle);
    }
    Ok(())
}

/// How long [`reload_collector`] blocks waiting for the new child to
/// pass its health probe before giving up. 10s is comfortably above the
/// observed spawn-to-bind cost on macOS/Linux (~400ms cold, <100ms warm)
/// and below the wizard's test-export budget so the user never sees a
/// confused two-error stack.
const READY_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn wait_until_ready(
    mut rx: tokio::sync::watch::Receiver<CollectorState>,
    timeout: std::time::Duration,
) -> Result<(), CollectorBootError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match &*rx.borrow_and_update() {
            CollectorState::Running { .. } => return Ok(()),
            CollectorState::Failed { reason } => {
                return Err(CollectorBootError::ReadyFailed(reason.clone()));
            }
            CollectorState::Crashed { restarts } => {
                return Err(CollectorBootError::ReadyFailed(format!(
                    "collector exited during startup (restarts={restarts})",
                )));
            }
            CollectorState::Stopped => {
                return Err(CollectorBootError::ReadyFailed(
                    "collector supervisor stopped before reaching running state".to_string(),
                ));
            }
            CollectorState::Idle | CollectorState::Starting { .. } | CollectorState::Stopping => {}
        }
        match tokio::time::timeout_at(deadline, rx.changed()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => return Err(CollectorBootError::ReadyTimeout(timeout)),
        }
    }
}

/// The bytes of the smoke configuration the supervisor falls back to
/// when no backend has been chosen yet. Exposed so `clear_backend` can
/// rewrite `collector.yaml` to the pass-through default.
#[must_use]
pub fn smoke_config_yaml() -> &'static str {
    SMOKE_CONFIG_YAML
}

fn shutdown_collector(handle: &AppHandle) {
    let Some(state) = handle.try_state::<SupervisorState>() else {
        return;
    };
    let supervisor = {
        let mut guard = state.lock().expect("supervisor state poisoned");
        guard.take()
    };
    if let Some(supervisor) = supervisor {
        tracing::info!("shutting down trove-otelcol supervisor");
        tauri::async_runtime::block_on(supervisor.shutdown());
    }
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

/// Reconcile `collector.yaml` and the env map the supervisor must
/// set on the child against the user's saved `state.json` at boot
/// time. Three branches:
///
/// - `state.json` carries a backend: re-render YAML + env via
///   [`collector::codegen::render`] (resolving secrets from the
///   keychain) and atomically write the YAML so the file on disk
///   matches what the supervisor is about to spawn against.
/// - `state.json` has no backend or fails to load: write the bundled
///   smoke config and return an empty env map. The collector starts
///   in pass-through mode while the wizard runs.
///
/// Without this, the supervisor at boot reads the previous session's
/// `collector.yaml` (which references `${env:TROVE_SIGNOZ_*}`
/// placeholders) but spawns the child with an empty env, so the
/// `SigNoz` exporter fails initialization with `requires a non-empty
/// "endpoint"` and the user sees "Sidecar down". The bug was masked
/// until the orphan reaper started killing the previous session's
/// long-lived child that held those env vars in-process.
fn prepare_collector_runtime(
    app: &AppHandle,
) -> Result<(PathBuf, std::collections::HashMap<String, String>), CollectorBootError> {
    let config_path = collector_config_path(app)?;
    let (yaml, env) = match app_state::load(app) {
        Ok(state) => match &state.backend {
            None => (SMOKE_CONFIG_YAML.to_string(), std::collections::HashMap::new()),
            Some(backend) => match collector::codegen::render(backend) {
                Ok(rendered) => {
                    let env: std::collections::HashMap<String, String> = rendered
                        .env
                        .into_iter()
                        .map(|(k, v)| (k, v.to_string()))
                        .collect();
                    // Apply opt-in identity overlay. Detection sweep
                    // runs synchronously here so per-harness probes
                    // (currently stubs) see the same harness state the
                    // dashboard does. With identity disabled, the
                    // overlay returns yaml unchanged.
                    let harnesses = if state.identity.enabled {
                        detect::detect_all()
                    } else {
                        Vec::new()
                    };
                    let resolved = identity::resolve(&state.identity, &harnesses);
                    let yaml_with_identity = if state.identity.enabled {
                        collector::codegen::apply_identity_overlay(rendered.yaml, &resolved)
                    } else {
                        rendered.yaml
                    };
                    // Layer the Tier A mapping overlay on top of the
                    // identity overlay. Order matters: the mapping
                    // overlay's pipeline-list edit anchors on both the
                    // baseline and the identity-augmented form, so it's
                    // order-independent, but running identity first
                    // keeps `resource/identity` at the tail of the
                    // pipeline (it tags every emission, including the
                    // synthesized Tier A metrics).
                    let yaml = collector::codegen::apply_mapping_overlay(
                        yaml_with_identity,
                        &state.mappings,
                    );
                    (yaml, env)
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "could not render collector config from saved state; using smoke fallback",
                    );
                    (SMOKE_CONFIG_YAML.to_string(), std::collections::HashMap::new())
                }
            },
        },
        Err(e) => {
            tracing::warn!(
                error = %e,
                "could not load app state at boot; using smoke collector config",
            );
            (SMOKE_CONFIG_YAML.to_string(), std::collections::HashMap::new())
        }
    };
    safety::atomic::write_atomic(&config_path, yaml.as_bytes())?;
    Ok((config_path, env))
}

/// Resolve `collector.yaml`'s absolute path, creating the parent
/// directory if missing. Unlike [`ensure_collector_config`], does not
/// write any default content — used by [`reload_collector`] which
/// always overwrites the file via an atomic write.
fn collector_config_path(app: &AppHandle) -> Result<PathBuf, CollectorBootError> {
    let dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("collector.yaml"))
}

fn ensure_log_path(app: &AppHandle) -> Result<PathBuf, CollectorBootError> {
    let dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("collector.log"))
}

/// Resolve the path used by the supervisor to record the spawned
/// child's PID. Lives next to `collector.yaml` in the app data dir.
/// On the next launch, the supervisor reads this file to detect any
/// orphaned `trove-otelcol` process from a previous session that
/// otherwise blocks port 8888 and triggers a perpetual crashloop.
fn collector_pid_path(app: &AppHandle) -> Result<PathBuf, CollectorBootError> {
    let dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("collector.pid"))
}

/// Public accessor for the collector log path the supervisor tees its
/// child output to. Used by the `test_export` IPC command to scan for
/// otelcol exporter failure markers after sending its synthetic payload.
pub fn collector_log_path(app: &AppHandle) -> Result<PathBuf, CollectorBootError> {
    ensure_log_path(app)
}

/// Failures from `start_collector` / `reload_collector`. Visible as
/// `pub` because `reload_collector` is itself `pub` (called by the IPC
/// layer) and Rust's private-interfaces lint forbids returning a
/// `pub(crate)` type from a `pub fn`.
#[derive(Debug, thiserror::Error)]
pub enum CollectorBootError {
    #[error("could not resolve sidecar binary path: current_exe has no parent")]
    ExeWithoutParent,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
    #[error(transparent)]
    Start(#[from] crate::collector::StartError),
    /// The supervisor spawned successfully but did not reach
    /// `CollectorState::Running` within the deadline. Returned by
    /// [`reload_collector`] so callers (notably `save_backend`) don't
    /// hand control back to the UI while the new child is still binding
    /// its OTLP ports.
    #[error("collector did not become ready within {0:?}")]
    ReadyTimeout(std::time::Duration),
    /// The supervisor transitioned to `Failed`, `Crashed`, or `Stopped`
    /// while we were waiting for `Running`. `reason` captures the
    /// supervise-loop's own explanation when available.
    #[error("collector failed to reach a ready state: {0}")]
    ReadyFailed(String),
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
