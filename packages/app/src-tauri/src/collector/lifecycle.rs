//! Supervisor for the bundled `trove-otelcol` sidecar.
//!
//! Spawns the Collector as a child process, polls its health endpoint,
//! restarts it on unexpected exit (with exponential backoff), and shuts
//! it down cleanly on app exit. State transitions are published via a
//! [`tokio::sync::watch`] channel that Sprint 6 will surface to the UI.

use std::collections::HashMap;
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::async_runtime::JoinHandle;
use thiserror::Error;
use tokio::io::BufReader;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, oneshot, watch};
use tokio::time::{Instant, sleep};

use super::{health, logs};

/// Default health endpoint exposed by the bundled smoke configuration.
pub const DEFAULT_HEALTH_URL: &str = "http://127.0.0.1:13133/health";

/// CREATE_NO_WINDOW — suppresses the console window Windows would
/// otherwise allocate when a GUI-subsystem parent (this app) spawns a
/// console-subsystem child (`trove-otelcol`, `tasklist`, `taskkill`).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    /// Environment variables to set on every spawned child. Sprint 5
    /// PR 2 uses this to pass backend-specific values
    /// (`TROVE_SIGNOZ_INGESTION_KEY`, `TROVE_HONEYCOMB_TEAM`, etc.) the
    /// `${env:...}` placeholders in `collector.yaml` reference. Always
    /// passed via [`Command::envs`](tokio::process::Command::envs) —
    /// never argv — so secrets cannot leak via `ps`.
    pub env: HashMap<String, String>,
    /// Path to a single-line file holding the most recent child PID.
    /// On startup the supervisor checks this file and reaps any
    /// surviving `trove-otelcol` process from a previous app session
    /// (e.g., a force-quit or crash that bypassed `kill_on_drop`)
    /// before spawning a new child. Without this, an orphan child
    /// holding port 18888 (the otelcol Prometheus telemetry endpoint)
    /// blocks every subsequent spawn with `bind: address already in
    /// use`, producing a perpetual crashloop. `None` disables the
    /// feature (only used by tests that don't need it).
    pub pid_file_path: Option<PathBuf>,
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
            env: HashMap::new(),
            pid_file_path: None,
        }
    }

    /// Set the path where the supervisor records the spawned child's
    /// PID. Enables orphan reaping at startup; see [`Self::pid_file_path`].
    #[must_use]
    pub fn with_pid_file_path(mut self, path: PathBuf) -> Self {
        self.pid_file_path = Some(path);
        self
    }

    /// Builder helper. Replaces any previous env map.
    #[must_use]
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
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

/// One line captured from the supervised Collector child's stdout or
/// stderr, with the originating stream tagged. Sprint 6 PR 1 broadcasts
/// these so the dashboard's logs panel can render the live tail without
/// re-reading the rotated log file from disk.
#[derive(Clone, Debug)]
pub struct CollectorLogLine {
    pub stream: &'static str,
    pub line: String,
}

/// External channels the supervisor publishes into. Hoisted out of
/// [`SupervisorHandle`] so they survive across [`reload_collector`]:
/// every reload constructs a fresh handle but reuses the same channels,
/// so subscribers (tray, dashboard) keep observing transitions instead
/// of receiving a single `Stopped` and going silent.
///
/// `state` is a `watch` because consumers only care about the latest
/// state — coalescing intermediate transitions is fine. `logs` is a
/// `broadcast` because every line matters; lagging consumers get a
/// per-receiver `Lagged` error rather than dropping the channel.
#[derive(Clone)]
pub struct SupervisorChannels {
    pub state: Arc<watch::Sender<CollectorState>>,
    pub logs: broadcast::Sender<CollectorLogLine>,
}

impl SupervisorChannels {
    /// Construct a fresh channel set seeded at [`CollectorState::Idle`]
    /// with a 1024-line broadcast buffer.
    #[must_use]
    pub fn new() -> Self {
        let (state_tx, _) = watch::channel(CollectorState::Idle);
        let (logs_tx, _) = broadcast::channel(1024);
        Self {
            state: Arc::new(state_tx),
            logs: logs_tx,
        }
    }

