//! Managed-region engine — insert, replace, and remove the block of
//! configuration Trove owns inside a host config file, across the four
//! formats adapters touch.
//!
//! Two implementation shapes are unified behind one [`Format`] enum:
//!
//! - **JSON / JSONC** use a top-level `_trove` object recording the
//!   keypaths Trove installed (`managed_keys`) and a `hash` of the
//!   canonical payload. The payload's leaf keypaths are written at their
//!   natural locations in the host document so the harness reads them
//!   normally — Trove never invents new schema. Revert reads
//!   `_trove.managed_keys`, deletes each path, then deletes `_trove`.
//!
//! - **TOML / YAML** use a comment-fenced block bracketed by
//!   `# trove:start` … `# trove:end`. Adapters provide the literal text
//!   to place between the fences; revert deletes the block. Comments
//!   are valid syntax in both formats, so this is robust against any
//!   value the user might write outside the block.
//!
//! Both shapes share the same [`ManagedRegion`] struct; the meaning of
//! the `payload` field shifts by format. Adapters in Sprint 3+ won't
//! see this distinction — they'll call format-specific helpers that
//! produce the right `ManagedRegion` for their host format.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One of the four config-file formats Trove patches.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Jsonc,
    Toml,
    Yaml,
}

impl Format {
    fn label(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Jsonc => "jsonc",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Sentinel start marker used in TOML and YAML comment-fenced blocks.
const FENCE_START: &str = "# trove:start";
/// Sentinel end marker.
const FENCE_END: &str = "# trove:end";
/// Top-level key used in JSON / JSONC documents to record what Trove owns.
const TROVE_KEY: &str = "_trove";

/// Description of the managed block Trove installs in a host config file.
///
/// `payload` and `managed_keys` are interpreted differently per format:
///
/// - For [`Format::Json`] / [`Format::Jsonc`], `payload` is a serialized
///   JSON object whose leaf paths are merged into the host document.
///   `managed_keys` lists those leaf paths in dotted form so revert can
///   remove them precisely.
/// - For [`Format::Toml`] / [`Format::Yaml`], `payload` is the literal
///   text block placed between the fence markers. `managed_keys` is
///   informational only (used by the dashboard).
///
/// `hash` is the hex-encoded SHA-256 of the canonical payload form
/// (sorted-keys for JSON; verbatim bytes for fenced text). It's recorded
/// alongside the block so [`crate::safety::conflict`] can detect whether
/// a user edited inside the managed region.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedRegion {
    pub managed_keys: Vec<String>,
    pub payload: String,
    pub hash: String,
}

impl ManagedRegion {
    /// Build a `ManagedRegion` for JSON / JSONC by recording a set of
    /// dotted-path → JSON value patches. `managed_keys` is derived from
    /// the leaf paths of `patches`.
    pub fn for_json_patches(
        patches: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, SentinelError> {
        let leaves = leaf_paths(patches, "");
        let canonical = canonical_json(&serde_json::Value::Object(patches.clone()))?;
        let payload = serde_json::to_string(&serde_json::Value::Object(patches.clone()))
            .map_err(|e| SentinelError::EmitFailed(e.to_string()))?;
        Ok(Self {
            managed_keys: leaves,
            payload,
            hash: hash_hex(&canonical),
        })
    }

    /// Build a `ManagedRegion` for TOML / YAML from a literal text block.
    /// `managed_keys` is supplied by the caller as informational labels
    /// for the dashboard; it does not affect insert/remove behaviour.
    #[must_use]
    pub fn for_text_block(payload: impl Into<String>, managed_keys: Vec<String>) -> Self {
        let payload = payload.into();
        Self {
            managed_keys,
            hash: hash_hex(payload.as_bytes()),
            payload,
        }
    }
}

/// Errors produced by the sentinel engine.
#[derive(Debug, thiserror::Error)]
pub enum SentinelError {
    #[error("malformed {format} document: {message}")]
    Malformed { format: Format, message: String },
    #[error("the managed region is malformed: {0}")]
    RegionMalformed(String),
    #[error("multiple managed regions found in the document")]
    MultipleRegions,
    #[error("could not emit document: {0}")]
    EmitFailed(String),
}

/// Insert or replace the managed region in `content`. Idempotent under
/// the same `region` input.
pub fn upsert_region(
    format: Format,
    content: &str,
    region: &ManagedRegion,
) -> Result<String, SentinelError> {
    match format {
        Format::Json => json_like::upsert(content, region, /*is_jsonc=*/ false),
        Format::Jsonc => json_like::upsert(content, region, /*is_jsonc=*/ true),
        Format::Toml => comment_fence::upsert_toml(content, region),
        Format::Yaml => comment_fence::upsert_yaml(content, region),
    }
}

/// Remove the managed region. No-op if it isn't present.
pub fn remove_region(format: Format, content: &str) -> Result<String, SentinelError> {
    match format {
        Format::Json => json_like::remove(content, /*is_jsonc=*/ false),
        Format::Jsonc => json_like::remove(content, /*is_jsonc=*/ true),
        Format::Toml => comment_fence::remove_toml(content),
        Format::Yaml => comment_fence::remove_yaml(content),
    }
}

/// Read the current managed region back out of `content`. Returns
/// `None` if no region is present.
pub fn extract_region(
    format: Format,
    content: &str,
) -> Result<Option<ManagedRegion>, SentinelError> {
    match format {
        Format::Json => json_like::extract(content, /*is_jsonc=*/ false),
        Format::Jsonc => json_like::extract(content, /*is_jsonc=*/ true),
        Format::Toml => comment_fence::extract_toml(content),
        Format::Yaml => comment_fence::extract_yaml(content),
    }
}

// ---------------------------------------------------------------------------
// JSON / JSONC
// ---------------------------------------------------------------------------

mod json_like {
    use super::{
        ManagedRegion, SentinelError, TROVE_KEY, canonical_json, format_for, hash_hex, leaf_paths,
        set_leaf, take_leaf,
    };

