//! Generic file-tail worker for Tier 3 adapters.
//!
//! Sprint 9 PR 1. The Cline / Aider / Copilot CLI adapters each watch a
//! best-effort log path and turn newly-appended lines into OTLP signals.
//! This module owns the polling loop so each adapter only writes a parser.
//!
//! Semantics — `tail -F`-equivalent:
//! - Open the path lazily; if it doesn't exist, sleep and retry (the
//!   wrapper scripts may not have produced a log yet).
//! - Read new bytes since the last offset, emit one record per newline.
//! - On EOF, sleep `poll_interval` and try again.
//! - Detect rotation via `metadata().len() < last_offset` (file was
//!   truncated or replaced via copy-then-truncate) and re-open.
//! - Detect explicit re-creation (path disappeared then reappeared) by
//!   reopening when the path is missing on a poll cycle.
//!
//! Errors are logged at `tracing::warn!` and the watcher continues —
//! one parse failure must not silence the stream.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader, SeekFrom};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// How often a watcher polls a file for new bytes when it's at EOF or
/// the file doesn't exist yet. 250ms balances responsiveness against
/// idle CPU; consistent with `tail -F` defaults.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Public handle a Tier 3 adapter holds onto. Aborting drops the
/// underlying tokio task; the file handle closes on drop.
#[derive(Debug)]
pub struct WatcherHandle {
    join: JoinHandle<()>,
}

impl WatcherHandle {
    /// Wrap an externally-spawned tokio task in a `WatcherHandle` so it
    /// can be stored alongside the line-tail handles in
    /// `tier3_watchers`. Used by adapter-specific watchers (e.g. the
    /// Cline tasks-dir poller) that don't fit the generic line-tail
    /// shape but still need the same lifetime.
    #[must_use]
    pub fn from_join(join: JoinHandle<()>) -> Self {
        Self { join }
    }

    /// Cancel the watcher. Idempotent.
    pub fn abort(&self) {
        self.join.abort();
    }

    /// Whether the underlying task has finished. Useful for tests.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.join.is_finished()
    }

    /// Test-only: await the underlying task. Real callers `abort()`.
    #[cfg(test)]
    pub async fn join(self) {
        let _ = self.join.await;
    }
}

/// Spawn a tokio task that tails `path` and forwards each line as a
/// `String` (no trailing newline) to `tx`. Closing the receiver halts
/// the watcher on its next send. Returns immediately; the actual file
/// open is deferred to the task so a missing file at startup is not
/// an error.
#[must_use]
pub fn spawn(
    path: impl Into<PathBuf>,
    tx: mpsc::Sender<String>,
    poll_interval: Duration,
) -> WatcherHandle {
    let path = path.into();
    let join = tokio::spawn(async move {
        run(path, tx, poll_interval).await;
    });
    WatcherHandle { join }
}

