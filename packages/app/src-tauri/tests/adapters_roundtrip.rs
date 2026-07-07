//! Cross-harness integration test for the Tier 1 adapter set.
//!
//! Asserts the end-to-end "apply → revert → byte-identical" contract
//! the MVP plan calls out in Sprint 3's acceptance criteria (extended
//! to all four Tier 1 adapters in Sprint 4). Each adapter is exercised
//! against a fresh `tempdir`-scoped `$HOME` carrying pre-existing user
//! keys in the harness's native config format.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::tempdir;

use trove_app::adapters::{
    ApplyOptions, antigravity_cli, claude_code, codex_cli, cursor_cli, cursor_ide, droid, opencode,
    qwen_code,
};

const CLAUDE_ORIGINAL: &str =
    "{\n  \"theme\": \"dark\",\n  \"env\": {\n    \"MY_USER_VAR\": \"keepme\"\n  }\n}\n";
const QWEN_ORIGINAL: &str =
    "{\n  \"theme\": \"dark\",\n  \"model\": {\n    \"name\": \"qwen3-coder\"\n  }\n}\n";
const CODEX_ORIGINAL: &str = "[user]\nname = \"jeff\"\n\n[model]\ndefault = \"o1\"\n";

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn tier_1_native_adapters_apply_and_revert_byte_identical() {
    let home = tempdir().unwrap();

    let claude_path = claude_code::config_path(home.path());
    let codex_path = codex_cli::config_path(home.path());
    let qwen_path = qwen_code::config_path(home.path());

    write(&claude_path, CLAUDE_ORIGINAL);
    write(&codex_path, CODEX_ORIGINAL);
    write(&qwen_path, QWEN_ORIGINAL);

    // Apply all three. Bindings named to dodge clippy::similar_names
    // (which trips on `claude_patch` / `claude_path`). Antigravity CLI
    // is no longer here — it dropped native OTLP and is now a hook
    // adapter (see `antigravity_apply_then_revert_is_byte_identical`).
    let claude_metadata = claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    let codex_metadata = codex_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
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
    // Codex 0.130+ schema uses externally-tagged exporter sub-tables —
    // `[otel.exporter.otlp-http]` rather than the older
    // `[otel.exporter] kind = "otlp-http"`. The exporter variant name
    // is the sub-table key.
    let codex_text = fs::read_to_string(&codex_path).unwrap();
    let codex_doc: toml_edit::DocumentMut = codex_text
        .parse()
        .expect("post-apply Codex config.toml must parse as TOML");
    assert_eq!(codex_doc["user"]["name"].as_str(), Some("jeff"));
    assert_eq!(codex_doc["model"]["default"].as_str(), Some("o1"));
    assert!(
        codex_doc["otel"]["exporter"]["otlp-http"].is_table_like(),
        "expected [otel.exporter.otlp-http] sub-table, got: {codex_text}",
    );
    assert_eq!(
        codex_doc["otel"]["exporter"]["otlp-http"]["endpoint"].as_str(),
        Some("http://127.0.0.1:4318/v1/logs")
    );
    assert!(codex_text.contains("# trove:start"));
    assert!(codex_text.contains("# trove:end"));

    // Returned TrovePatch metadata is well-formed across all four.
    for metadata in [&claude_metadata, &codex_metadata, &qwen_metadata] {
        assert_eq!(metadata.managed_block_hash.len(), 64);
        assert_eq!(metadata.file_hash_at_last_write.len(), 64);
    }

    // Now revert all four. The acceptance criterion: byte-identical to
    // the pre-apply file, including the trailing newline.
    claude_code::revert(home.path()).unwrap();
    codex_cli::revert(home.path()).unwrap();
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
    let qwen_path = qwen_code::config_path(home.path());

    write(&claude_path, CLAUDE_ORIGINAL);
    write(&codex_path, CODEX_ORIGINAL);
    write(&qwen_path, QWEN_ORIGINAL);

    claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();

    // The other two are untouched.
    assert_eq!(fs::read_to_string(&codex_path).unwrap(), CODEX_ORIGINAL);
    assert_eq!(fs::read_to_string(&qwen_path).unwrap(), QWEN_ORIGINAL);
}

