//! Three-way conflict detection for managed regions.
//!
//! Adapters call [`detect`] before re-applying a patch to figure out
//! whether the user has edited the host config since Trove last touched
//! it, and if so whether their edit landed inside the managed region or
//! outside it. The four outcomes drive different UI:
//!
//! - [`ConflictState::Clean`] — nothing has changed. Re-apply is a no-op.
//! - [`ConflictState::UserEditedOutside`] — user edited the file but our
//!   region is intact. Safe to re-apply our region; their edits survive.
//! - [`ConflictState::RegionRemoved`] — our region was deleted entirely.
//!   Re-apply re-installs it; nothing to merge.
//! - [`ConflictState::RegionConflict`] — user edited inside our region.
//!   Surface a three-way merge dialog (Sprint 8). Don't silently
//!   overwrite.
//!
//! Detection is hash-based. After every successful upsert, the caller
//! persists a [`StoredPatchMetadata`] capturing two sha256 digests: the
//! managed block's hash, and the entire host file's hash. On the next
//! detect, we re-extract the managed region, recompute both, and route
//! to the right state via simple equality checks.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::safety::sentinels::{Format, ManagedRegion, SentinelError, extract_region};

/// What the caller persists after each successful upsert. Saved into
/// `HarnessConfig.trovePatch` (see `packages/shared/src/schemas.ts`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredPatchMetadata {
    /// sha256 hex of the canonical payload (matches `ManagedRegion::hash`
    /// at the time of the last write).
    pub managed_block_hash: String,
    /// sha256 hex of the entire host file at the time of the last write.
    pub file_hash_at_last_write: String,
}

impl StoredPatchMetadata {
    /// Compute metadata to persist immediately after a successful upsert.
    /// `region_at_write` should be the same `ManagedRegion` passed to
    /// [`crate::safety::sentinels::upsert_region`]; `file_after_write`
    /// is the textual content the caller just persisted with
    /// [`crate::safety::atomic::write_atomic`].
    #[must_use]
    pub fn capture(region_at_write: &ManagedRegion, file_after_write: &str) -> Self {
        Self {
            managed_block_hash: region_at_write.hash.clone(),
            file_hash_at_last_write: hash_hex(file_after_write.as_bytes()),
        }
    }
}

/// Outcome of a three-way conflict check.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictState {
    /// Neither the file nor our region has changed since the last write.
    Clean,
    /// The file was edited outside our region; the region is intact and
    /// matches the stored hash. Safe to re-apply.
    UserEditedOutside,
    /// Our region is no longer present in the file. The user (or an
    /// adapter elsewhere) deleted it. Re-applying restores it.
    RegionRemoved,
    /// Our region exists but its hash differs from the stored hash —
    /// the user edited inside the region. Surface a merge UI.
    RegionConflict,
}

/// Errors produced when reading the host file for detection.
#[derive(Debug, thiserror::Error)]
pub enum ConflictError {
    #[error(transparent)]
    Sentinel(#[from] SentinelError),
}

/// Detect the conflict state for `current_file` given the metadata
/// recorded after the last successful upsert.
pub fn detect(
    stored: &StoredPatchMetadata,
    current_file: &str,
    format: Format,
) -> Result<ConflictState, ConflictError> {
    let current_file_hash = hash_hex(current_file.as_bytes());

    let extracted = extract_region(format, current_file)?;
    let Some(region) = extracted else {
        return Ok(ConflictState::RegionRemoved);
    };

    let region_hash_matches = region.hash == stored.managed_block_hash;
    let file_hash_matches = current_file_hash == stored.file_hash_at_last_write;

    Ok(match (region_hash_matches, file_hash_matches) {
        (true, true) => ConflictState::Clean,
        (true, false) => ConflictState::UserEditedOutside,
        (false, _) => ConflictState::RegionConflict,
    })
}

fn hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::sentinels::{Format, ManagedRegion, upsert_region};
    use pretty_assertions::assert_eq;

    fn build_region() -> ManagedRegion {
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(r#"{"env":{"OTEL_FOO":"bar"}}"#).unwrap();
        ManagedRegion::for_json_patches(&map).unwrap()
    }

    fn apply_and_capture(
        format: Format,
        content: &str,
        region: &ManagedRegion,
    ) -> (String, StoredPatchMetadata) {
        let after = upsert_region(format, content, region, "test-adapter").unwrap();
        let meta = StoredPatchMetadata::capture(region, &after);
        (after, meta)
    }

    #[test]
    fn clean_when_nothing_changed() {
        let region = build_region();
        let (file, meta) = apply_and_capture(Format::Json, "{}", &region);
        assert_eq!(detect(&meta, &file, Format::Json).unwrap(), ConflictState::Clean);
    }

    #[test]
    fn region_removed_when_user_deletes_block() {
        let region = build_region();
        let (file, meta) = apply_and_capture(Format::Json, "{}", &region);
        // Simulate the user deleting our region.
        let stripped =
            crate::safety::sentinels::remove_region(Format::Json, &file, "test-adapter").unwrap();
        assert_eq!(
            detect(&meta, &stripped, Format::Json).unwrap(),
            ConflictState::RegionRemoved
        );
    }

    #[test]
    fn user_edited_outside_when_only_unrelated_keys_changed() {
        // Use TOML so editing outside our region produces a parseable
        // file with no impact on our managed text.
        let region = ManagedRegion::for_text_block(
            "endpoint = \"http://127.0.0.1:4318\"\n",
            vec!["endpoint".into()],
        );
        let (file, meta) = apply_and_capture(Format::Toml, "[user]\nname = \"a\"\n", &region);

        // The user appends an unrelated table outside the trove block.
        let mut edited = file.clone();
        edited.push_str("\n[other]\nx = 1\n");

        assert_eq!(
            detect(&meta, &edited, Format::Toml).unwrap(),
            ConflictState::UserEditedOutside
        );
    }

    #[test]
    fn region_conflict_when_user_edits_inside_block() {
        let region = ManagedRegion::for_text_block(
            "endpoint = \"http://127.0.0.1:4318\"\n",
            vec!["endpoint".into()],
        );
        let (file, meta) = apply_and_capture(Format::Toml, "", &region);

        // Tamper inside the managed block — change the value the user
        // sees.
        let edited = file.replace(
            "endpoint = \"http://127.0.0.1:4318\"",
            "endpoint = \"http://evil.example.com\"",
        );
        assert_ne!(edited, file);

        assert_eq!(
            detect(&meta, &edited, Format::Toml).unwrap(),
            ConflictState::RegionConflict
        );
    }

    #[test]
    fn capture_round_trips_through_serde() {
        let region = build_region();
        let (file, meta) = apply_and_capture(Format::Json, "{}", &region);
        let json = serde_json::to_string(&meta).unwrap();
        let revived: StoredPatchMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, revived);
        assert_eq!(detect(&revived, &file, Format::Json).unwrap(), ConflictState::Clean);
    }
}