    use jsonc_parser::ParseOptions;
    use serde_json::{Map, Value};

    pub(super) fn upsert(
        content: &str,
        region: &ManagedRegion,
        is_jsonc: bool,
    ) -> Result<String, SentinelError> {
        let mut value = parse(content, is_jsonc)?;

        let patches: Value = serde_json::from_str(&region.payload)
            .map_err(|e| SentinelError::RegionMalformed(format!("payload not valid JSON: {e}")))?;
        let Value::Object(patches_map) = patches else {
            return Err(SentinelError::RegionMalformed(
                "payload must be a JSON object".into(),
            ));
        };

        // Strip any prior _trove (and its managed keys) so re-applies don't
        // leave stale entries when the patch set shrinks.
        if let Some(prior) = strip_trove(&mut value) {
            for key in prior.managed_keys {
                let _ = take_leaf(value_object_mut(&mut value)?, &key);
            }
        }

        // Install the new patches at their natural locations.
        let leaves = leaf_paths(&patches_map, "");
        let host = value_object_mut(&mut value)?;
        let leaf_values = collect_leaf_values(&Value::Object(patches_map.clone()), "");
        for (path, leaf_value) in leaf_values {
            set_leaf(host, &path, leaf_value);
        }

        // Write _trove. Using sorted keys inside the payload's canonical
        // form makes the hash stable regardless of input map order.
        let canonical = canonical_json(&Value::Object(patches_map))?;
        let trove_meta = trove_metadata(&leaves, &hash_hex(&canonical));
        host.insert(TROVE_KEY.into(), trove_meta);

        emit(&value, is_jsonc)
    }

    pub(super) fn remove(content: &str, is_jsonc: bool) -> Result<String, SentinelError> {
        let mut value = parse(content, is_jsonc)?;
        if let Some(prior) = strip_trove(&mut value) {
            let host = value_object_mut(&mut value)?;
            for key in prior.managed_keys {
                let _ = take_leaf(host, &key);
            }
        }
        emit(&value, is_jsonc)
    }

