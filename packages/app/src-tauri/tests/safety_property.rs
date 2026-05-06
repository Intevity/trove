//! Property-based tests for `safety::sentinels`.
//!
//! The Sprint 2 acceptance bar calls for "1000 random patches → revert →
//! file is byte-identical to original" for the comment-fenced formats,
//! and equivalent normalised-revert behaviour for JSON. We use proptest
//! to generate arbitrary inputs and assert three properties per format:
//!
//! 1. **Roundtrip** — `revert(apply(file, region))` lands at the same
//!    output as `revert(file)` (which is a no-op for files that don't
//!    contain a managed region).
//! 2. **Idempotency** — applying the same region twice produces
//!    byte-identical output the second time.
//! 3. **Extract returns the live payload hash** — the hash returned by
//!    `extract_region` matches the hash we'd compute from the current
//!    payload, not a stale recorded value.

use proptest::prelude::*;
use trove_app::safety::sentinels::{
    Format, ManagedRegion, extract_region, remove_region, upsert_region,
};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Generate a small ASCII-only key suitable for a JSON object key, a TOML
/// table name, or a YAML mapping key.
fn key_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_map(String::from)
}

/// Random JSON object payload with one or two leaf keypaths.
fn json_payload_strategy() -> impl Strategy<Value = serde_json::Map<String, serde_json::Value>>
{
    (
        key_strategy(),
        key_strategy(),
        prop_oneof![
            "[a-z0-9 ]{0,12}".prop_map(serde_json::Value::String),
            any::<i32>().prop_map(|n| serde_json::Value::Number(n.into())),
            any::<bool>().prop_map(serde_json::Value::Bool),
        ],
    )
        .prop_map(|(outer, inner, value)| {
            let mut nested = serde_json::Map::new();
            nested.insert(inner, value);
            let mut map = serde_json::Map::new();
            map.insert(outer, serde_json::Value::Object(nested));
            map
        })
}

/// Random initial JSON document — a top-level object with 0..=2 unrelated
/// keys whose values won't collide with the patch.
fn json_initial_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        (
            "user_[a-z]{1,4}".prop_map(String::from),
            "[a-z0-9 ]{0,8}".prop_map(String::from),
        ),
        0..=2,
    )
    .prop_map(|pairs| {
        let mut obj = serde_json::Map::new();
        for (k, v) in pairs {
            obj.insert(k, serde_json::Value::String(v));
        }
        serde_json::to_string(&serde_json::Value::Object(obj)).unwrap()
    })
}

/// Random TOML payload (a top-level key=value line, deterministic format).
fn toml_payload_strategy() -> impl Strategy<Value = (String, String, String)> {
    (key_strategy(), key_strategy(), "[a-z0-9 ]{0,12}".prop_map(String::from))
}

/// Random initial TOML document — zero or one user table.
fn toml_initial_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        (key_strategy(), key_strategy(), "[a-z0-9]{0,8}".prop_map(String::from)).prop_map(
            |(table, k, v)| format!("[user_{table}]\n{k} = \"{v}\"\n")
        ),
    ]
}

/// Random YAML payload — a single mapping key.
fn yaml_payload_strategy() -> impl Strategy<Value = (String, String)> {
    (key_strategy(), "[a-z0-9 ]{0,12}".prop_map(String::from))
}

/// Random initial YAML document — zero or one user mapping at the top.
fn yaml_initial_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        (key_strategy(), "[a-z0-9 ]{0,8}".prop_map(String::from))
            .prop_map(|(k, v)| format!("user_{k}: {v}\n")),
    ]
}