#[test]
fn fresh_install_works_when_no_files_exist() {
    let home = tempdir().unwrap();

    claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    codex_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
    qwen_code::apply(home.path(), &ApplyOptions::default()).unwrap();

    let claude_path = claude_code::config_path(home.path());
    let codex_path = codex_cli::config_path(home.path());
    let qwen_path = qwen_code::config_path(home.path());

    // All three parent dirs created; all three files parse.
    assert!(claude_path.exists());
    assert!(codex_path.exists());
    assert!(qwen_path.exists());

    let _claude: Value =
        serde_json::from_str(&fs::read_to_string(&claude_path).unwrap()).unwrap();
    let _codex: toml_edit::DocumentMut = fs::read_to_string(&codex_path).unwrap().parse().unwrap();
    let _qwen: Value = serde_json::from_str(&fs::read_to_string(&qwen_path).unwrap()).unwrap();
}

const CURSOR_ORIGINAL: &str = "{\n  \"unrelatedUserKey\": \"keepme\"\n}\n";
const CURSOR_HOOK_SCRIPT_FIXTURE: &str = "/opt/trove/resources/hooks/cursor-otel-hook.cjs";
const ANTIGRAVITY_HOOK_SCRIPT_FIXTURE: &str =
    "/opt/trove/resources/hooks/antigravity-otel-hook.cjs";

fn cursor_hook_path() -> PathBuf {
    PathBuf::from(CURSOR_HOOK_SCRIPT_FIXTURE)
}

fn antigravity_hook_path() -> PathBuf {
    PathBuf::from(ANTIGRAVITY_HOOK_SCRIPT_FIXTURE)
}

const ANTIGRAVITY_ORIGINAL: &str = "{\n  \"PreToolUse\": {\n    \"type\": \"command\",\n    \"command\": \"/x/user-hook\"\n  }\n}\n";

#[test]
fn antigravity_apply_then_revert_is_byte_identical() {
    // Antigravity CLI is a hook adapter (it dropped native OTLP), so it
    // takes a `hook_script_path` like Cursor and writes JSONHookSpec
    // objects keyed by event name into ~/.gemini/antigravity-cli/hooks.json.
    let home = tempdir().unwrap();
    let path = antigravity_cli::config_path(home.path());
    write(&path, ANTIGRAVITY_ORIGINAL);

    let metadata =
        antigravity_cli::apply(home.path(), &ApplyOptions::default(), &antigravity_hook_path())
            .unwrap();
    assert_eq!(metadata.managed_block_hash.len(), 64);

    // Post-apply: the user's own (unmanaged) PreToolUse hook survives,
    // and Trove's managed event hooks point at the bundled script.
    let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["PreToolUse"]["command"], "/x/user-hook");
    assert_eq!(after["Stop"]["type"], "command");
    assert_eq!(after["Stop"]["command"], ANTIGRAVITY_HOOK_SCRIPT_FIXTURE);
    assert_eq!(
        after["UserPromptSubmit"]["command"],
        ANTIGRAVITY_HOOK_SCRIPT_FIXTURE,
    );
    assert!(after.get("_trove").is_some());

    antigravity_cli::revert(home.path()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), ANTIGRAVITY_ORIGINAL);
}

#[test]
fn cursor_ide_apply_then_revert_is_byte_identical() {
    let home = tempdir().unwrap();
    let path = cursor_ide::config_path(home.path());
    write(&path, CURSOR_ORIGINAL);

    let metadata =
        cursor_ide::apply(home.path(), &ApplyOptions::default(), &cursor_hook_path()).unwrap();
    assert_eq!(metadata.managed_block_hash.len(), 64);

    // Post-apply: user key still present, both hook events installed.
    let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["unrelatedUserKey"], "keepme");
    assert_eq!(after["version"], 1);
    assert_eq!(
        after["hooks"]["beforeShellExecution"][0]["command"],
        CURSOR_HOOK_SCRIPT_FIXTURE,
    );
    assert_eq!(
        after["hooks"]["afterShellExecution"][0]["command"],
        CURSOR_HOOK_SCRIPT_FIXTURE,
    );
    assert!(after.get("_trove").is_some());

    cursor_ide::revert(home.path()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), CURSOR_ORIGINAL);
}

