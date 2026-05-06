//! Supervisor for the bundled `trove-otelcol` sidecar.
//!
//! Spawns the Collector as a child process, polls its health endpoint,
//! restarts it on unexpected exit (with exponential backoff), and shuts
//! it down cleanly on app exit. State transitions are published via a
//! [`tokio::sync::watch`] channel that Sprint 6 will surface to the UI.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use tauri::async_runtime::JoinHandle;
use thiserror::Error;
use tokio::io::BufReader;
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, watch};
use tokio::time::{Instant, sleep};

use super::{health, logs};

/// Default health endpoint exposed by the bundled smoke configuration.
pub const DEFAULT_HEALTH_URL: &str = "http://127.0.0.1:13133/health";

/// Tunables for the supervisor task. Constructed once and held for the
/// lifetime of the supervisor.
#[derive(Clone, Debug)]
pub struct SupervisorOptions {
    /// Absolute path to the `trove-otelcol` binary.
    pub binary_path: PathBuf,
    /// Absolute path to the YAML config passed via `--config`.
    pub config_path: PathBuf,
    /// File the Collector's stdout/stderr is appended to (with rotation
    /// at [`logs::MAX_LOG_BYTES`]).
    pub log_path: PathBuf,
    /// URL to GET when checking startup health.
    pub health_url: String,
    /// How long to wait after spawn for `health_url` to return 200.
    pub health_timeout: Duration,
    /// How often to retry the health probe while waiting.
    pub health_poll_interval: Duration,
    /// Initial wait between an unhealthy exit and the next spawn.
    pub restart_initial_backoff: Duration,
    /// Cap on the exponential backoff between respawns.
    pub restart_max_backoff: Duration,
    /// A run that survived at least this long is treated as healthy and
    /// resets the restart backoff to its initial value.
    pub restart_healthy_threshold: Duration,
    /// How long to wait for the child to exit after a graceful kill
    /// before escalating to SIGKILL.
    pub shutdown_grace: Duration,
}

impl SupervisorOptions {
    #[must_use]
    pub fn new(binary_path: PathBuf, config_path: PathBuf, log_path: PathBuf) -> Self {
        Self {
            binary_path,
            config_path,
            log_path,
            health_url: DEFAULT_HEALTH_URL.to_string(),
            health_timeout: Duration::from_secs(10),
            health_poll_interval: Duration::from_millis(100),
            restart_initial_backoff: Duration::from_millis(500),
            restart_max_backoff: Duration::from_secs(5),
            restart_healthy_threshold: Duration::from_secs(30),
            shutdown_grace: Duration::from_secs(5),
        }
    }
}

/// Public state machine for the supervised Collector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollectorState {
    /// Initial state before the first spawn attempt.
    Idle,
    /// Process spawned but health probe has not yet succeeded.
    Starting { pid: u32 },
    /// Health probe returned 200; Collector is accepting OTLP traffic.
    Running { pid: u32, restarts: u32 },
    /// The previous child exited unexpectedly; supervisor is in backoff.
    Crashed { restarts: u32 },
    /// Shutdown requested; child is being terminated.
    Stopping,
    /// Supervisor task has exited cleanly.
    Stopped,
    /// Fatal error: child cannot be spawned at all.
    Failed { reason: String },
}

#[derive(Debug, Error)]
pub enum StartError {
    #[error("collector binary not found at {0:?}")]
    BinaryNotFound(PathBuf),
    #[error("config file not found at {0:?}")]
    ConfigNotFound(PathBuf),
}

/// Entry point. Spawns the supervisor task and returns a handle for state
/// inspection and shutdown.
pub struct Supervisor;

impl Supervisor {
    pub fn start(opts: SupervisorOptions) -> Result<SupervisorHandle, StartError> {
        if !opts.binary_path.exists() {
            return Err(StartError::BinaryNotFound(opts.binary_path));
        }
        if !opts.config_path.exists() {
            return Err(StartError::ConfigNotFound(opts.config_path));
        }

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let (state_tx, state_rx) = watch::channel(CollectorState::Idle);

        // Spawn on Tauri's runtime so this works from `setup` (the main
        // thread, which has no current tokio runtime). Also works from
        // `#[tokio::test]` since `tauri::async_runtime` lazy-inits its
        // own runtime independent of the test's. Inner spawns inside the
        // supervise loop can use `tokio::spawn` because they run from
        // within an async context where a runtime is current.
        let task_opts = opts;
        let join = tauri::async_runtime::spawn(async move {
            supervise_loop(task_opts, shutdown_rx, state_tx).await;
        });

        Ok(SupervisorHandle {
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
            state_rx,
            join: Mutex::new(Some(join)),
        })
    }
}

/// Owns the supervisor task. Drop without [`shutdown`](Self::shutdown)
/// terminates the child via `kill_on_drop`, but state transitions may
/// not be observed.
///
/// The `state` and `subscribe` methods are part of the supervisor's
/// public API — Sprint 6 wires them into the dashboard and tray icon.
/// They are exercised by the integration test in
/// `tests/collector_integration.rs` rather than by lib-internal code.
#[allow(dead_code)]
pub struct SupervisorHandle {
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    state_rx: watch::Receiver<CollectorState>,
    join: Mutex<Option<JoinHandle<()>>>,
}

