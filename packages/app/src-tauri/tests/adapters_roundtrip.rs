//! Cross-harness integration test for the Sprint 3 adapter trio.
//!
//! Asserts the end-to-end "apply → revert → byte-identical" contract
//! the MVP plan calls out in Sprint 3's acceptance criteria. Both
//! Tier 1 adapters with implementations (claude-code, gemini-cli) are
//! exercised against a fresh `tempdir`-scoped `$HOME` carrying
//! pre-existing user keys.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::tempdir;

use trove_app::adapters::{ApplyOptions, claude_code, gemini_cli};

const CLAUDE_ORIGINAL: &str =
    "{\n  \"theme\": \"dark\",\n  \"env\": {\n    \"MY_USER_VAR\": \"keepme\"\n  }\n}\n";
const GEMINI_ORIGINAL: &str =
    "{\n  \"theme\": \"dark\",\n  \"model\": {\n    \"name\": \"flash\"\n  }\n}\n";

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn both_adapters_apply_and_revert_byte_identical() {
    let home = tempdir().unwrap();

    let claude_path = claude_code::config_path(home.path());
    let gemini_path = gemini_cli::config_path(home.path());

    write(&claude_path, CLAUDE_ORIGINAL);
    write(&gemini_path, GEMINI_ORIGINAL);

    // Apply both. Bindings are named to dodge clippy::similar_names
    // (which trips on `claude_patch` / `claude_path`).
    let claude_metadata = claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    let gemini_metadata = gemini_cli::apply(home.path(), &ApplyOptions::default()).unwrap();

    // Both files must parse as JSON, preserve user keys, and contain
    // the harness-specific Trove block.
    let claude_after: Value = serde_json::from_str(&fs::read_to_string(&claude_path).unwrap())
        .expect("post-apply Claude settings.json must parse as JSON");
    assert_eq!(claude_after["theme"], "dark");
    assert_eq!(claude_after["env"]["MY_USER_VAR"], "keepme");
    assert_eq!(
        claude_after["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"],
        "http://127.0.0.1:4318"
    );
    assert!(claude_after.get("_trove").is_some());

    let gemini_after: Value = serde_json::from_str(&fs::read_to_string(&gemini_path).unwrap())
        .expect("post-apply Gemini settings.json must parse as JSON");
    assert_eq!(gemini_after["theme"], "dark");
    assert_eq!(gemini_after["model"]["name"], "flash");
    assert_eq!(gemini_after["telemetry"]["enabled"], true);
    assert_eq!(
        gemini_after["telemetry"]["otlpEndpoint"],
        "http://127.0.0.1:4318"
    );
    assert!(gemini_after.get("_trove").is_some());

    // Returned TrovePatch metadata is well-formed.
    assert_eq!(claude_metadata.managed_block_hash.len(), 64);
    assert_eq!(claude_metadata.file_hash_at_last_write.len(), 64);
    assert_eq!(gemini_metadata.managed_block_hash.len(), 64);
    assert_eq!(gemini_metadata.file_hash_at_last_write.len(), 64);

    // Now revert both. The acceptance criterion: byte-identical to the
    // pre-apply file, including the trailing newline.
    claude_code::revert(home.path()).unwrap();
    gemini_cli::revert(home.path()).unwrap();

    let claude_after_revert = fs::read_to_string(&claude_path).unwrap();
    assert_eq!(
        claude_after_revert, CLAUDE_ORIGINAL,
        "Claude config must be byte-identical post-revert"
    );

    let gemini_after_revert = fs::read_to_string(&gemini_path).unwrap();
    assert_eq!(
        gemini_after_revert, GEMINI_ORIGINAL,
        "Gemini config must be byte-identical post-revert"
    );
}

#[test]
fn applying_one_adapter_does_not_disturb_the_other_file() {
    let home = tempdir().unwrap();

    let claude_path = claude_code::config_path(home.path());
    let gemini_path = gemini_cli::config_path(home.path());

    write(&claude_path, CLAUDE_ORIGINAL);
    write(&gemini_path, GEMINI_ORIGINAL);

    claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();

    // Gemini's file is untouched.
    assert_eq!(fs::read_to_string(&gemini_path).unwrap(), GEMINI_ORIGINAL);
}

#[test]
fn fresh_install_works_when_neither_file_exists() {
    let home = tempdir().unwrap();

    claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    gemini_cli::apply(home.path(), &ApplyOptions::default()).unwrap();

    // Both parent dirs created; both files parse.
    let claude_path = claude_code::config_path(home.path());
    let gemini_path = gemini_cli::config_path(home.path());
    assert!(claude_path.exists());
    assert!(gemini_path.exists());
    let _claude: Value =
        serde_json::from_str(&fs::read_to_string(&claude_path).unwrap()).unwrap();
    let _gemini: Value =
        serde_json::from_str(&fs::read_to_string(&gemini_path).unwrap()).unwrap();
}