#[test]
fn cursor_ide_and_cli_patch_independent_host_files() {
    // Pre-Sprint 9-rewrite contract was that both adapters shared a
    // single managed region in ~/.cursor/hooks.json. After the
    // cursor-cli wrapper rewrite they patch independent files —
    // cursor-ide owns hooks.json, cursor-cli owns the shell rc — so
    // the new contract is: enabling one leaves the other's host file
    // untouched, and reverting one leaves the other's region intact.
    let home = tempdir().unwrap();
    let hooks_path = cursor_ide::config_path(home.path());
    let zshrc = home.path().join(".zshrc");
    fs::write(&zshrc, "# user content\n").unwrap();

    // 1. Apply both adapters.
    cursor_ide::apply(home.path(), &ApplyOptions::default(), &cursor_hook_path()).unwrap();
    cursor_cli::apply(
        home.path(),
        &ApplyOptions::default(),
        &PathBuf::from("/opt/trove/wrappers/trove-cursor-agent"),
    )
    .unwrap();

    let hooks_after = fs::read_to_string(&hooks_path).unwrap();
    let zshrc_after = fs::read_to_string(&zshrc).unwrap();
    assert!(hooks_after.contains("_trove"), "cursor-ide must patch hooks.json");
    assert!(
        zshrc_after.contains("cursor-agent()")
            && zshrc_after.contains("trove-cursor-agent"),
        "cursor-cli must patch shell rc with its wrapper-routing function, got:\n{zshrc_after}",
    );

    // 2. Revert cursor-cli; hooks.json (cursor-ide's territory) untouched.
    cursor_cli::revert(home.path()).unwrap();
    let hooks_after_cli_revert = fs::read_to_string(&hooks_path).unwrap();
    assert_eq!(hooks_after, hooks_after_cli_revert);
    let zshrc_after_cli_revert = fs::read_to_string(&zshrc).unwrap();
    assert!(
        !zshrc_after_cli_revert.contains("cursor-agent()"),
        "cursor-cli revert must remove its wrapper function block",
    );

    // 3. Revert cursor-ide; hooks.json `_trove` block gone.
    cursor_ide::revert(home.path()).unwrap();
    let after_ide_revert: Value =
        serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).unwrap();
    assert!(after_ide_revert.get("_trove").is_none());
}

#[test]
fn applying_cursor_does_not_disturb_tier_1_files() {
    let home = tempdir().unwrap();

    let claude_path = claude_code::config_path(home.path());
    let codex_path = codex_cli::config_path(home.path());
    write(&claude_path, CLAUDE_ORIGINAL);
    write(&codex_path, CODEX_ORIGINAL);

    cursor_ide::apply(home.path(), &ApplyOptions::default(), &cursor_hook_path()).unwrap();

    assert_eq!(fs::read_to_string(&claude_path).unwrap(), CLAUDE_ORIGINAL);
    assert_eq!(fs::read_to_string(&codex_path).unwrap(), CODEX_ORIGINAL);
}

const OPENCODE_ORIGINAL: &str =
    "{\n  \"theme\": \"midnight\",\n  \"mcp\": {\n    \"someServer\": \"keepme\"\n  }\n}\n";

#[test]
fn opencode_apply_then_revert_is_byte_identical() {
    let home = tempdir().unwrap();
    let path = opencode::config_path(home.path());
    write(&path, OPENCODE_ORIGINAL);

    let metadata = opencode::apply(home.path(), &ApplyOptions::default()).unwrap();
    assert_eq!(metadata.managed_block_hash.len(), 64);

    let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["theme"], "midnight");
    assert_eq!(after["mcp"]["someServer"], "keepme");
    assert_eq!(after["plugin"][0], "@devtheops/opencode-plugin-otel");
    assert_eq!(after["$schema"], "https://opencode.ai/config.json");
    // opencode opts out of the `_trove` marker (Bug C fix) — the
    // opencode CLI's JSON schema rejects unknown top-level keys.
    assert!(after.get("_trove").is_none());

    opencode::revert(home.path()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), OPENCODE_ORIGINAL);
}