    /// Borrow a fresh state receiver. Always usable — `watch` channels
    /// are seeded with the current value, so `borrow()` and `changed()`
    /// behave correctly even before the supervisor has spawned a child.
    #[must_use]
    pub fn subscribe_state(&self) -> watch::Receiver<CollectorState> {
        self.state.subscribe()
    }

    /// Borrow a fresh log broadcast receiver. The receiver only sees
    /// lines emitted *after* this call; pre-existing log content lives
    /// in `collector.log` and is fetched via the
    /// `get_collector_log_tail` IPC at mount time.
    #[must_use]
    pub fn subscribe_logs(&self) -> broadcast::Receiver<CollectorLogLine> {
        self.logs.subscribe()
    }
}

impl Default for SupervisorChannels {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry point. Spawns the supervisor task and returns a handle for state
/// inspection and shutdown.
pub struct Supervisor;

impl Supervisor {
    /// Spawn a fresh supervisor task that publishes into the supplied
    /// `channels`. Callers (typically [`crate::lib::start_collector`]
    /// and [`crate::reload_collector`]) hold a long-lived
    /// [`SupervisorChannels`] and pass the same instance into every
    /// invocation, so subscribers persist across reloads.
    pub fn start(
        opts: SupervisorOptions,
        channels: SupervisorChannels,
    ) -> Result<SupervisorHandle, StartError> {
        if !opts.binary_path.exists() {
            return Err(StartError::BinaryNotFound(opts.binary_path));
        }
        if !opts.config_path.exists() {
            return Err(StartError::ConfigNotFound(opts.config_path));
        }

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        // Reset the shared watch to Idle so a previous `Stopped` /
        // `Failed` reading doesn't linger after a reload. `send_replace`
        // notifies subscribers without requiring at least one Receiver,
        // which is correct here even before the dashboard is mounted.
        channels.state.send_replace(CollectorState::Idle);

        // Spawn on Tauri's runtime so this works from `setup` (the main
        // thread, which has no current tokio runtime). Also works from
        // `#[tokio::test]` since `tauri::async_runtime` lazy-inits its
        // own runtime independent of the test's. Inner spawns inside the
        // supervise loop can use `tokio::spawn` because they run from
        // within an async context where a runtime is current.
        let task_opts = opts;
        let task_channels = channels.clone();
        let join = tauri::async_runtime::spawn(async move {
            supervise_loop(task_opts, shutdown_rx, task_channels).await;
        });

        Ok(SupervisorHandle {
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
            channels,
            join: Mutex::new(Some(join)),
        })
    }
}

/// Owns the supervisor task. Drop without [`shutdown`](Self::shutdown)
/// terminates the child via `kill_on_drop`, but state transitions may
/// not be observed.
///
/// `channels` is the shared [`SupervisorChannels`] held by the caller —
/// the same instance is passed to every [`Supervisor::start`] call, so
/// subscribers survive across [`crate::reload_collector`].
#[allow(dead_code)]
pub struct SupervisorHandle {
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    channels: SupervisorChannels,
    join: Mutex<Option<JoinHandle<()>>>,
}

#[allow(dead_code)]
impl SupervisorHandle {
    /// Snapshot the current state.
    pub fn state(&self) -> CollectorState {
        self.channels.state.borrow().clone()
    }

    /// Subscribe to state transitions. The returned receiver always has
    /// the current state available via `borrow`.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<CollectorState> {
        self.channels.subscribe_state()
    }

    /// Subscribe to per-line collector log output emitted while this
    /// handle is alive. Returns a fresh broadcast receiver — pre-existing
    /// lines live on disk and are fetched via `get_collector_log_tail`.
    #[must_use]
    pub fn subscribe_logs(&self) -> broadcast::Receiver<CollectorLogLine> {
        self.channels.subscribe_logs()
    }

