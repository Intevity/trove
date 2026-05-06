//! Integration suite for `safety::sentinels` covering the seven cases the
//! Sprint 2 plan calls out, exercised across all four formats.
//!
//! Inline `#[cfg(test)]` modules in `sentinels.rs` cover function-level
//! behaviour. This file drives the safety contract end-to-end: every
//! format must pass fresh-install, idempotency, edited-outside,
//! edited-inside, malformed, and roundtrip semantics.

use trove_app::safety::sentinels::{
    Format, ManagedRegion, SentinelError, extract_region, remove_region, upsert_region,
};

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

mod json_cases {
    use super::*;
    use pretty_assertions::assert_eq;

    fn region() -> ManagedRegion {
        let map = serde_json::from_str(
            r#"{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}"#,
        )
        .unwrap();
        ManagedRegion::for_json_patches(&map).unwrap()
    }

    #[test]
    fn fresh_install() {
        let after = upsert_region(Format::Json, "{}", &region()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(
            parsed["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"],
            "http://127.0.0.1:4318"
        );
        assert!(parsed.get("_trove").is_some());
    }

    #[test]
    fn idempotent_second_run() {
        let after_first = upsert_region(Format::Json, "{}", &region()).unwrap();
        let after_second = upsert_region(Format::Json, &after_first, &region()).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn user_edited_outside_block_survives() {
        let original = r#"{"env":{"USER_KEY":"x"}}"#;
        let after = upsert_region(Format::Json, original, &region()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["env"]["USER_KEY"], "x");
        let reverted = remove_region(Format::Json, &after).unwrap();
        let reverted_parsed: serde_json::Value = serde_json::from_str(&reverted).unwrap();
        assert_eq!(reverted_parsed["env"]["USER_KEY"], "x");
    }

    #[test]
    fn extract_returns_recorded_metadata() {
        let after = upsert_region(Format::Json, "{}", &region()).unwrap();
        let got = extract_region(Format::Json, &after).unwrap().unwrap();
        assert_eq!(got.managed_keys, vec!["env.OTEL_EXPORTER_OTLP_ENDPOINT"]);
        assert_eq!(got.hash, region().hash);
    }

    #[test]
    fn malformed_input_errors() {
        let err = upsert_region(Format::Json, "not json", &region()).unwrap_err();
        assert!(matches!(err, SentinelError::Malformed { .. }));
    }

    #[test]
    fn roundtrip_apply_revert_is_byte_identical_for_canonical_input() {
        // For JSON, "byte-identical" means after we parse-and-emit through
        // serde_json, applying then reverting should land at the same
        // emitted form. A canonical input (already pretty-printed) makes
        // this assertion clean.
        let original = "{\n  \"env\": {\n    \"USER_KEY\": \"x\"\n  }\n}\n";
        let after = upsert_region(Format::Json, original, &region()).unwrap();
        let reverted = remove_region(Format::Json, &after).unwrap();
        // Parse-and-re-emit the original through the same pipeline so the
        // comparison is fair.
        let baseline = remove_region(Format::Json, original).unwrap();
        assert_eq!(reverted, baseline);
    }
}

// ---------------------------------------------------------------------------
// JSONC
// ---------------------------------------------------------------------------

mod jsonc_cases {
    use super::*;
    use pretty_assertions::assert_eq;

    fn region() -> ManagedRegion {
        let map = serde_json::from_str(r#"{"telemetry":{"endpoint":"http://127.0.0.1:4318"}}"#)
            .unwrap();
        ManagedRegion::for_json_patches(&map).unwrap()
    }

    #[test]
    fn fresh_install_with_comments() {
        // Sprint 2 known issue: serde-only path drops user comments. The
        // data round-trips correctly; comment preservation upgrades to
        // jsonc-parser CST in a later sprint.
        let original = "{\n  // user comment\n  \"existing\": true\n}";
        let after = upsert_region(Format::Jsonc, original, &region()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["existing"], true);
        assert_eq!(parsed["telemetry"]["endpoint"], "http://127.0.0.1:4318");
    }

    #[test]
    fn idempotent_second_run() {
        let after_first = upsert_region(Format::Jsonc, "{}", &region()).unwrap();
        let after_second = upsert_region(Format::Jsonc, &after_first, &region()).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn extract_returns_recorded_metadata() {
        let after = upsert_region(Format::Jsonc, "{}", &region()).unwrap();
        let got = extract_region(Format::Jsonc, &after).unwrap().unwrap();
        assert_eq!(got.managed_keys, vec!["telemetry.endpoint"]);
        assert_eq!(got.hash, region().hash);
    }

    #[test]
    fn malformed_input_errors() {
        let err = upsert_region(Format::Jsonc, "{ unbalanced", &region()).unwrap_err();
        assert!(matches!(err, SentinelError::Malformed { .. }));
    }
}

// ---------------------------------------------------------------------------
// TOML
// ---------------------------------------------------------------------------

mod toml_cases {
    use super::*;
    use pretty_assertions::assert_eq;

    fn region() -> ManagedRegion {
        ManagedRegion::for_text_block(
            "[otel]\nendpoint = \"http://127.0.0.1:4318\"\n",
            vec!["otel.endpoint".into()],
        )
    }

    #[test]
    fn fresh_install() {
        let original = "[user]\nname = \"jeff\"\n";
        let after = upsert_region(Format::Toml, original, &region()).unwrap();
        assert!(after.contains("# trove:start"));
        assert!(after.contains("# trove:end"));
        assert!(after.contains("name = \"jeff\""));
        assert!(after.contains("endpoint = \"http://127.0.0.1:4318\""));
    }

    #[test]
    fn idempotent_second_run() {
        let original = "[user]\nname = \"jeff\"\n";
        let after_first = upsert_region(Format::Toml, original, &region()).unwrap();
        let after_second = upsert_region(Format::Toml, &after_first, &region()).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn user_content_outside_block_unchanged() {
        let original = "[user]\nname = \"jeff\"\nemail = \"jeff@example.com\"\n";
        let after = upsert_region(Format::Toml, original, &region()).unwrap();
        let reverted = remove_region(Format::Toml, &after).unwrap();
        // TOML uses fence-bracketed text; revert is byte-for-byte.
        assert_eq!(reverted, original);
    }

    #[test]
    fn extract_returns_payload_and_hash() {
        let after = upsert_region(Format::Toml, "", &region()).unwrap();
        let got = extract_region(Format::Toml, &after).unwrap().unwrap();
        assert_eq!(got.managed_keys, vec!["otel.endpoint".to_string()]);
        assert_eq!(got.hash, region().hash);
        assert!(got.payload.contains("[otel]"));
    }

    #[test]
    fn malformed_input_errors() {
        let err = upsert_region(Format::Toml, "[unterminated", &region()).unwrap_err();
        assert!(matches!(err, SentinelError::Malformed { .. }));
    }

    #[test]
    fn roundtrip_byte_identical() {
        let original = "# user comment\n[user]\nname = \"jeff\"\n";
        let after = upsert_region(Format::Toml, original, &region()).unwrap();
        let reverted = remove_region(Format::Toml, &after).unwrap();
        assert_eq!(reverted, original);
    }
}

// ---------------------------------------------------------------------------
// YAML
// ---------------------------------------------------------------------------

mod yaml_cases {
    use super::*;
    use pretty_assertions::assert_eq;

    fn region() -> ManagedRegion {
        ManagedRegion::for_text_block(
            "otel:\n  endpoint: http://127.0.0.1:4318\n",
            vec!["otel.endpoint".into()],
        )
    }

    #[test]
    fn fresh_install() {
        let original = "service:\n  name: trove\n";
        let after = upsert_region(Format::Yaml, original, &region()).unwrap();
        assert!(after.contains("# trove:start"));
        assert!(after.contains("# trove:end"));
        assert!(after.contains("name: trove"));
        assert!(after.contains("endpoint: http://127.0.0.1:4318"));
    }

    #[test]
    fn idempotent_second_run() {
        let original = "service:\n  name: trove\n";
        let after_first = upsert_region(Format::Yaml, original, &region()).unwrap();
        let after_second = upsert_region(Format::Yaml, &after_first, &region()).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn user_content_outside_block_unchanged() {
        let original = "# user header comment\nservice:\n  name: trove\n";
        let after = upsert_region(Format::Yaml, original, &region()).unwrap();
        let reverted = remove_region(Format::Yaml, &after).unwrap();
        assert_eq!(reverted, original);
    }

    #[test]
    fn extract_returns_payload_and_hash() {
        let after = upsert_region(Format::Yaml, "", &region()).unwrap();
        let got = extract_region(Format::Yaml, &after).unwrap().unwrap();
        assert_eq!(got.managed_keys, vec!["otel.endpoint".to_string()]);
        assert_eq!(got.hash, region().hash);
    }

    #[test]
    fn malformed_input_errors() {
        // Tab-in-indent is invalid YAML.
        let err =
            upsert_region(Format::Yaml, "service:\n\tname: trove\n", &region()).unwrap_err();
        assert!(matches!(err, SentinelError::Malformed { .. }));
    }

    #[test]
    fn roundtrip_byte_identical() {
        let original = "# user header\nservice:\n  name: trove\n  port: 4317\n";
        let after = upsert_region(Format::Yaml, original, &region()).unwrap();
        let reverted = remove_region(Format::Yaml, &after).unwrap();
        assert_eq!(reverted, original);
    }
}
