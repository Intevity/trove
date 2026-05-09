//! Registry of running Tier 3 watchers, keyed by harness id.
//!
//! Sprint 9 PR 1 lays the slot in place; PR 2 (Cline) and PR 3
//! (Aider, Copilot CLI) wire `apply_patch` / `revert_patch` to call
//! [`TierThreeWatchers::insert`] and [`TierThreeWatchers::abort`].
//!
//! Until those PRs land the registry is unused but managed by Tauri so
//! the IPC commands have a stable lookup target. PR 1 also walks
//! `state.json` at app boot to honour the "watchers spawn at app start
//! for any enabled Tier 3 harness" lifecycle promise — currently a
//! no-op because no Tier 3 adapter is registered yet.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::harness::HarnessId;
use crate::log_watcher::WatcherHandle;

/// Tauri-managed registry holding the running watcher handles. A `Mutex`
/// rather than a `tokio::sync::Mutex` because the IPC commands live on
/// the synchronous Tauri command thread and never await while holding
/// the lock.
#[derive(Default)]
pub struct TierThreeWatchers {
    inner: Mutex<HashMap<HarnessId, WatcherHandle>>,
}

impl TierThreeWatchers {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a freshly-spawned watcher for `id`. Aborts any prior
    /// watcher for the same id (defensive — the IPC layer should call
    /// [`Self::abort`] on revert before starting a new one, but a
    /// double-apply must not leak a task).
    pub fn insert(&self, id: HarnessId, handle: WatcherHandle) {
        let mut guard = self.inner.lock().expect("tier3_watchers poisoned");
        if let Some(prev) = guard.insert(id, handle) {
            prev.abort();
        }
    }

    /// Cancel the watcher registered for `id`, if any. Returns `true`
    /// if a handle was found and aborted.
    pub fn abort(&self, id: HarnessId) -> bool {
        let mut guard = self.inner.lock().expect("tier3_watchers poisoned");
        match guard.remove(&id) {
            Some(handle) => {
                handle.abort();
                true
            }
            None => false,
        }
    }

    /// Whether a watcher is currently registered for `id`. Note: a
    /// finished handle may still be registered until the next operation
    /// trims it; callers should treat `true` as "started, may have
    /// since exited" rather than "currently running".
    #[must_use]
    pub fn contains(&self, id: HarnessId) -> bool {
        self.inner
            .lock()
            .expect("tier3_watchers poisoned")
            .contains_key(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::log_watcher::{spawn, DEFAULT_POLL_INTERVAL};
    use tempfile::tempdir;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn insert_then_abort_removes_handle() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ph.log");
        std::fs::File::create(&path).unwrap();

        let (tx, _rx) = mpsc::channel(1);
        let handle = spawn(path, tx, DEFAULT_POLL_INTERVAL);

        let registry = TierThreeWatchers::new();
        registry.insert(HarnessId::Cline, handle);
        assert!(registry.contains(HarnessId::Cline));
        assert!(registry.abort(HarnessId::Cline));
        assert!(!registry.contains(HarnessId::Cline));
        assert!(!registry.abort(HarnessId::Cline)); // idempotent
    }

    #[tokio::test]
    async fn double_insert_aborts_previous() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ph.log");
        std::fs::File::create(&path).unwrap();

        let (tx1, _rx1) = mpsc::channel(1);
        let h1 = spawn(path.clone(), tx1, DEFAULT_POLL_INTERVAL);
        let (tx2, _rx2) = mpsc::channel(1);
        let h2 = spawn(path, tx2, DEFAULT_POLL_INTERVAL);

        let registry = TierThreeWatchers::new();
        registry.insert(HarnessId::Aider, h1);
        registry.insert(HarnessId::Aider, h2); // replaces h1; h1 aborts
        assert!(registry.contains(HarnessId::Aider));
        assert!(registry.abort(HarnessId::Aider));
    }

    #[test]
    fn contains_false_for_unregistered_id() {
        let registry = TierThreeWatchers::new();
        assert!(!registry.contains(HarnessId::CopilotCli));
    }
}