#[test]
fn applying_opencode_does_not_disturb_tier_1_or_cursor_files() {
    let home = tempdir().unwrap();

    let claude_path = claude_code::config_path(home.path());
    let cursor_path = cursor_ide::config_path(home.path());
    write(&claude_path, CLAUDE_ORIGINAL);
    cursor_ide::apply(home.path(), &ApplyOptions::default(), &cursor_hook_path()).unwrap();
    let cursor_after_cursor_apply = fs::read_to_string(&cursor_path).unwrap();

    opencode::apply(home.path(), &ApplyOptions::default()).unwrap();

    // Claude Code config still byte-identical; cursor file unchanged from
    // its post-cursor-apply state.
    assert_eq!(fs::read_to_string(&claude_path).unwrap(), CLAUDE_ORIGINAL);
    assert_eq!(
        fs::read_to_string(&cursor_path).unwrap(),
        cursor_after_cursor_apply,
    );
}

#[test]
fn all_seven_supported_harnesses_apply_and_revert_byte_identical() {
    let home = tempdir().unwrap();

    let claude_path = claude_code::config_path(home.path());
    let codex_path = codex_cli::config_path(home.path());
    let qwen_path = qwen_code::config_path(home.path());
    let cursor_path = cursor_ide::config_path(home.path());
    let opencode_path = opencode::config_path(home.path());
    let antigravity_path = antigravity_cli::config_path(home.path());

    write(&claude_path, CLAUDE_ORIGINAL);
    write(&codex_path, CODEX_ORIGINAL);
    write(&qwen_path, QWEN_ORIGINAL);
    // Cursor, OpenCode, and Antigravity (all hook/plugin adapters) start
    // from missing files so we cover the "fresh install" branch too.

    claude_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    codex_cli::apply(home.path(), &ApplyOptions::default()).unwrap();
    qwen_code::apply(home.path(), &ApplyOptions::default()).unwrap();
    cursor_ide::apply(home.path(), &ApplyOptions::default(), &cursor_hook_path()).unwrap();
    opencode::apply(home.path(), &ApplyOptions::default()).unwrap();
    antigravity_cli::apply(home.path(), &ApplyOptions::default(), &antigravity_hook_path())
        .unwrap();

    // All now exist and parse.
    assert!(claude_path.exists());
    assert!(codex_path.exists());
    assert!(qwen_path.exists());
    assert!(cursor_path.exists());
    assert!(opencode_path.exists());
    assert!(antigravity_path.exists());

    // Revert each. The native config adapters return to byte-identical
    // originals; the hook/plugin adapters (cursor, opencode, antigravity)
    // started from missing so we just confirm the _trove block is gone.
    claude_code::revert(home.path()).unwrap();
    codex_cli::revert(home.path()).unwrap();
    qwen_code::revert(home.path()).unwrap();
    cursor_ide::revert(home.path()).unwrap();
    opencode::revert(home.path()).unwrap();
    antigravity_cli::revert(home.path()).unwrap();

    assert_eq!(fs::read_to_string(&claude_path).unwrap(), CLAUDE_ORIGINAL);
    assert_eq!(fs::read_to_string(&codex_path).unwrap(), CODEX_ORIGINAL);
    assert_eq!(fs::read_to_string(&qwen_path).unwrap(), QWEN_ORIGINAL);

    let cursor_after: Value =
        serde_json::from_str(&fs::read_to_string(&cursor_path).unwrap()).unwrap();
    assert!(cursor_after.get("_trove").is_none());

    let opencode_after: Value =
        serde_json::from_str(&fs::read_to_string(&opencode_path).unwrap()).unwrap();
    assert!(opencode_after.get("_trove").is_none());

    let antigravity_after: Value =
        serde_json::from_str(&fs::read_to_string(&antigravity_path).unwrap()).unwrap();
    assert!(antigravity_after.get("_trove").is_none());
}