    /// Borrow the shared channel set so the caller can hand a clone to
    /// a future [`Supervisor::start`] without going through this handle.
    #[must_use]
    pub fn channels(&self) -> SupervisorChannels {
        self.channels.clone()
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
    channels: SupervisorChannels,
) {
    let state_tx = channels.state.clone();
    let log_tx = channels.logs.clone();
    let mut backoff = opts.restart_initial_backoff;
    let mut restarts: u32 = 0;

    // Reap any orphaned collector left behind by a prior app session
    // (force-quit, crash, or installer replacing the bundle without a
    // clean shutdown). If we skip this, the orphan still owns
    // 127.0.0.1:18888 (the otelcol Prometheus telemetry endpoint) and
    // every spawn here fails with `bind: address already in use`,
    // producing a crashloop the user sees as "Sidecar down".
    //
    // Two reapers, defense in depth:
    //   1. `reap_orphan_collector` reads <app_data>/collector.pid (the
    //      authoritative record written after each successful spawn).
    //   2. `reap_orphans_by_name` scans the process table for any
    //      other process whose executable name matches the supervisor
    //      binary. Catches orphans from pre-fix app versions (no
    //      collector.pid existed yet) and corrupted pid files.
    if let Some(pid_path) = opts.pid_file_path.as_deref() {
        reap_orphan_collector(pid_path, &opts.binary_path).await;
    }
    reap_orphans_by_name(&opts.binary_path).await;

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

        if let Some(pid_path) = opts.pid_file_path.as_deref() {
            write_pid_file(pid_path, pid);
        }

        spawn_log_tee(
            child.stdout.take(),
            opts.log_path.clone(),
            tracing::Level::INFO,
            "stdout",
            log_tx.clone(),
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
            log_tx.clone(),
        );

        let health_join = spawn_health_probe(&opts, pid, restarts, (*state_tx).clone());

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
                if let Some(pid_path) = opts.pid_file_path.as_deref() {
                    let _ = std::fs::remove_file(pid_path);
                }
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
    // Backend-specific env vars (TROVE_SIGNOZ_INGESTION_KEY etc.) the
    // collector.yaml's ${env:...} placeholders reference. Routed via
    // `envs` rather than argv so secrets do not surface in `ps`.
    cmd.envs(&opts.env);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    // CREATE_NO_WINDOW — trove-otelcol is a console-subsystem exe; the
    // parent GUI app has no console, so without this flag Windows
    // allocates a visible console window for the child. The flag only
    // suppresses console allocation; the piped stdout/stderr feeding
    // the log tee are unaffected.
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn()
}

fn spawn_log_tee<R>(
    stream: Option<R>,
    log_path: PathBuf,
    level: tracing::Level,
    label: &'static str,
    broadcast: broadcast::Sender<CollectorLogLine>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    if let Some(s) = stream {
        tauri::async_runtime::spawn(async move {
            logs::tee_stream(BufReader::new(s), log_path, level, label, broadcast).await;
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

/// Persist `pid` to `path` via a temp-file + rename so a crash mid-write
/// can't leave a half-written file the next reap step would parse
/// incorrectly. Failures are logged but never propagated — the supervisor
/// shouldn't refuse to start because the PID file is unwritable.
fn write_pid_file(path: &std::path::Path, pid: u32) {
    if let Err(e) = crate::safety::atomic::write_atomic(path, pid.to_string().as_bytes()) {
        tracing::warn!(error = %e, ?path, "failed to write collector pid file");
    }
}

/// If `pid_path` exists and references a still-alive `trove-otelcol`
/// process from a previous app session, send SIGTERM (Unix) or
/// `TerminateProcess` (Windows) and wait up to ~3s for it to exit, then
/// escalate to SIGKILL / `taskkill /F`. Deletes the file on completion.
///
/// Defends against:
/// - The parent (Trove app) being force-quit or crashing, leaving the
///   `kill_on_drop` guarantee unfired so the child survives.
/// - An installer replacing the bundle without a clean shutdown.
///
/// PID reuse is real: another process may have inherited the PID by the
/// time we check. Mitigated by [`process_command_name`] — we only kill
/// if the running process's argv0/`COMM` resolves to the supervisor's
/// own binary name (e.g., `trove-otelcol`).
async fn reap_orphan_collector(pid_path: &std::path::Path, binary_path: &std::path::Path) {
    let Some(pid) = read_pid_file(pid_path) else {
        return;
    };
    let expected = binary_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("trove-otelcol");
    // Strip the .exe suffix on Windows so the comparison against
    // tasklist's IMAGENAME (which keeps it) works either way.
    let expected_stem = expected.trim_end_matches(".exe");

    let observed = process_command_name(pid);
    let matches = observed
        .as_deref()
        .map(|n| n.trim_end_matches(".exe"))
        .is_some_and(|n| n == expected_stem || n.contains(expected_stem));

    if !matches {
        if observed.is_some() {
            tracing::info!(
                pid,
                observed = ?observed,
                expected = expected_stem,
                "stale collector pid file refers to a different process; ignoring",
            );
        }
        let _ = std::fs::remove_file(pid_path);
        return;
    }

    tracing::warn!(
        pid,
        "found orphaned trove-otelcol from a previous session; reaping",
    );
    send_terminate_signal(pid);
    // Wait up to ~3s for graceful exit, polling at 100ms.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if !process_is_alive(pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if process_is_alive(pid) {
        tracing::warn!(pid, "orphan did not exit gracefully; force killing");
        send_kill_signal(pid);
    }
    let _ = std::fs::remove_file(pid_path);
}

/// Find every running process whose executable name matches `binary`'s
/// leaf name and reap it. Complements [`reap_orphan_collector`] for the
/// case where a pre-fix app version (which didn't write a PID file)
/// left an orphan behind, or where the PID file was corrupted.
///
/// Skips the current process so we never reach in and kill ourselves
/// (defensive — the supervisor binary differs from the collector
/// binary, but the check is cheap).
async fn reap_orphans_by_name(binary: &std::path::Path) {
    let Some(name) = binary
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
    else {
        return;
    };
    let self_pid = std::process::id();
    let mut survivors = Vec::new();
    for pid in find_processes_by_name(&name) {
        if pid == self_pid {
            continue;
        }
        tracing::warn!(
            pid,
            binary = ?binary,
            "found orphaned collector by name scan; reaping",
        );
        send_terminate_signal(pid);
        survivors.push(pid);
    }
    if survivors.is_empty() {
        return;
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        survivors.retain(|p| process_is_alive(*p));
        if survivors.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for pid in survivors {
        tracing::warn!(pid, "name-scanned orphan did not exit; force killing");
        send_kill_signal(pid);
    }
}

/// List PIDs of running processes whose executable matches `name` (its
/// leaf — `trove-otelcol` or `trove-otelcol.exe`). Returns an empty
/// vec on any error so callers can treat it as "nothing to reap."
fn find_processes_by_name(name: &str) -> Vec<u32> {
    // Strip a trailing `.exe` so callers on Windows that pass
    // `trove-otelcol.exe` still match the tasklist IMAGENAME column.
    let stem = name.trim_end_matches(".exe");
    #[cfg(unix)]
    {
        // `pgrep -x <name>` matches the executable name exactly. We
        // pass the stem; macOS's process accounting truncates `comm`
        // at 15 chars, so a longer binary name relies on the prefix
        // match `pgrep` performs by default if `-x` finds nothing.
        if let Ok(out) = std::process::Command::new("pgrep").args(["-x", stem]).output() {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|l| l.trim().parse::<u32>().ok())
                    .filter(|p| *p > 0)
                    .collect();
            }
        }
        Vec::new()
    }
    #[cfg(windows)]
    {
        let filter = format!("IMAGENAME eq {stem}.exe");
        if let Ok(out) = std::process::Command::new("tasklist")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|line| {
                        let cols: Vec<&str> = line.split(',').collect();
                        cols.get(1)
                            .and_then(|c| c.trim_matches('"').parse::<u32>().ok())
                    })
                    .filter(|p| *p > 0)
                    .collect();
            }
        }
        Vec::new()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = stem;
        Vec::new()
    }
}