    pub(super) fn extract(
        content: &str,
        is_jsonc: bool,
    ) -> Result<Option<ManagedRegion>, SentinelError> {
        let value = parse(content, is_jsonc)?;
        let Value::Object(obj) = &value else {
            return Ok(None);
        };
        let Some(trove) = obj.get(TROVE_KEY) else {
            return Ok(None);
        };
        let trove_obj = trove.as_object().ok_or_else(|| {
            SentinelError::RegionMalformed(format!(
                "{TROVE_KEY} is not a JSON object"
            ))
        })?;

        let managed_keys = trove_obj
            .get("managed_keys")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                SentinelError::RegionMalformed(format!(
                    "{TROVE_KEY}.managed_keys missing or not an array"
                ))
            })?
            .iter()
            .map(|v| {
                v.as_str().map(String::from).ok_or_else(|| {
                    SentinelError::RegionMalformed(
                        "managed_keys entry is not a string".into(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Recorded hash is informational (used by tests). The hash we
        // return is freshly computed from the live payload so a user's
        // hand-edit inside the region surfaces as a conflict rather
        // than passing through unnoticed.
        let _recorded_hash = trove_obj
            .get("hash")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                SentinelError::RegionMalformed(format!(
                    "{TROVE_KEY}.hash missing or not a string"
                ))
            })?;

        // Reconstruct the payload object by reading the managed leaves
        // from the host document.
        let mut payload_obj = Map::new();
        for key in &managed_keys {
            if let Some(v) = read_leaf(obj, key) {
                set_leaf(&mut payload_obj, key, v.clone());
            }
        }
        let payload_value = Value::Object(payload_obj);
        let payload = serde_json::to_string(&payload_value)
            .map_err(|e| SentinelError::EmitFailed(e.to_string()))?;

        let canonical = super::canonical_json(&payload_value)?;
        let hash = super::hash_hex(&canonical);

        Ok(Some(ManagedRegion {
            managed_keys,
            payload,
            hash,
        }))
    }

    fn value_object_mut(value: &mut Value) -> Result<&mut Map<String, Value>, SentinelError> {
        value.as_object_mut().ok_or_else(|| {
            SentinelError::RegionMalformed("host JSON document is not an object".into())
        })
    }

    fn parse(content: &str, is_jsonc: bool) -> Result<Value, SentinelError> {
        if is_jsonc {
            // jsonc_parser::parse_to_serde_value handles comments + trailing
            // commas. Empty input is treated as an empty object so adapters
            // can patch a fresh-but-empty config file.
            let trimmed = content.trim();
            if trimmed.is_empty() {
                return Ok(Value::Object(Map::new()));
            }
            let value = jsonc_parser::parse_to_serde_value(content, &ParseOptions::default())
                .map_err(|e| SentinelError::Malformed {
                    format: format_for(is_jsonc),
                    message: e.to_string(),
                })?
                .unwrap_or(Value::Object(Map::new()));
            Ok(value)
        } else {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                return Ok(Value::Object(Map::new()));
            }
            serde_json::from_str(content).map_err(|e| SentinelError::Malformed {
                format: format_for(is_jsonc),
                message: e.to_string(),
            })
        }
    }

    fn emit(value: &Value, _is_jsonc: bool) -> Result<String, SentinelError> {
        // serde_json with `preserve_order` keeps key insertion order
        // stable across re-applies. JSONC-with-comments through a
        // serde-only round trip drops the user's comments; the
        // CST-based path documented in the plan is the upgrade target,
        // tracked as a known-issue for Sprint 4.
        let mut buf = serde_json::to_string_pretty(value)
            .map_err(|e| SentinelError::EmitFailed(e.to_string()))?;
        buf.push('\n');
        Ok(buf)
    }

    /// Returns the prior `_trove` block (if present) and removes it from
    /// the document. Used by both upsert (to clear stale managed keys)
    /// and remove (to discover what to clean up).
    fn strip_trove(value: &mut Value) -> Option<TrovePriorState> {
        let obj = value.as_object_mut()?;
        let trove = obj.remove(TROVE_KEY)?;
        let trove_obj = trove.as_object()?;
        let managed_keys = trove_obj.get("managed_keys").and_then(Value::as_array).map(
            |arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            },
        );
        Some(TrovePriorState {
            managed_keys: managed_keys.unwrap_or_default(),
        })
    }

    fn trove_metadata(managed_keys: &[String], hash: &str) -> Value {
        let mut meta = Map::new();
        meta.insert(
            "managed_keys".into(),
            Value::Array(managed_keys.iter().cloned().map(Value::String).collect()),
        );
        meta.insert("hash".into(), Value::String(hash.into()));
        Value::Object(meta)
    }

    fn collect_leaf_values(value: &Value, prefix: &str) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let path = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    if matches!(v, Value::Object(_)) {
                        out.extend(collect_leaf_values(v, &path));
                    } else {
                        out.push((path, v.clone()));
                    }
                }
            }
            other => {
                if !prefix.is_empty() {
                    out.push((prefix.into(), other.clone()));
                }
            }
        }
        out
    }

    fn read_leaf<'a>(obj: &'a Map<String, Value>, dotted: &str) -> Option<&'a Value> {
        let mut parts = dotted.split('.');
        let mut cur: &Value = obj.get(parts.next()?)?;
        for p in parts {
            cur = cur.as_object()?.get(p)?;
        }
        Some(cur)
    }

    struct TrovePriorState {
        managed_keys: Vec<String>,
    }
}

