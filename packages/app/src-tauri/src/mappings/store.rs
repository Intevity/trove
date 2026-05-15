//! Live shared mapping state for watchers and codegen.
//!
//! The mapping state is persisted on disk (`state.json`) and updated via
//! the `apply_mappings` IPC command. Watchers running in-process need to
//! see those updates *without* being restarted — otherwise a user edit
//! would silently fail to take effect until the next app launch.
//!
//! `tokio::sync::watch` is the right tool: a single Sender owned by the
//! Tauri app slot, cheaply cloneable Receivers passed into each watcher
//! at spawn time. Watchers call `.borrow()` at emit time to grab a
//! short-lived view of the current state. The receiver also notifies
//! on change, which we don't use today (watchers re-read on every poll)
//! but is available for future zero-latency reconfigs.
//!
//! The receiver alone is enough for read-only consumers. The [`MappingStateStore`]
//! wraps the Sender so the IPC layer can push updates.

use std::sync::Arc;

use tokio::sync::watch;

use super::MappingState;

/// A receiver handle watchers hold. Cheap to clone. Use [`Self::current`]
/// at emit time; the returned [`Arc`] keeps the snapshot alive for the
/// duration of one OTLP build without blocking the sender.
#[derive(Clone, Debug)]
pub struct SharedMappingState {
    rx: watch::Receiver<Arc<MappingState>>,
}

impl SharedMappingState {
    /// Latest snapshot. Cheap (the inner `watch` channel just clones
    /// the current `Arc`). Returns the snapshot inside an `Arc` so the
    /// caller can hold the view without blocking the writer side.
    #[must_use]
    pub fn current(&self) -> Arc<MappingState> {
        self.rx.borrow().clone()
    }

    /// Block until the watched state changes (used by tests and any
    /// future hot-reload codegen). Returns the new snapshot.
    /// Errors only if the sender has been dropped — i.e. the app is
    /// shutting down — in which case the caller should also exit.
    ///
    /// Note: this is `async` and awaits internally; safe to call from
    /// any tokio context.
    pub async fn changed(&mut self) -> Result<Arc<MappingState>, watch::error::RecvError> {
        self.rx.changed().await?;
        Ok(self.rx.borrow().clone())
    }
}

/// The writer side of the store, plus a constructor for receivers.
/// Managed by Tauri as a slot; the `apply_mappings` IPC pushes new
/// values via [`Self::publish`], and watcher spawn paths grab a
/// [`SharedMappingState`] via [`Self::subscribe`].
#[derive(Clone, Debug)]
pub struct MappingStateStore {
    tx: Arc<watch::Sender<Arc<MappingState>>>,
}

impl MappingStateStore {
    /// Build a store seeded with `initial`. Typically the loaded state
    /// from `state.json`. Cheap.
    #[must_use]
    pub fn new(initial: MappingState) -> Self {
        let (tx, _rx) = watch::channel(Arc::new(initial));
        Self { tx: Arc::new(tx) }
    }

    /// Push a new state to every subscribed receiver. Replaces the
    /// previous snapshot wholesale. Idempotent: subscribers that
    /// haven't `.changed()` yet still see the latest value via
    /// `.current()`.
    pub fn publish(&self, next: MappingState) {
        // send_replace is preferred over send: it doesn't error when
        // there are no subscribers (the watcher set may be empty at
        // boot or during shutdown).
        self.tx.send_replace(Arc::new(next));
    }

    /// Hand out a fresh receiver. The store's invariant is that the
    /// channel always carries the current state, so subscribers see
    /// today's value on their first `.current()` call.
    #[must_use]
    pub fn subscribe(&self) -> SharedMappingState {
        SharedMappingState {
            rx: self.tx.subscribe(),
        }
    }

    /// Convenience: peek the current snapshot without subscribing.
    /// Used by non-watcher code paths (codegen at apply time, etc.).
    #[must_use]
    pub fn current(&self) -> Arc<MappingState> {
        self.tx.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::default_state;

    #[tokio::test]
    async fn subscribers_see_initial_value() {
        let store = MappingStateStore::new(default_state());
        let sub = store.subscribe();
        let snap = sub.current();
        assert_eq!(snap.schema_version, super::super::MAPPING_SCHEMA_VERSION);
        assert!(!snap.harnesses.is_empty());
    }

    #[tokio::test]
    async fn publish_propagates_to_subscribers() {
        let store = MappingStateStore::new(default_state());
        let mut sub = store.subscribe();

        // Push a modified state.
        let mut next = default_state();
        next.harnesses.clear();
        store.publish(next);

        // Receiver sees the new state on the next .current().
        let snap = sub.current();
        assert!(snap.harnesses.is_empty());

        // changed() returns immediately if a publish happened.
        // (We need to publish again after the first borrow to trigger
        // `changed`, since `current` doesn't consume the change marker.)
        let mut next2 = default_state();
        next2.harnesses.clear();
        // mark a distinguishable field
        next2.metrics.clear();
        store.publish(next2);
        let snap2 = sub.changed().await.unwrap();
        assert!(snap2.metrics.is_empty());
    }

    #[tokio::test]
    async fn publish_with_no_subscribers_is_a_noop() {
        let store = MappingStateStore::new(default_state());
        let mut next = default_state();
        next.harnesses.clear();
        store.publish(next); // no panic, no error
    }
}