// ---------------------------------------------------------------------------
// Properties — JSON
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(250))]

    #[test]
    fn json_apply_revert_normalises_to_baseline(
        initial in json_initial_strategy(),
        patches in json_payload_strategy(),
    ) {
        let region = ManagedRegion::for_json_patches(&patches).unwrap();

        // Skip if the patch's outer key collides with an unrelated user
        // key in the initial doc — that would reasonably overwrite it,
        // so revert wouldn't restore the original.
        let initial_value: serde_json::Value =
            serde_json::from_str(&initial).unwrap();
        if let serde_json::Value::Object(map) = &initial_value {
            for outer_key in patches.keys() {
                if map.contains_key(outer_key) {
                    return Ok(());
                }
            }
        }

        let after_apply = upsert_region(Format::Json, &initial, &region).unwrap();
        let after_revert = remove_region(Format::Json, &after_apply).unwrap();
        let baseline = remove_region(Format::Json, &initial).unwrap();

        prop_assert_eq!(after_revert, baseline);
    }

    #[test]
    fn json_apply_is_idempotent(
        initial in json_initial_strategy(),
        patches in json_payload_strategy(),
    ) {
        let region = ManagedRegion::for_json_patches(&patches).unwrap();
        let first = upsert_region(Format::Json, &initial, &region).unwrap();
        let second = upsert_region(Format::Json, &first, &region).unwrap();
        prop_assert_eq!(first, second);
    }

    #[test]
    fn json_extract_returns_live_hash(
        patches in json_payload_strategy(),
    ) {
        let region = ManagedRegion::for_json_patches(&patches).unwrap();
        let after = upsert_region(Format::Json, "{}", &region).unwrap();
        let extracted = extract_region(Format::Json, &after).unwrap().unwrap();
        prop_assert_eq!(extracted.hash, region.hash);
    }
}

// ---------------------------------------------------------------------------
// Properties — TOML
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(250))]

    #[test]
    fn toml_apply_revert_byte_identical(
        initial in toml_initial_strategy(),
        (table, key, val) in toml_payload_strategy(),
    ) {
        let payload = format!("[trove_{table}]\n{key} = \"{val}\"\n");
        let region = ManagedRegion::for_text_block(payload, vec![format!("trove_{table}.{key}")]);

        let after_apply = upsert_region(Format::Toml, &initial, &region).unwrap();
        let after_revert = remove_region(Format::Toml, &after_apply).unwrap();
        prop_assert_eq!(after_revert, initial);
    }

    #[test]
    fn toml_apply_is_idempotent(
        initial in toml_initial_strategy(),
        (table, key, val) in toml_payload_strategy(),
    ) {
        let payload = format!("[trove_{table}]\n{key} = \"{val}\"\n");
        let region = ManagedRegion::for_text_block(payload, vec![format!("trove_{table}.{key}")]);

        let first = upsert_region(Format::Toml, &initial, &region).unwrap();
        let second = upsert_region(Format::Toml, &first, &region).unwrap();
        prop_assert_eq!(first, second);
    }

    #[test]
    fn toml_extract_returns_live_hash(
        (table, key, val) in toml_payload_strategy(),
    ) {
        let payload = format!("[trove_{table}]\n{key} = \"{val}\"\n");
        let region = ManagedRegion::for_text_block(payload, vec![]);
        let after = upsert_region(Format::Toml, "", &region).unwrap();
        let extracted = extract_region(Format::Toml, &after).unwrap().unwrap();
        prop_assert_eq!(extracted.hash, region.hash);
    }
}

// ---------------------------------------------------------------------------
// Properties — YAML
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(250))]

    #[test]
    fn yaml_apply_revert_byte_identical(
        initial in yaml_initial_strategy(),
        (key, val) in yaml_payload_strategy(),
    ) {
        let payload = format!("trove_{key}: {val}\n");
        let region = ManagedRegion::for_text_block(payload, vec![format!("trove_{key}")]);

        let after_apply = upsert_region(Format::Yaml, &initial, &region).unwrap();
        let after_revert = remove_region(Format::Yaml, &after_apply).unwrap();
        prop_assert_eq!(after_revert, initial);
    }

    #[test]
    fn yaml_apply_is_idempotent(
        initial in yaml_initial_strategy(),
        (key, val) in yaml_payload_strategy(),
    ) {
        let payload = format!("trove_{key}: {val}\n");
        let region = ManagedRegion::for_text_block(payload, vec![]);

        let first = upsert_region(Format::Yaml, &initial, &region).unwrap();
        let second = upsert_region(Format::Yaml, &first, &region).unwrap();
        prop_assert_eq!(first, second);
    }
}

// ---------------------------------------------------------------------------
// Property — JSONC (without comments — comment preservation is a known
// Sprint 4 upgrade target tracked in sentinels.rs).
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(250))]

    #[test]
    fn jsonc_apply_revert_normalises_to_baseline(
        patches in json_payload_strategy(),
    ) {
        let region = ManagedRegion::for_json_patches(&patches).unwrap();
        let initial = "{}";
        let after_apply = upsert_region(Format::Jsonc, initial, &region).unwrap();
        let after_revert = remove_region(Format::Jsonc, &after_apply).unwrap();
        let baseline = remove_region(Format::Jsonc, initial).unwrap();
        prop_assert_eq!(after_revert, baseline);
    }
}