// ---------------------------------------------------------------------------
// TOML / YAML
// ---------------------------------------------------------------------------

mod comment_fence {
    use super::{FENCE_END, FENCE_START, Format, ManagedRegion, SentinelError};

    pub(super) fn upsert_toml(
        content: &str,
        region: &ManagedRegion,
    ) -> Result<String, SentinelError> {
        validate_toml(content)?;
        let new = replace_or_append(content, region)?;
        validate_toml(&new)?;
        Ok(new)
    }

    pub(super) fn upsert_yaml(
        content: &str,
        region: &ManagedRegion,
    ) -> Result<String, SentinelError> {
        validate_yaml(content)?;
        let new = replace_or_append(content, region)?;
        validate_yaml(&new)?;
        Ok(new)
    }

    pub(super) fn remove_toml(content: &str) -> Result<String, SentinelError> {
        validate_toml(content)?;
        let new = strip_block(content)?;
        validate_toml(&new)?;
        Ok(new)
    }

    pub(super) fn remove_yaml(content: &str) -> Result<String, SentinelError> {
        validate_yaml(content)?;
        let new = strip_block(content)?;
        validate_yaml(&new)?;
        Ok(new)
    }

    pub(super) fn extract_toml(
        content: &str,
    ) -> Result<Option<ManagedRegion>, SentinelError> {
        validate_toml(content)?;
        extract_block(content)
    }

    pub(super) fn extract_yaml(
        content: &str,
    ) -> Result<Option<ManagedRegion>, SentinelError> {
        validate_yaml(content)?;
        extract_block(content)
    }

