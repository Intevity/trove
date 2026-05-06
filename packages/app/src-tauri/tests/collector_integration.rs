//! End-to-end test for the trove-otelcol supervisor.
//!
//! This test spawns the bundled Collector binary against a tempdir-located
//! copy of the smoke configuration, asserts the health endpoint comes up,
//! kills the child externally, asserts the supervisor restarts it on a
//! fresh PID, then asserts that `shutdown()` cleanly tears everything
//! down.
//!
//! The test is skipped (with a printed notice) when no binary can be
//! located, so the Ubuntu CI lane that does not build the Collector
//! still passes. The macOS-14 `sidecar-mac` lane in CI sets
//! `TROVE_TEST_COLLECTOR_BIN` after running `pnpm build:collector`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use tokio::time::sleep;
use trove_app::collector::{
    CollectorState, Supervisor, SupervisorHandle, SupervisorOptions,
};

const SMOKE_CONFIG: &str = include_str!("../../../../resources/otelcol/smoke-config.yaml");

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_spawns_restarts_and_shuts_down_cleanly() {
    let Some(binary_path) = locate_binary() else {
        eprintln!(
            "[collector_integration] skipping: no trove-otelcol binary found. \
             Set TROVE_TEST_COLLECTOR_BIN, or run `pnpm build:collector && \
             pnpm bundle:sidecar` from the repo root."
        );
        return;
    };

    let tmp = tempfile::tempdir().expect("create tempdir");
    let config_path = tmp.path().join("collector.yaml");
    std::fs::write(&config_path, SMOKE_CONFIG).expect("write smoke config");
    let log_path = tmp.path().join("collector.log");

    let mut opts = SupervisorOptions::new(binary_path, config_path, log_path);
    // Tighten the test loop. The defaults assume real-world latency budgets;
    // here we want fast feedback.
    opts.health_timeout = Duration::from_secs(5);
    opts.health_poll_interval = Duration::from_millis(50);
    opts.restart_initial_backoff = Duration::from_millis(200);
    opts.restart_max_backoff = Duration::from_secs(1);
    opts.shutdown_grace = Duration::from_secs(3);

    let handle = Supervisor::start(opts).expect("supervisor starts");

    let pid_first = wait_for_running(&handle, Duration::from_secs(8))
        .await
        .expect("collector reaches Running within 8s of spawn");
    eprintln!("[collector_integration] first PID: {pid_first}");

    // Acceptance: external kill triggers a restart within 5s.
    kill_pid(pid_first);
    let pid_second = wait_for_running_other_than(pid_first, &handle, Duration::from_secs(8))
        .await
        .expect("supervisor restarts collector after external kill");
    assert_ne!(pid_first, pid_second, "supervisor must spawn a new PID");
    eprintln!("[collector_integration] restarted PID: {pid_second}");

    // Acceptance: shutdown() cleans up the child (no zombie).
    handle.shutdown().await;
    assert_eq!(
        handle.state(),
        CollectorState::Stopped,
        "supervisor should be Stopped after shutdown"
    );
    assert!(
        wait_until_pid_gone(pid_second, Duration::from_secs(3)),
        "child PID {pid_second} should exit within shutdown grace",
    );
}

fn locate_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TROVE_TEST_COLLECTOR_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let triple = host_triple()?;
    let ext = if triple.contains("windows") { ".exe" } else { "" };
    let candidate = crate_root
        .join("binaries")
        .join(format!("trove-otelcol-{triple}{ext}"));
    candidate.exists().then_some(candidate)
}

fn host_triple() -> Option<String> {
    let out = Command::new("rustc").arg("-vV").output().ok()?.stdout;
    let s = String::from_utf8(out).ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("host: ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .status();
    }
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // `kill -0` checks for existence + permission to signal.
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
    }
}

fn wait_until_pid_gone(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !pid_alive(pid)
}

async fn wait_for_running(handle: &SupervisorHandle, timeout: Duration) -> Option<u32> {
    let deadline = Instant::now() + timeout;
    let mut rx = handle.subscribe();
    loop {
        if let CollectorState::Running { pid, .. } = &*rx.borrow() {
            return Some(*pid);
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        match tokio::time::timeout(remaining, rx.changed()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => return None,
        }
    }
}

async fn wait_for_running_other_than(
    excluded_pid: u32,
    handle: &SupervisorHandle,
    timeout: Duration,
) -> Option<u32> {
    let deadline = Instant::now() + timeout;
    let mut rx = handle.subscribe();
    loop {
        if let CollectorState::Running { pid, .. } = &*rx.borrow() {
            if *pid != excluded_pid {
                return Some(*pid);
            }
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if !matches!(
            tokio::time::timeout(remaining, rx.changed()).await,
            Ok(Ok(()))
        ) {
            // Fallback: poll once more in case state didn't change but a
            // transient out-of-order event slipped through.
            sleep(Duration::from_millis(50)).await;
            if let CollectorState::Running { pid, .. } = &*rx.borrow() {
                if *pid != excluded_pid {
                    return Some(*pid);
                }
            }
            return None;
        }
    }
}