async fn run(path: PathBuf, tx: mpsc::Sender<String>, poll_interval: Duration) {
    loop {
        let opened = File::open(&path).await.ok();
        if let Some(file) = opened {
            // Tail this file until it's truncated, deleted, or replaced.
            // `tail_file` returns when the watcher should re-open by path.
            if !tail_file(&path, file, &tx, poll_interval).await {
                // Receiver closed — exit cleanly.
                return;
            }
        } else if tx.is_closed() {
            return;
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Inner loop that tails an already-opened file. Returns `true` if the
/// caller should retry (file was rotated/truncated/disappeared) and
/// `false` if the watcher's receiver closed (caller should exit).
async fn tail_file(
    path: &Path,
    file: File,
    tx: &mpsc::Sender<String>,
    poll_interval: Duration,
) -> bool {
    // Replay any pre-existing content first: at startup, we want to
    // see lines already in the file, not skip ahead. Tier 3 adapters
    // call `apply_patch` immediately before `spawn`, so the rc-shell
    // wrapper or globalStorage tree is fresh; pre-existing bytes
    // are typically zero.
    let mut reader = BufReader::new(file);
    let mut offset: u64 = 0;

    loop {
        // Drain available lines.
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF for now
                Ok(n) => {
                    offset += n as u64;
                    let trimmed = line.trim_end_matches(['\n', '\r']);
                    if trimmed.is_empty() {
                        continue;
                    }
                    if tx.send(trimmed.to_string()).await.is_err() {
                        return false;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, ?path, "log_watcher read error; reopening file");
                    return true;
                }
            }
        }

        if tx.is_closed() {
            return false;
        }

        // EOF: poll metadata to detect rotation/truncation.
        match tokio::fs::metadata(path).await {
            Ok(meta) => {
                let len = meta.len();
                if len < offset {
                    // Truncated or replaced (copy-truncate); reopen.
                    return true;
                }
                // Otherwise the file is unchanged or grew; the next
                // read_line will pick up the new bytes.
            }
            Err(_) => {
                // Path removed; reopen on next loop after sleep.
                return true;
            }
        }

        tokio::time::sleep(poll_interval).await;

        // Re-seek to current offset to pick up appended bytes. BufReader
        // caches AsyncSeek behind it.
        if reader.seek(SeekFrom::Start(offset)).await.is_err() {
            return true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;

    /// Append `line` plus a newline to `path` using a synchronous open
    /// to ensure the bytes hit disk immediately (the watcher polls
    /// metadata at 50ms).
    fn append_sync(path: &Path, line: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("append open");
        writeln!(f, "{line}").unwrap();
        f.flush().unwrap();
    }

    /// Same idea, but truncates the file first.
    fn truncate_then_write(path: &Path, content: &str) {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)
            .expect("truncate open");
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
    }

    #[tokio::test]
    async fn watcher_emits_each_appended_line_in_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.log");
        // Pre-create empty file so the open path exercises both
        // "file exists at startup" and "no pre-existing content".
        std::fs::File::create(&path).unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let handle = spawn(path.clone(), tx, Duration::from_millis(50));

        // Give the watcher a moment to open + reach EOF.
        tokio::time::sleep(Duration::from_millis(80)).await;

        for i in 0..5 {
            append_sync(&path, &format!("line-{i}"));
        }

        let mut received = Vec::new();
        for _ in 0..5 {
            let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("receive timeout")
                .expect("channel closed");
            received.push(line);
        }
        assert_eq!(
            received,
            vec!["line-0", "line-1", "line-2", "line-3", "line-4"]
        );

        handle.abort();
        handle.join().await;
    }

    #[tokio::test]
    async fn watcher_recovers_from_truncation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("b.log");
        std::fs::File::create(&path).unwrap();
        append_sync(&path, "before-rotate-1");
        append_sync(&path, "before-rotate-2");

        let (tx, mut rx) = mpsc::channel(16);
        let handle = spawn(path.clone(), tx, Duration::from_millis(50));

        // Receive the two pre-existing lines.
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap()
                .unwrap();
        }

        // Truncate the file (simulates `logrotate copytruncate` or
        // a wrapper-script that overwrote the log).
        truncate_then_write(&path, "");
        // Wait long enough for the watcher to detect len < offset and
        // re-open.
        tokio::time::sleep(Duration::from_millis(250)).await;

        append_sync(&path, "after-rotate");
        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("receive timeout")
            .expect("channel closed");
        assert_eq!(line, "after-rotate");

        handle.abort();
        handle.join().await;
    }

    #[tokio::test]
    async fn watcher_waits_for_path_to_appear() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.log");
        // Path does NOT exist at spawn time.

        let (tx, mut rx) = mpsc::channel(16);
        let handle = spawn(path.clone(), tx, Duration::from_millis(50));

        // No errors logged; watcher idles waiting for path.
        tokio::time::sleep(Duration::from_millis(150)).await;
        append_sync(&path, "after-path-exists");

        let line = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("receive timeout")
            .expect("channel closed");
        assert_eq!(line, "after-path-exists");

        handle.abort();
        handle.join().await;
    }

    #[tokio::test]
    async fn closing_receiver_halts_watcher() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("d.log");
        std::fs::File::create(&path).unwrap();

        let (tx, rx) = mpsc::channel(1);
        let handle = spawn(path.clone(), tx, Duration::from_millis(50));

        // Drop receiver; watcher should observe the closed channel
        // and exit on the next send (or, for path-missing branches,
        // on the next loop tick).
        drop(rx);
        // Trigger at least one iteration of the inner loop by
        // appending — closing the receiver should cause the next send
        // to fail and the task to return.
        append_sync(&path, "after-drop");
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(handle.is_finished(), "watcher should exit when rx drops");
        // Don't await — task already finished.
    }

    #[tokio::test]
    async fn watcher_skips_blank_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("e.log");
        std::fs::File::create(&path).unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let handle = spawn(path.clone(), tx, Duration::from_millis(50));

        // Append a real line, then two blank lines, then another real one.
        append_sync(&path, "real-1");
        append_sync(&path, "");
        append_sync(&path, "");
        append_sync(&path, "real-2");

        let line1 = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let line2 = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(line1, "real-1");
        assert_eq!(line2, "real-2");

        handle.abort();
        handle.join().await;
    }

    /// Exercise the `AsyncWriteExt` import path so it doesn't show as
    /// unused on machines where the test mod compiles but the rotation
    /// test is filtered out.
    #[tokio::test]
    async fn async_write_ext_is_imported() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("f.log");
        let mut f = tokio::fs::File::create(&path).await.unwrap();
        f.write_all(b"x").await.unwrap();
    }
}
