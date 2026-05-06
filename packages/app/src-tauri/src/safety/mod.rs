//! Filesystem safety toolkit shared by every harness adapter.
//!
//! Sprint 2 ships three primitives, in three PRs:
//!
//! - `atomic` — temp-file + fsync + rename writes that preserve mode and
//!   cannot leave a partial destination on crash. **Landed PR 1.**
//! - `backup` — timestamped sidecar backups with retention pruning.
//!   **Landed PR 1.**
//! - `sentinels` — managed-region insert/replace/remove across JSON, JSONC,
//!   TOML and YAML. **Lands PR 2.**
//! - `conflict` — three-way detection so re-applying a patch never silently
//!   overwrites a hand-edited region. **Lands PR 3.**
//!
//! Every adapter (Sprint 3+) goes through this module. The acceptance bar
//! is byte-identical revert under property-test fuzzing — see
//! `documentation/MVP_PLAN.md` Sprint 2.

pub mod atomic;
pub mod backup;
pub mod conflict;
pub mod sentinels;