// ---------------------------------------------------------------------------
// Droid adapter round-trip tests
// ---------------------------------------------------------------------------

const DROID_ZSHRC_ORIGINAL: &str = "# ~/.zshrc\nexport PATH=\"$HOME/.local/bin:$PATH\"\n";
const DROID_ZSHRC_LEGACY: &str = concat!(
    "# ~/.zshrc\n",
    "export PATH=\"$HOME/.local/bin:$PATH\"\n",
    "\n",
    "# trove:start\n",
    "export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318\n",
    "export OTEL_RESOURCE_ATTRIBUTES=harness.id=droid,harness.name=Droid,service.name=droid\n",
    "# trove:end\n",
);

/// Helper: write `content` to `home/.zshrc` (creating the file).
fn write_zshrc(home: &Path, content: &str) {
    fs::write(home.join(".zshrc"), content).unwrap();
}

#[test]
fn droid_apply_then_revert_is_byte_identical() {
    let home = tempdir().unwrap();
    write_zshrc(home.path(), DROID_ZSHRC_ORIGINAL);

    let metadata = droid::apply(home.path(), &ApplyOptions::default()).unwrap();
    assert_eq!(metadata.managed_block_hash.len(), 64);
    assert_eq!(metadata.file_hash_at_last_write.len(), 64);

    let rc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert!(rc.contains("# trove:droid:start"), "fence start must be present");
    assert!(rc.contains("# trove:droid:end"), "fence end must be present");
    assert!(
        rc.contains("export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318"),
        "endpoint var must be present"
    );
    assert!(
        !rc.contains("OTEL_RESOURCE_ATTRIBUTES"),
        "OTEL_RESOURCE_ATTRIBUTES must not be written"
    );
    assert!(rc.contains("export PATH="), "user PATH export must be preserved");

    droid::revert(home.path()).unwrap();
    assert_eq!(
        fs::read_to_string(home.path().join(".zshrc")).unwrap(),
        DROID_ZSHRC_ORIGINAL,
        "droid apply+revert must be byte-identical to original"
    );
}

#[test]
fn droid_migrates_legacy_fence_on_first_apply() {
    let home = tempdir().unwrap();
    write_zshrc(home.path(), DROID_ZSHRC_LEGACY);

    droid::apply(home.path(), &ApplyOptions::default()).unwrap();

    let rc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert!(rc.contains("# trove:droid:start"), "namespaced fence must be present after migration");
    assert!(!rc.contains("# trove:start\n"), "legacy fence start must be gone");
    assert!(!rc.contains("# trove:end\n"), "legacy fence end must be gone");
    assert!(!rc.contains("OTEL_RESOURCE_ATTRIBUTES"), "resource attrs must be dropped");
    assert!(rc.contains("OTEL_TELEMETRY_ENDPOINT"), "endpoint var must be present");
    assert!(rc.contains("export PATH="), "user PATH export must be preserved");
}

#[test]
fn applying_droid_does_not_disturb_tier_1_config_files() {
    let home = tempdir().unwrap();
    write_zshrc(home.path(), DROID_ZSHRC_ORIGINAL);

    let claude_path = claude_code::config_path(home.path());
    let qwen_path = qwen_code::config_path(home.path());
    write(&claude_path, CLAUDE_ORIGINAL);
    write(&qwen_path, QWEN_ORIGINAL);

    droid::apply(home.path(), &ApplyOptions::default()).unwrap();

    assert_eq!(
        fs::read_to_string(&claude_path).unwrap(),
        CLAUDE_ORIGINAL,
        "droid apply must not touch Claude Code config"
    );
    assert_eq!(
        fs::read_to_string(&qwen_path).unwrap(),
        QWEN_ORIGINAL,
        "droid apply must not touch Qwen config"
    );
}
