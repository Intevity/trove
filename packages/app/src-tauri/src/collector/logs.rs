//! Tee Collector child stdout/stderr to the parent's `tracing` logger and
//! to an append-mode log file under the platform-specific state dir.
//!
//! The file is size-capped: when it crosses [`MAX_LOG_BYTES`] it's rotated
//! once to `<name>.1`, replacing any previous rotation. This is
//! deliberately simpler than a full rolling-log library — Sprint 11's
//! diagnostics tab can replace it if we need richer rotation policies.

use std::path::{Path, PathBuf};

use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};

/// Roll the log file when it exceeds 10 MiB. Tuned to keep ~one boot
/// cycle of detailed output without risking unbounded disk usage.
pub const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Read lines from `reader` (typically a `ChildStdout` / `ChildStderr`)
/// and append them to `log_path`, also re-emitting them via `tracing` at
/// the supplied level. Exits when the reader returns EOF or errors.
pub async fn tee_stream<R>(
    mut reader: BufReader<R>,
    log_path: PathBuf,
    level: tracing::Level,
    stream_label: &'static str,
) where
    R: AsyncRead + Unpin,
{
    if let Err(e) = ensure_parent_dir(&log_path).await {
        tracing::warn!(?e, ?log_path, "could not create collector log directory");
        return;
    }

    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf).await {
            Ok(0) => return, // EOF
            Ok(_) => {
                let line = buf.trim_end_matches('\n').trim_end_matches('\r');
                emit_traced(level, stream_label, line);
                if let Err(e) = append_with_rotation(&log_path, line).await {
                    tracing::warn!(?e, ?log_path, "could not append to collector log");
                }
            }
            Err(e) => {
                tracing::warn!(?e, "error reading collector stream; closing tee");
                return;
            }
        }
    }
}

fn emit_traced(level: tracing::Level, stream_label: &'static str, line: &str) {
    match level {
        tracing::Level::ERROR => tracing::error!(stream = stream_label, "{line}"),
        tracing::Level::WARN => tracing::warn!(stream = stream_label, "{line}"),
        tracing::Level::INFO => tracing::info!(stream = stream_label, "{line}"),
        tracing::Level::DEBUG => tracing::debug!(stream = stream_label, "{line}"),
        tracing::Level::TRACE => tracing::trace!(stream = stream_label, "{line}"),
    }
}

async fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}

async fn append_with_rotation(path: &Path, line: &str) -> std::io::Result<()> {
    rotate_if_needed(path).await?;
    let mut f: File = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    f.write_all(line.as_bytes()).await?;
    f.write_all(b"\n").await?;
    Ok(())
}

async fn rotate_if_needed(path: &Path) -> std::io::Result<()> {
    let meta = match fs::metadata(path).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.len() < MAX_LOG_BYTES {
        return Ok(());
    }
    let rotated = rotated_path(path);
    if rotated.exists() {
        fs::remove_file(&rotated).await?;
    }
    fs::rename(path, &rotated).await?;
    Ok(())
}

fn rotated_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".1");
    PathBuf::from(s)
}
