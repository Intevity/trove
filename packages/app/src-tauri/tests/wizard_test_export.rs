//! End-to-end test for the wizard's "Test export" path.
//!
//! Spins a tiny stub OTLP/HTTP receiver on a localhost port, codegens
//! an `otlp-generic` collector config pointing at it, starts the real
//! `trove-otelcol` supervisor with the resulting YAML+env, and asserts
//! that `test_export_at` reports `ok` after the synthetic span flows
//! through.
//!
//! The test relies on the bundled collector binary, the same way
//! `tests/collector_integration.rs` does — and skips with a printed
//! notice when it cannot be located. CI's `sidecar-mac` lane builds
//! the binary; the Ubuntu `lint` lane doesn't, and that's fine.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use trove_app::app_state::{Backend, OtlpProtocol};
use trove_app::collector::codegen::{render_with, RenderError, RenderedCollector};
use trove_app::collector::{CollectorState, Supervisor, SupervisorHandle, SupervisorOptions};
use trove_app::ipc::test_export::{test_export_at, TestExportStatus};
use zeroize::Zeroizing;

/// 30-line stub HTTP server. Binds an ephemeral port, accepts one
/// connection at a time, reads until `\r\n\r\n`, parses
/// `Content-Length`, drains the body, returns a canned 200 OK with an
/// empty OTLP `partialSuccess` envelope.
struct StubReceiver {
    addr: std::net::SocketAddr,
    received_count: Arc<AtomicUsize>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl StubReceiver {
    async fn start() -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let received_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&received_count);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let join = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        let Ok((stream, _)) = accept else { continue; };
                        let counter = Arc::clone(&counter);
                        tokio::spawn(async move {
                            if handle_one(stream).await.is_ok() {
                                counter.fetch_add(1, Ordering::SeqCst);
                            }
                        });
                    }
                    _ = &mut shutdown_rx => return,
                }
            }
        });

        Ok(Self {
            addr,
            received_count,
            shutdown_tx: Some(shutdown_tx),
            join: Some(join),
        })
    }

    fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn count(&self) -> usize {
        self.received_count.load(Ordering::SeqCst)
    }

    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(j) = self.join.take() {
            let _ = j.await;
        }
    }
}

async fn handle_one(mut stream: TcpStream) -> io::Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0usize;
    let mut content_length = 0usize;
    let mut header_end = 0usize;
    loop {
        let n = stream.read(&mut buf[total..]).await?;
        if n == 0 {
            break;
        }
        total += n;
        if let Some(pos) = find_header_end(&buf[..total]) {
            header_end = pos;
            let header_text = std::str::from_utf8(&buf[..header_end])
                .map_err(|_| io::Error::other("non-utf8 headers"))?;
            content_length = header_text
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                })
                .unwrap_or(0);
            break;
        }
        if total >= buf.len() {
            buf.resize(buf.len() * 2, 0);
        }
    }

    // Drain the body if we haven't already read it all alongside the
    // headers.
    let body_so_far = total.saturating_sub(header_end);
    if content_length > body_so_far {
        let mut remaining = content_length - body_so_far;
        let mut sink = vec![0u8; 8 * 1024];
        while remaining > 0 {
            let take = remaining.min(sink.len());
            let n = stream.read(&mut sink[..take]).await?;
            if n == 0 {
                break;
            }
            remaining -= n;
        }
    }

    let body = b"{\"partialSuccess\":{}}";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
}

// ---- Supervisor setup helpers --------------------------------------------

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
    let out = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .ok()?
        .stdout;
    let s = String::from_utf8(out).ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("host: ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

#[allow(clippy::unnecessary_wraps)]
fn empty_resolver(_account: &str) -> Result<Zeroizing<String>, RenderError> {
    Ok(Zeroizing::new(String::new()))
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
            _ => return None,
        }
    }
}

fn otlp_generic_at(endpoint: &str) -> Backend {
    Backend::OtlpGeneric {
        endpoint: endpoint.to_string(),
        protocol: OtlpProtocol::Http,
        headers: BTreeMap::new(),
    }
}

fn render(backend: &Backend) -> RenderedCollector {
    render_with(backend, &empty_resolver).expect("render with empty resolver")
}