fn read_pid_file(path: &std::path::Path) -> Option<u32> {
    let contents = std::fs::read_to_string(path).ok()?;
    contents.trim().parse::<u32>().ok().filter(|p| *p > 0)
}

/// Resolve the executable name of `pid` if it is currently alive.
/// Returns `None` for both "no such process" and "permission denied"
/// — in both cases the reaper treats the entry as not-our-binary and
/// leaves the process alone.
fn process_command_name(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if line.is_empty() {
            None
        } else {
            // `ps -o comm=` prints the full executable path; we want the leaf.
            Some(
                std::path::Path::new(&line)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(String::from)
                    .unwrap_or(line),
            )
        }
    }
    #[cfg(windows)]
    {
        // tasklist /FI "PID eq <pid>" /FO CSV /NH → "IMAGENAME","PID",…
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&out.stdout);
        let first = line.lines().next()?.trim();
        if first.is_empty() || first.starts_with("INFO:") {
            return None;
        }
        // CSV row: "IMAGENAME","PID",...
        let name = first.split(',').next()?.trim_matches('"').to_string();
        if name.is_empty() { None } else { Some(name) }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        None
    }
}

fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // `kill -0 <pid>` returns success iff the process exists and the
        // caller has permission to signal it. We're killing our own
        // children, so permission is implicit.
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(not(unix))]
    {
        process_command_name(pid).is_some()
    }
}