    fn replace_or_append(
        content: &str,
        region: &ManagedRegion,
    ) -> Result<String, SentinelError> {
        let block = render_block(region);
        if let Some((start_idx, end_idx)) = locate_block(content)? {
            // Replace inclusive of the `# trove:end` line. Keep the
            // surrounding bytes untouched.
            let mut out = String::with_capacity(content.len() + block.len());
            out.push_str(&content[..start_idx]);
            out.push_str(&block);
            out.push_str(&content[end_idx..]);
            Ok(out)
        } else {
            let mut out = String::with_capacity(content.len() + block.len() + 1);
            out.push_str(content);
            if !content.is_empty() && !content.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&block);
            Ok(out)
        }
    }

    fn strip_block(content: &str) -> Result<String, SentinelError> {
        if let Some((start_idx, end_idx)) = locate_block(content)? {
            let mut out = String::with_capacity(content.len());
            out.push_str(&content[..start_idx]);
            out.push_str(&content[end_idx..]);
            Ok(out)
        } else {
            Ok(content.into())
        }
    }

    fn extract_block(content: &str) -> Result<Option<ManagedRegion>, SentinelError> {
        let Some((start_idx, end_idx)) = locate_block(content)? else {
            return Ok(None);
        };
        let block = &content[start_idx..end_idx];
        // The block we render is:
        //   # trove:start hash=<hex> keys=<comma-list>\n
        //   <payload>\n
        //   # trove:end\n
        // Parse it back. We deliberately *re-hash* the payload here
        // rather than trusting the header's hash field — if a user
        // edited inside the block, the header hash is stale and
        // returning it would mask the conflict.
        let mut lines = block.lines();
        let header = lines
            .next()
            .ok_or_else(|| SentinelError::RegionMalformed("empty managed region".into()))?;
        let (_header_hash, managed_keys) = parse_header(header)?;
        let mut payload_lines = Vec::new();
        for line in lines {
            if line.trim_start().starts_with(FENCE_END) {
                break;
            }
            payload_lines.push(line);
        }
        let mut payload = payload_lines.join("\n");
        if !payload.is_empty() && !payload.ends_with('\n') {
            payload.push('\n');
        }

        let hash = super::hash_hex(payload.as_bytes());
        Ok(Some(ManagedRegion {
            managed_keys,
            payload,
            hash,
        }))
    }

    fn render_block(region: &ManagedRegion) -> String {
        use std::fmt::Write as _;

        let keys = region.managed_keys.join(",");
        let mut out = String::new();
        let _ = writeln!(out, "{FENCE_START} hash={hash} keys={keys}", hash = region.hash);
        if !region.payload.is_empty() {
            out.push_str(&region.payload);
            if !region.payload.ends_with('\n') {
                out.push('\n');
            }
        }
        let _ = writeln!(out, "{FENCE_END}");
        out
    }

    /// Locate the byte range covering the whole sentinel block (start
    /// fence line through end fence line, both inclusive of trailing
    /// newline). Returns `None` if no fences are present.
    fn locate_block(content: &str) -> Result<Option<(usize, usize)>, SentinelError> {
        let starts: Vec<usize> = content
            .match_indices(FENCE_START)
            .filter(|(idx, _)| line_starts_with_fence(content, *idx, FENCE_START))
            .map(|(idx, _)| idx)
            .collect();
        let ends: Vec<usize> = content
            .match_indices(FENCE_END)
            .filter(|(idx, _)| line_starts_with_fence(content, *idx, FENCE_END))
            .map(|(idx, _)| idx)
            .collect();
        match (starts.len(), ends.len()) {
            (0, 0) => Ok(None),
            (1, 1) => {
                let start = starts[0];
                let end = ends[0];
                if end <= start {
                    return Err(SentinelError::RegionMalformed(
                        "trove:end appears before trove:start".into(),
                    ));
                }
                let after_end = content[end..]
                    .find('\n')
                    .map_or(content.len(), |off| end + off + 1);
                Ok(Some((start, after_end)))
            }
            _ => Err(SentinelError::MultipleRegions),
        }
    }

    fn line_starts_with_fence(content: &str, idx: usize, fence: &str) -> bool {
        let prefix_ok = idx == 0
            || content.as_bytes()[..idx]
                .iter()
                .rev()
                .take_while(|b| **b != b'\n')
                .all(u8::is_ascii_whitespace);
        if !prefix_ok {
            return false;
        }
        // Avoid matching `# trove:start` inside `# trove:started` etc.
        let after = &content[idx + fence.len()..];
        match after.chars().next() {
            None => true,
            Some(c) => matches!(c, ' ' | '\t' | '\n' | '\r'),
        }
    }

    fn parse_header(line: &str) -> Result<(String, Vec<String>), SentinelError> {
        // line looks like `# trove:start hash=<hex> keys=a,b,c`
        let rest = line
            .strip_prefix(FENCE_START)
            .ok_or_else(|| SentinelError::RegionMalformed("missing trove:start prefix".into()))?
            .trim_start();
        let mut hash: Option<String> = None;
        let mut keys: Vec<String> = Vec::new();
        for token in rest.split_ascii_whitespace() {
            if let Some(value) = token.strip_prefix("hash=") {
                hash = Some(value.into());
            } else if let Some(value) = token.strip_prefix("keys=") {
                if value.is_empty() {
                    keys = Vec::new();
                } else {
                    keys = value.split(',').map(str::to_string).collect();
                }
            }
        }
        let hash = hash.ok_or_else(|| {
            SentinelError::RegionMalformed("trove:start header missing hash=".into())
        })?;
        Ok((hash, keys))
    }

    fn validate_toml(content: &str) -> Result<(), SentinelError> {
        if content.trim().is_empty() {
            return Ok(());
        }
        content
            .parse::<toml_edit::DocumentMut>()
            .map(|_| ())
            .map_err(|e| SentinelError::Malformed {
                format: Format::Toml,
                message: e.to_string(),
            })
    }

    fn validate_yaml(content: &str) -> Result<(), SentinelError> {
        if content.trim().is_empty() {
            return Ok(());
        }
        // We only need to confirm parseability; we don't use the value.
        serde_yml::from_str::<serde_yml::Value>(content)
            .map(|_| ())
            .map_err(|e| SentinelError::Malformed {
                format: Format::Yaml,
                message: e.to_string(),
            })
    }
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

