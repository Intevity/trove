//! Lifecycle, health, and logging for the bundled `trove-otelcol` sidecar.
//!
//! The Collector is the OTLP gateway every detected harness writes to.
//! It runs as a child process supervised by [`Supervisor`], started during
//! Tauri's `setup` phase and shut down on `RunEvent::ExitRequested`. The
//! supervisor restarts the process on unexpected exit (with exponential
//! backoff capped at 5s) and exposes its current state via a
//! [`tokio::sync::watch`] channel that Sprint 6 will surface to the UI.

pub mod codegen;
pub mod health;
pub mod lifecycle;
pub mod logs;

// Re-export the supervisor surface. `CollectorState` is unused inside the
// lib in Sprint 1 — it's surfaced to the integration test and to Sprint 6's
// dashboard via `SupervisorHandle::state` / `subscribe`.
#[allow(unused_imports)]
pub use lifecycle::{
    CollectorState, StartError, Supervisor, SupervisorHandle, SupervisorOptions,
};

/// Tauri-managed state slot for the live supervisor. Sprint 5 PR 2
/// wraps the handle so `save_backend` / `clear_backend` can swap it
/// during a collector reload: take → await `shutdown` outside the
/// lock → start a fresh supervisor → put it back.
pub type SupervisorState = std::sync::Mutex<Option<SupervisorHandle>>;
