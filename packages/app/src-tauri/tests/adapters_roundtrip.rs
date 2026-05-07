//! Cross-harness integration test for the Tier 1 adapter set.
//!
//! Asserts the end-to-end "apply → revert → byte-identical" contract
//! the MVP plan calls out in Sprint 3's acceptance criteria (extended
//! to all four Tier 1 adapters in Sprint 4). Each adapter is exercised
//! against a fresh `tempdir`-scoped `$HOME` carrying pre-existing user
//! keys in the harness's native config format.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::tempdir;

use trove_app::adapters::{ApplyOptions, claude_code, codex_cli, gemini_cli, qwen_code};

const CLAUDE_ORIGINAL: &str =
    "{\n  \"theme\": \"dark\",\n  \"env\": {\n    \"MY_USER_VAR\": \"keepme\"\n  }\n}\n";
const GEMINI_ORIGINAL: &str =
    "{\n  \"theme\": \"dark\",\n  \"model\": {\n    \"name\": \"flash\"\n  }\n}\n";
const QWEN_ORIGINAL: &str =
    "{\n  \"theme\": \"dark\",\n  \"model\": {\n    \"name\": \"qwen3-coder\"\n  }\n}\n";
const CODEX_ORIGINAL: &str = "[user]\nname = \"jeff\"\n\n[model]\ndefault = \"o1\"\n";

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn all_four_tier_1_adapters_apply_and_revert_byte_identical() {
    let home = tempdir().unwrap();

    let claude_path = claude_code::config_path(home.path());
    let codex_path = codex_cli::config_path(home.path());
    let gemini_path = gemini_cli::config_path(home.path());
    let qwen_path = qwen_code::config_path(home.path());

    write(&claude_path, CLAUDE_ORIGINAL);
    write(&codex_path, CODEX_ORIGINAL);
    write(&gemini_path, GEMINI_ORIGINAL);
    write(&qwen_path, QWEN_ORIGINAL);

    // Apply all four. Bindings named to dodge clippy::similar_names
    // (which trips on `claude_patch` / `claude_path`).
    let claude_metadata = claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    let codex_metadata = codex_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
    let gemini_metadata = gemini_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
    let qwen_metadata = qwen_code::apply(home.path(), &ApplyOptions::default()).unwrap();

    // Each JSON file must parse, preserve user keys, and contain the
    // harness-specific Trove block.
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

    let qwen_after: Value = serde_json::from_str(&fs::read_to_string(&qwen_path).unwrap())
        .expect("post-apply Qwen settings.json must parse as JSON");
    assert_eq!(qwen_after["theme"], "dark");
    assert_eq!(qwen_after["model"]["name"], "qwen3-coder");
    assert_eq!(qwen_after["telemetry"]["enabled"], true);
    assert_eq!(
        qwen_after["telemetry"]["otlpEndpoint"],
        "http://127.0.0.1:4318"
    );
    assert!(qwen_after.get("_trove").is_some());

    // Codex is TOML; assertions use toml_edit instead of serde_json.
    let codex_text = fs::read_to_string(&codex_path).unwrap();
    let codex_doc: toml_edit::DocumentMut = codex_text
        .parse()
        .expect("post-apply Codex config.toml must parse as TOML");
    assert_eq!(codex_doc["user"]["name"].as_str(), Some("jeff"));
    assert_eq!(codex_doc["model"]["default"].as_str(), Some("o1"));
    assert_eq!(
        codex_doc["otel"]["exporter"]["kind"].as_str(),
        Some("otlp-http")
    );
    assert_eq!(
        codex_doc["otel"]["exporter"]["endpoint"].as_str(),
        Some("http://127.0.0.1:4318/v1/logs")
    );
    assert!(codex_text.contains("# trove:start"));
    assert!(codex_text.contains("# trove:end"));

    // Returned TrovePatch metadata is well-formed across all four.
    for metadata in [
        &claude_metadata,
        &codex_metadata,
        &gemini_metadata,
        &qwen_metadata,
    ] {
        assert_eq!(metadata.managed_block_hash.len(), 64);
        assert_eq!(metadata.file_hash_at_last_write.len(), 64);
    }

    // Now revert all four. The acceptance criterion: byte-identical to
    // the pre-apply file, including the trailing newline.
    claude_code::revert(home.path()).unwrap();
    codex_cli::revert(home.path()).unwrap();
    gemini_cli::revert(home.path()).unwrap();
    qwen_code::revert(home.path()).unwrap();

    assert_eq!(
        fs::read_to_string(&claude_path).unwrap(),
        CLAUDE_ORIGINAL,
        "Claude config must be byte-identical post-revert"
    );
    assert_eq!(
        fs::read_to_string(&codex_path).unwrap(),
        CODEX_ORIGINAL,
        "Codex config must be byte-identical post-revert"
    );
    assert_eq!(
        fs::read_to_string(&gemini_path).unwrap(),
        GEMINI_ORIGINAL,
        "Gemini config must be byte-identical post-revert"
    );
    assert_eq!(
        fs::read_to_string(&qwen_path).unwrap(),
        QWEN_ORIGINAL,
        "Qwen config must be byte-identical post-revert"
    );
}

#[test]
fn applying_one_adapter_does_not_disturb_the_other_files() {
    let home = tempdir().unwrap();

    let claude_path = claude_code::config_path(home.path());
    let codex_path = codex_cli::config_path(home.path());
    let gemini_path = gemini_cli::config_path(home.path());
    let qwen_path = qwen_code::config_path(home.path());

    write(&claude_path, CLAUDE_ORIGINAL);
    write(&codex_path, CODEX_ORIGINAL);
    write(&gemini_path, GEMINI_ORIGINAL);
    write(&qwen_path, QWEN_ORIGINAL);

    claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();

    // The other three are untouched.
    assert_eq!(fs::read_to_string(&codex_path).unwrap(), CODEX_ORIGINAL);
    assert_eq!(fs::read_to_string(&gemini_path).unwrap(), GEMINI_ORIGINAL);
    assert_eq!(fs::read_to_string(&qwen_path).unwrap(), QWEN_ORIGINAL);
}

#[test]
fn fresh_install_works_when_no_files_exist() {
    let home = tempdir().unwrap();

    claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    codex_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
    gemini_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
    qwen_code::apply(home.path(), &ApplyOptions::default()).unwrap();

    let claude_path = claude_code::config_path(home.path());
    let codex_path = codex_cli::config_path(home.path());
    let gemini_path = gemini_cli::config_path(home.path());
    let qwen_path = qwen_code::config_path(home.path());

    // All four parent dirs created; all four files parse.
    assert!(claude_path.exists());
    assert!(codex_path.exists());
    assert!(gemini_path.exists());
    assert!(qwen_path.exists());

    let _claude: Value =
        serde_json::from_str(&fs::read_to_string(&claude_path).unwrap()).unwrap();
    let _codex: toml_edit::DocumentMut = fs::read_to_string(&codex_path).unwrap().parse().unwrap();
    let _gemini: Value =
        serde_json::from_str(&fs::read_to_string(&gemini_path).unwrap()).unwrap();
    let _qwen: Value = serde_json::from_str(&fs::read_to_string(&qwen_path).unwrap()).unwrap();
}