#[allow(dead_code)]
impl SupervisorHandle {
    /// Snapshot the current state.
    pub fn state(&self) -> CollectorState {
        self.state_rx.borrow().clone()
    }

    /// Subscribe to state transitions. The returned receiver always has
    /// the current state available via `borrow`.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<CollectorState> {
        self.state_rx.clone()
    }

    /// Request a graceful shutdown and wait for the supervisor task to
    /// exit. Idempotent: subsequent calls are no-ops.
    pub async fn shutdown(&self) {
        let tx = {
            let mut guard = self.shutdown_tx.lock().expect("poisoned shutdown mutex");
            guard.take()
        };
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
        let join = {
            let mut guard = self.join.lock().expect("poisoned join mutex");
            guard.take()
        };
        if let Some(join) = join {
            let _ = join.await;
        }
    }
}

async fn supervise_loop(
    opts: SupervisorOptions,
    mut shutdown_rx: oneshot::Receiver<()>,
    state_tx: watch::Sender<CollectorState>,
) {
    let mut backoff = opts.restart_initial_backoff;
    let mut restarts: u32 = 0;

    loop {
        // Honor shutdown that arrived during a previous backoff window.
        match shutdown_rx.try_recv() {
            Ok(()) | Err(oneshot::error::TryRecvError::Closed) => {
                state_tx.send_replace(CollectorState::Stopped);
                return;
            }
            Err(oneshot::error::TryRecvError::Empty) => {}
        }

        let mut child = match spawn_child(&opts) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, binary = ?opts.binary_path, "failed to spawn collector");
                state_tx.send_replace(CollectorState::Failed {
                    reason: format!("spawn failed: {e}"),
                });
                return;
            }
        };

        let pid = child.id().unwrap_or(0);
        tracing::info!(pid, "trove-otelcol spawned");
        state_tx.send_replace(CollectorState::Starting { pid });

        spawn_log_tee(
            child.stdout.take(),
            opts.log_path.clone(),
            tracing::Level::INFO,
            "stdout",
        );
        spawn_log_tee(
            child.stderr.take(),
            opts.log_path.clone(),
            // The OTel Collector emits its own info/warn/error logs to
            // stderr; don't re-level them upward. Sprint 6's dashboard
            // can parse the embedded level from the collector's
            // structured output if it wants finer granularity.
            tracing::Level::INFO,
            "stderr",
        );

        let health_join = spawn_health_probe(&opts, pid, restarts, state_tx.clone());

        let started_at = Instant::now();

        tokio::select! {
            exit = child.wait() => {
                health_join.abort();
                let elapsed = started_at.elapsed();
                tracing::warn!(?exit, ?elapsed, restarts, "collector exited");
                if elapsed >= opts.restart_healthy_threshold {
                    backoff = opts.restart_initial_backoff;
                }
                restarts = restarts.saturating_add(1);
                state_tx.send_replace(CollectorState::Crashed { restarts });
            }
            _ = &mut shutdown_rx => {
                health_join.abort();
                state_tx.send_replace(CollectorState::Stopping);
                terminate_child(&mut child, opts.shutdown_grace).await;
                state_tx.send_replace(CollectorState::Stopped);
                return;
            }
        }

        // Backoff before respawn. Surface a shutdown that arrives during
        // the wait so we don't spin up a child only to immediately kill it.
        tokio::select! {
            () = sleep(backoff) => {}
            _ = &mut shutdown_rx => {
                state_tx.send_replace(CollectorState::Stopped);
                return;
            }
        }
        backoff = (backoff * 2).min(opts.restart_max_backoff);
    }
}

fn spawn_child(opts: &SupervisorOptions) -> std::io::Result<Child> {
    let mut cmd = Command::new(&opts.binary_path);
    cmd.arg("--config").arg(&opts.config_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    cmd.spawn()
}

fn spawn_log_tee<R>(
    stream: Option<R>,
    log_path: PathBuf,
    level: tracing::Level,
    label: &'static str,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    if let Some(s) = stream {
        tauri::async_runtime::spawn(async move {
            logs::tee_stream(BufReader::new(s), log_path, level, label).await;
        });
    }
}

fn spawn_health_probe(
    opts: &SupervisorOptions,
    pid: u32,
    restarts: u32,
    state_tx: watch::Sender<CollectorState>,
) -> JoinHandle<()> {
    let url = opts.health_url.clone();
    let timeout = opts.health_timeout;
    let poll = opts.health_poll_interval;
    tauri::async_runtime::spawn(async move {
        match health::wait_until_healthy(&url, timeout, poll).await {
            Ok(()) => {
                // Only promote to Running if we're still in the same incarnation
                // (Starting{pid}). A crash that beat the probe is already in
                // Crashed, and we don't want to overwrite it with a stale Running.
                state_tx.send_if_modified(|s| match s {
                    CollectorState::Starting { pid: p } if *p == pid => {
                        *s = CollectorState::Running { pid, restarts };
                        true
                    }
                    _ => false,
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "collector health probe timed out");
            }
        }
    })
}

async fn terminate_child(child: &mut Child, grace: Duration) {
    if let Err(e) = child.start_kill() {
        tracing::warn!(error = %e, "start_kill failed; child may already be gone");
    }
    if tokio::time::timeout(grace, child.wait()).await.is_err() {
        tracing::warn!("collector did not exit within grace period; force killing");
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}
