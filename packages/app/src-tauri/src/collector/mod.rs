//! Lifecycle, health, and logging for the bundled `trove-otelcol` sidecar.
//!
//! The Collector is the OTLP gateway every detected harness writes to.
//! It runs as a child process supervised by [`Supervisor`], started during
//! Tauri's `setup` phase and shut down on `RunEvent::ExitRequested`. The
//! supervisor restarts the process on unexpected exit (with exponential
//! backoff capped at 5s) and exposes its current state via a
//! [`tokio::sync::watch`] channel that Sprint 6 will surface to the UI.

pub mod codegen;
pub mod derive;
pub mod health;
pub mod health_tracker;
pub mod lifecycle;
pub mod logs;
pub mod metrics_tap;

// Re-export the supervisor surface.
#[allow(unused_imports)]
pub use lifecycle::{
    CollectorLogLine, CollectorState, StartError, Supervisor, SupervisorChannels,
    SupervisorHandle, SupervisorOptions,
};
#[allow(unused_imports)]
pub use derive::{
    BackendHealth, BackendHealthSamples, BackendHealthStatus, OverallHealth,
    derive_backend_health, derive_overall_health,
};
#[allow(unused_imports)]
pub use health_tracker::{BackendHealthHandle, BackendHealthTracker, BackendsFetcher};
#[allow(unused_imports)]
pub use metrics_tap::{
    ExporterCounts, MetricsSnapshot, MetricsTap, MetricsTapHandle, MetricsTapOptions, SignalCounts,
};
pub use codegen::harness_id_suffix;

/// Tauri-managed state slot for the live supervisor. Sprint 5 PR 2
/// wraps the handle so `save_backend` / `clear_backend` can swap it
/// during a collector reload: take → await `shutdown` outside the
/// lock → start a fresh supervisor → put it back. The watch sender
/// inside [`SupervisorChannels`] is held separately (registered as
/// its own Tauri-managed slot) so subscribers survive the swap.
pub type SupervisorState = std::sync::Mutex<Option<SupervisorHandle>>;