fn write_yaml_and_env(
    yaml: &str,
    env: std::collections::HashMap<String, Zeroizing<String>>,
) -> (
    PathBuf,
    PathBuf,
    std::collections::HashMap<String, String>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let yaml_path = dir.path().join("collector.yaml");
    // Tighten the batch processor for the test loop. The default 5s
    // timeout races test_export's 5s budget — by the time the batch
    // releases, our budget has elapsed. 200ms makes the batch flush
    // fast enough that both success-path and failure-path logs land
    // inside the test budget.
    let tightened = yaml.replace("timeout: 5s", "timeout: 200ms");
    std::fs::write(&yaml_path, tightened).expect("write yaml");
    let log_path = dir.path().join("collector.log");
    let plain: std::collections::HashMap<String, String> = env
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect();
    (yaml_path, log_path, plain, dir)
}

// ---- Tests ----------------------------------------------------------------

/// Both tests below spawn a real `trove-otelcol` child that binds the
/// fixed loopback ports 4317/4318/13133. Tokio runs `#[tokio::test]`
/// entries in parallel, so without serialisation the second test's
/// child finds the first one already holding those ports. A static
/// mutex held for the lifetime of each test enforces serial access
/// and avoids pulling in the `serial_test` crate.
fn collector_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_export_succeeds_against_a_stub_otlp_receiver() {
    let _guard = collector_lock().lock().expect("collector_lock poisoned");
    let Some(binary) = locate_binary() else {
        eprintln!(
            "[wizard_test_export] skipping: no trove-otelcol binary found. \
             Set TROVE_TEST_COLLECTOR_BIN, or run `pnpm build:collector \
             && pnpm bundle:sidecar` from the repo root."
        );
        return;
    };

    let stub = StubReceiver::start().await.expect("start stub");
    let backend = otlp_generic_at(&stub.endpoint());

    let rendered = render(&backend);
    let (yaml_path, log_path, env, _dir) = write_yaml_and_env(&rendered.yaml, rendered.env);

    let opts = SupervisorOptions::new(binary, yaml_path, log_path.clone()).with_env(env);
    let handle = Supervisor::start(opts).expect("supervisor starts");
    wait_for_running(&handle, Duration::from_secs(15))
        .await
        .expect("collector running within 15s");

    // Send the synthetic export through the local collector.
    let result = test_export_at(
        "http://127.0.0.1:4318/v1/traces",
        &log_path,
        Duration::from_secs(5),
    )
    .await;

    handle.shutdown().await;

    assert!(
        matches!(result.status, TestExportStatus::Ok),
        "expected Ok, got {result:?}",
    );
    assert!(
        stub.count() >= 1,
        "stub OTLP receiver should have seen at least one POST",
    );
    stub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_export_reports_failure_when_backend_unreachable() {
    let _guard = collector_lock().lock().expect("collector_lock poisoned");
    let Some(binary) = locate_binary() else {
        eprintln!("[wizard_test_export] skipping: no trove-otelcol binary found.");
        return;
    };

    // Bind+drop a port to grab a known-unbound number for the
    // unreachable case. There's a tiny race between our drop and the
    // collector trying to dial; in practice the kernel marks it
    // CLOSE_WAIT'd long enough that connections refuse immediately.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unreachable = listener.local_addr().unwrap();
    drop(listener);

    let backend = otlp_generic_at(&format!("http://{unreachable}"));
    let rendered = render(&backend);
    let (yaml_path, log_path, env, _dir) = write_yaml_and_env(&rendered.yaml, rendered.env);

    let opts = SupervisorOptions::new(binary, yaml_path, log_path.clone()).with_env(env);
    let handle = Supervisor::start(opts).expect("supervisor starts");
    wait_for_running(&handle, Duration::from_secs(15))
        .await
        .expect("collector running");

    let result = test_export_at(
        "http://127.0.0.1:4318/v1/traces",
        &log_path,
        Duration::from_secs(5),
    )
    .await;

    handle.shutdown().await;

    assert!(
        matches!(result.status, TestExportStatus::Failed | TestExportStatus::Timeout),
        "expected Failed or Timeout for unreachable backend, got {result:?}",
    );
}