fn hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    hex::encode(digest)
}

fn canonical_json(value: &serde_json::Value) -> Result<Vec<u8>, SentinelError> {
    // Produce a sorted-keys serialization so the hash is stable across
    // input map orderings.
    let sorted = sort_json(value);
    serde_json::to_vec(&sorted).map_err(|e| SentinelError::EmitFailed(e.to_string()))
}

fn sort_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<_, _> =
                map.iter().map(|(k, v)| (k.clone(), sort_json(v))).collect();
            let mut out = serde_json::Map::with_capacity(sorted.len());
            for (k, v) in sorted {
                out.insert(k, v);
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(sort_json).collect())
        }
        other => other.clone(),
    }
}

fn leaf_paths(map: &serde_json::Map<String, serde_json::Value>, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (k, v) in map {
        let path = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            serde_json::Value::Object(inner) => out.extend(leaf_paths(inner, &path)),
            _ => out.push(path),
        }
    }
    out
}

fn set_leaf(
    map: &mut serde_json::Map<String, serde_json::Value>,
    dotted: &str,
    value: serde_json::Value,
) {
    let mut parts = dotted.split('.').peekable();
    let mut cursor = map;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            cursor.insert(part.to_string(), value);
            return;
        }
        let entry = cursor
            .entry(part.to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = serde_json::Value::Object(serde_json::Map::new());
        }
        cursor = entry.as_object_mut().expect("just-set object");
    }
}

fn take_leaf(
    map: &mut serde_json::Map<String, serde_json::Value>,
    dotted: &str,
) -> Option<serde_json::Value> {
    let parts: Vec<&str> = dotted.split('.').collect();
    take_leaf_inner(map, &parts)
}

fn take_leaf_inner(
    map: &mut serde_json::Map<String, serde_json::Value>,
    parts: &[&str],
) -> Option<serde_json::Value> {
    let (head, tail) = parts.split_first()?;
    if tail.is_empty() {
        return map.shift_remove(*head);
    }
    let removed = {
        let entry = map.get_mut(*head)?;
        let inner = entry.as_object_mut()?;
        let removed = take_leaf_inner(inner, tail);
        // Garbage-collect now-empty intermediate object.
        let is_empty = inner.is_empty();
        (removed, is_empty)
    };
    if removed.1 {
        map.shift_remove(*head);
    }
    removed.0
}

fn format_for(is_jsonc: bool) -> Format {
    if is_jsonc { Format::Jsonc } else { Format::Json }
}