fn send_terminate_signal(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
    }
    #[cfg(windows)]
    {
        // taskkill without /F sends WM_CLOSE (graceful for GUI apps) and
        // falls back to a soft termination for console apps. For
        // trove-otelcol (a console process) this is roughly equivalent
        // to CTRL_BREAK_EVENT — enough for the otelcol shutdown handler
        // to run.
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
}

fn send_kill_signal(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_pid_file_returns_none_when_path_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_pid_file(&dir.path().join("collector.pid")).is_none());
    }

    #[test]
    fn read_pid_file_returns_none_on_garbage_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collector.pid");
        std::fs::write(&path, b"not-a-number\n").unwrap();
        assert!(read_pid_file(&path).is_none());
    }

    #[test]
    fn read_pid_file_returns_zero_value_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collector.pid");
        std::fs::write(&path, b"0\n").unwrap();
        assert!(read_pid_file(&path).is_none());
    }

    #[test]
    fn write_pid_file_round_trips_through_read_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collector.pid");
        write_pid_file(&path, 12345);
        assert_eq!(read_pid_file(&path), Some(12345));
    }

    #[test]
    fn process_command_name_for_self_pid_resolves_to_something() {
        // The test runner is alive while this test runs; we can't pin
        // the exact name (cargo-test-runner-<hash>), but we can assert
        // we got a non-empty answer back.
        let me = std::process::id();
        let name = process_command_name(me);
        assert!(name.is_some(), "expected a name for self pid {me}");
        assert!(!name.unwrap().is_empty());
    }

    #[test]
    fn process_command_name_for_nonexistent_pid_is_none() {
        // PIDs above 4_000_000 are not allocated by default on macOS or
        // Linux. If something else is running with that PID at test
        // time we just get false-positive — acceptable noise.
        assert!(process_command_name(4_000_001).is_none());
    }

    #[test]
    fn process_is_alive_for_self_pid_is_true() {
        assert!(process_is_alive(std::process::id()));
    }

    #[test]
    fn process_is_alive_for_high_pid_is_false() {
        assert!(!process_is_alive(4_000_001));
    }

    #[tokio::test]
    async fn reap_orphan_collector_is_noop_when_pid_file_absent() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("collector.pid");
        let binary = std::path::PathBuf::from("/usr/local/bin/trove-otelcol");
        // Should not panic, should not create the file.
        reap_orphan_collector(&pid_path, &binary).await;
        assert!(!pid_path.exists());
    }

    #[test]
    fn find_processes_by_name_returns_empty_for_nonsense_name() {
        assert!(find_processes_by_name("trove-totally-not-a-real-binary-xyz").is_empty());
    }

    #[tokio::test]
    async fn reap_orphans_by_name_is_safe_when_no_matches() {
        let binary = std::path::PathBuf::from(
            "/usr/local/bin/trove-totally-not-a-real-binary-xyz",
        );
        // Must complete without panicking and without affecting the
        // test runner's own process.
        reap_orphans_by_name(&binary).await;
        assert!(process_is_alive(std::process::id()));
    }

    #[tokio::test]
    async fn reap_orphan_collector_does_not_kill_unrelated_process() {
        // Write our own pid to the file but claim the supervisor's
        // binary is something unrelated. The reaper must refuse to
        // kill, but should still clean up the stale file.
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("collector.pid");
        write_pid_file(&pid_path, std::process::id());
        let binary = std::path::PathBuf::from("/usr/local/bin/trove-otelcol");
        reap_orphan_collector(&pid_path, &binary).await;
        // Self-process must still be alive.
        assert!(process_is_alive(std::process::id()));
        // Stale entry removed so the next launch starts clean.
        assert!(!pid_path.exists());
    }

    #[tokio::test]
    async fn channels_seed_subscribers_with_idle() {
        let channels = SupervisorChannels::new();
        let rx = channels.subscribe_state();
        assert_eq!(*rx.borrow(), CollectorState::Idle);
    }

    #[tokio::test]
    async fn cloned_channels_publish_into_the_same_watch() {
        // Sprint 6 PR 1 contract: reload_collector takes the existing
        // SupervisorChannels and hands a clone to the new Supervisor.
        // Cloning the channels must not orphan pre-existing receivers.
        let channels = SupervisorChannels::new();
        let mut rx = channels.subscribe_state();

        let cloned = channels.clone();
        cloned
            .state
            .send_replace(CollectorState::Running { pid: 7, restarts: 0 });

        rx.changed().await.expect("receiver still observes the publish");
        assert_eq!(
            *rx.borrow(),
            CollectorState::Running { pid: 7, restarts: 0 },
        );

        // A second cloned set (analogous to a second reload) must also
        // route to the original receiver.
        let cloned_again = channels.clone();
        cloned_again
            .state
            .send_replace(CollectorState::Crashed { restarts: 1 });
        rx.changed().await.expect("receiver still observes after another reload");
        assert_eq!(*rx.borrow(), CollectorState::Crashed { restarts: 1 });
    }

    #[tokio::test]
    async fn log_broadcast_fans_out_to_late_subscribers_only_after_subscribe() {
        // Broadcast does not buffer history for late subscribers; that's
        // the documented behaviour we rely on (initial tail comes from
        // the on-disk log file via `get_collector_log_tail`).
        let channels = SupervisorChannels::new();
        let _ = channels.logs.send(CollectorLogLine {
            stream: "stdout",
            line: "early line, never observed".into(),
        });

        let mut rx = channels.subscribe_logs();
        let _ = channels.logs.send(CollectorLogLine {
            stream: "stdout",
            line: "post-subscribe".into(),
        });

        let line = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("recv resolves quickly")
            .expect("broadcast not closed");
        assert_eq!(line.line, "post-subscribe");
    }

    #[test]
    fn channels_default_matches_new() {
        // Default::default() and ::new() should be observationally
        // identical: both seed Idle.
        let a = SupervisorChannels::default();
        assert_eq!(*a.subscribe_state().borrow(), CollectorState::Idle);
    }
}