#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn json_region(payload: serde_json::Value) -> ManagedRegion {
        let serde_json::Value::Object(map) = payload else {
            panic!("test helper requires object");
        };
        ManagedRegion::for_json_patches(&map).unwrap()
    }

    fn text_region(payload: &str, keys: &[&str]) -> ManagedRegion {
        ManagedRegion::for_text_block(
            payload,
            keys.iter().map(|k| (*k).to_string()).collect(),
        )
    }

    // --- Format roundtrip: sentinel keys are all stable across re-applies. -

    #[test]
    fn format_label_round_trip() {
        for f in [Format::Json, Format::Jsonc, Format::Toml, Format::Yaml] {
            assert!(!f.label().is_empty());
            assert_eq!(f.to_string(), f.label());
        }
    }

    // --- JSON ---

    #[test]
    fn json_fresh_install_then_idempotent() {
        let original = "{}";
        let region = json_region(serde_json::json!({
            "env": {
                "OTEL_EXPORTER_OTLP_ENDPOINT": "http://127.0.0.1:4318"
            }
        }));
        let after = upsert_region(Format::Json, original, &region).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(
            parsed["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"],
            "http://127.0.0.1:4318",
        );
        assert!(parsed.get("_trove").is_some());

        // Idempotent: re-applying the same region produces byte-identical output.
        let again = upsert_region(Format::Json, &after, &region).unwrap();
        assert_eq!(after, again);
    }

    #[test]
    fn json_user_keys_outside_block_survive_revert() {
        let original =
            r#"{"env":{"USER_KEY":"keep-me","OTEL_EXPORTER_OTLP_ENDPOINT":"old"}}"#;
        let region = json_region(serde_json::json!({
            "env": { "OTEL_EXPORTER_OTLP_ENDPOINT": "http://127.0.0.1:4318" }
        }));
        let after = upsert_region(Format::Json, original, &region).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["env"]["USER_KEY"], "keep-me");
        assert_eq!(
            parsed["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"],
            "http://127.0.0.1:4318"
        );

        let reverted = remove_region(Format::Json, &after).unwrap();
        let reverted_parsed: serde_json::Value = serde_json::from_str(&reverted).unwrap();
        assert_eq!(reverted_parsed["env"]["USER_KEY"], "keep-me");
        assert!(reverted_parsed["env"].get("OTEL_EXPORTER_OTLP_ENDPOINT").is_none());
        assert!(reverted_parsed.get("_trove").is_none());
    }

    #[test]
    fn json_extract_returns_recorded_region() {
        let region = json_region(serde_json::json!({
            "env": { "OTEL_FOO": "bar" }
        }));
        let after = upsert_region(Format::Json, "{}", &region).unwrap();
        let extracted = extract_region(Format::Json, &after).unwrap().unwrap();
        assert_eq!(extracted.managed_keys, vec!["env.OTEL_FOO"]);
        assert_eq!(extracted.hash, region.hash);
    }

    #[test]
    fn json_remove_when_absent_is_noop() {
        let original = r#"{"foo":1}"#;
        let after = remove_region(Format::Json, original).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["foo"], 1);
    }

    #[test]
    fn json_malformed_input_errors() {
        let err = upsert_region(
            Format::Json,
            "{not json",
            &json_region(serde_json::json!({"x":1})),
        )
        .unwrap_err();
        assert!(matches!(err, SentinelError::Malformed { .. }));
    }

    #[test]
    fn json_empty_input_treated_as_empty_object() {
        let region = json_region(serde_json::json!({"x":1}));
        let after = upsert_region(Format::Json, "", &region).unwrap();
        assert!(after.contains("\"x\": 1"));
    }

    // --- JSONC ---

    #[test]
    fn jsonc_with_comments_parses_and_round_trips() {
        let original = r#"{
            // user comment
            "env": { "USER_KEY": "x" }
        }"#;
        let region = json_region(serde_json::json!({
            "env": { "OTEL_FOO": "bar" }
        }));
        let after = upsert_region(Format::Jsonc, original, &region).unwrap();
        // Note: serde-only path drops comments — documented in the
        // module header. The data is preserved.
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["env"]["USER_KEY"], "x");
        assert_eq!(parsed["env"]["OTEL_FOO"], "bar");
    }

    // --- TOML ---

    #[test]
    fn toml_fresh_install_then_idempotent() {
        let original = "[user]\nname = \"jeff\"\n";
        let region = text_region(
            "[otel]\nendpoint = \"http://127.0.0.1:4318\"\n",
            &["otel.endpoint"],
        );
        let after = upsert_region(Format::Toml, original, &region).unwrap();
        assert!(after.contains(FENCE_START));
        assert!(after.contains(FENCE_END));
        assert!(after.contains("name = \"jeff\""));
        assert!(after.contains("endpoint = \"http://127.0.0.1:4318\""));

        let again = upsert_region(Format::Toml, &after, &region).unwrap();
        assert_eq!(after, again);
    }

    #[test]
    fn toml_remove_restores_byte_for_byte() {
        let original = "[user]\nname = \"jeff\"\n";
        let region = text_region(
            "[otel]\nendpoint = \"http://127.0.0.1:4318\"\n",
            &["otel.endpoint"],
        );
        let after = upsert_region(Format::Toml, original, &region).unwrap();
        let reverted = remove_region(Format::Toml, &after).unwrap();
        assert_eq!(reverted, original);
    }

    #[test]
    fn toml_extract_returns_payload_and_hash() {
        let original = "";
        let region = text_region(
            "[otel]\nendpoint = \"x\"\n",
            &["otel.endpoint"],
        );
        let after = upsert_region(Format::Toml, original, &region).unwrap();
        let got = extract_region(Format::Toml, &after).unwrap().unwrap();
        assert_eq!(got.managed_keys, vec!["otel.endpoint".to_string()]);
        assert_eq!(got.hash, region.hash);
        assert!(got.payload.contains("[otel]"));
    }

    #[test]
    fn toml_malformed_input_errors() {
        let region = text_region("payload\n", &[]);
        let err = upsert_region(Format::Toml, "[unterminated", &region).unwrap_err();
        assert!(matches!(err, SentinelError::Malformed { .. }));
    }

    #[test]
    fn toml_multiple_blocks_errors() {
        // Two valid trove blocks with distinct keys so the surrounding
        // doc still parses as TOML — otherwise validate_toml short-circuits
        // before locate_block runs and we'd see Malformed instead.
        let doc = format!(
            "{FENCE_START} hash=a keys=foo\nfoo = 1\n{FENCE_END}\n\
             {FENCE_START} hash=b keys=bar\nbar = 2\n{FENCE_END}\n",
        );
        let err = remove_region(Format::Toml, &doc).unwrap_err();
        assert!(
            matches!(err, SentinelError::MultipleRegions),
            "got {err:?}"
        );
    }

    // --- YAML ---

    #[test]
    fn yaml_fresh_install_then_idempotent() {
        let original = "service:\n  name: trove\n";
        let region = text_region(
            "otel:\n  endpoint: http://127.0.0.1:4318\n",
            &["otel.endpoint"],
        );
        let after = upsert_region(Format::Yaml, original, &region).unwrap();
        assert!(after.contains(FENCE_START));
        assert!(after.contains("endpoint: http://127.0.0.1:4318"));

        let again = upsert_region(Format::Yaml, &after, &region).unwrap();
        assert_eq!(after, again);
    }

    #[test]
    fn yaml_remove_restores_byte_for_byte() {
        let original = "service:\n  name: trove\n";
        let region = text_region(
            "otel:\n  endpoint: http://127.0.0.1:4318\n",
            &["otel.endpoint"],
        );
        let after = upsert_region(Format::Yaml, original, &region).unwrap();
        let reverted = remove_region(Format::Yaml, &after).unwrap();
        assert_eq!(reverted, original);
    }

    #[test]
    fn yaml_extract_returns_payload_and_hash() {
        let original = "";
        let region = text_region("otel:\n  endpoint: x\n", &["otel.endpoint"]);
        let after = upsert_region(Format::Yaml, original, &region).unwrap();
        let got = extract_region(Format::Yaml, &after).unwrap().unwrap();
        assert_eq!(got.managed_keys, vec!["otel.endpoint".to_string()]);
        assert_eq!(got.hash, region.hash);
    }

    #[test]
    fn yaml_malformed_input_errors() {
        // YAML with a tab inside indentation is invalid.
        let region = text_region("payload: x\n", &[]);
        let err =
            upsert_region(Format::Yaml, "service:\n\tname: trove\n", &region).unwrap_err();
        assert!(matches!(err, SentinelError::Malformed { .. }));
    }

    // --- helpers ---

    #[test]
    fn leaf_paths_walks_nested_object() {
        let payload = serde_json::json!({
            "a": { "b": { "c": 1 }, "d": 2 },
            "e": 3,
        });
        let serde_json::Value::Object(map) = payload else {
            panic!()
        };
        let mut paths = leaf_paths(&map, "");
        paths.sort();
        assert_eq!(paths, vec!["a.b.c", "a.d", "e"]);
    }

    #[test]
    fn set_leaf_creates_intermediate_objects() {
        let mut m = serde_json::Map::new();
        set_leaf(&mut m, "a.b.c", serde_json::json!(7));
        let v = serde_json::Value::Object(m);
        assert_eq!(v["a"]["b"]["c"], 7);
    }

    #[test]
    fn take_leaf_removes_value_and_collapses_empty_parents() {
        let mut v = serde_json::json!({"a": {"b": {"c": 1}}});
        let serde_json::Value::Object(ref mut m) = v else {
            panic!()
        };
        let removed = take_leaf(m, "a.b.c");
        assert_eq!(removed, Some(serde_json::json!(1)));
        // a and a.b are now empty and removed.
        let serde_json::Value::Object(map_after) = v else {
            panic!()
        };
        assert!(map_after.get("a").is_none());
    }

    #[test]
    fn canonical_json_orders_keys() {
        let v1 = serde_json::json!({"b":1,"a":2});
        let v2 = serde_json::json!({"a":2,"b":1});
        assert_eq!(canonical_json(&v1).unwrap(), canonical_json(&v2).unwrap());
    }

    #[test]
    fn hash_hex_is_64_chars() {
        let h = hash_hex(b"trove");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
