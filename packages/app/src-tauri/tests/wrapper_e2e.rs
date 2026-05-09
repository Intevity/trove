//! End-to-end test of the `trove-aider` shell wrapper.
//!
//! Sprint 9 PR 3 acceptance: "integration test that runs the wrapper
//! against a captured-stdout fixture and asserts the emitted OTLP
//! shape." We:
//!
//! 1. Plant a stub `aider` shell script in a tempdir that emits a known
//!    line and exits with a controlled status.
//! 2. Plant a tempdir-scoped state dir via the `TROVE_STATE_DIR` env
//!    var the wrapper honours.
//! 3. Run the bundled `resources/wrappers/trove-aider` with PATH
//!    pointing at the stub dir.
//! 4. Read the wrapper's emitted log file and call
//!    `trove_app::adapters::aider::parse_event_line` on each line.
//! 5. Assert the resulting OTLP `LogRecord` carries the expected
//!    attributes and that the wrapper exited with the stub's exit
//!    code.
//!
//! Skipped on Windows where `bash` is not guaranteed.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use trove_app::adapters::ApplyOptions;
use trove_app::adapters::aider;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at packages/app/src-tauri at test time;
    // the wrappers live three directories up under resources/wrappers.
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn make_executable(path: &std::path::Path) {
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

#[test]
fn trove_aider_wrapper_writes_a_parseable_event_line() {
    let wrapper = workspace_root()
        .join("resources")
        .join("wrappers")
        .join("trove-aider");
    assert!(
        wrapper.exists(),
        "expected bundled wrapper at {}",
        wrapper.display()
    );

    // Stub PATH dir hosting a fake `aider` binary that exits 7.
    let path_dir = tempfile::tempdir().unwrap();
    let stub = path_dir.path().join("aider");
    std::fs::write(&stub, b"#!/bin/sh\necho 'fake aider invoked'\nexit 7\n").unwrap();
    make_executable(&stub);

    // Tempdir-scoped state dir so the wrapper writes its log here
    // instead of polluting the developer's $HOME.
    let state_dir = tempfile::tempdir().unwrap();

    // Prepend the stub dir to a minimal system PATH so /usr/bin/env
    // can still find bash + coreutils. The wrapper's loop iterates
    // PATH in order and uses the first `aider` it finds that isn't
    // itself, so the stub still wins.
    let path = format!("{}:/usr/bin:/bin", path_dir.path().display());
    let output = Command::new(&wrapper)
        .arg("--help")
        .arg("file.py")
        .env("PATH", &path)
        .env("TROVE_STATE_DIR", state_dir.path())
        .output()
        .expect("wrapper subprocess");

    // The wrapper must exit with the stub's exit code.
    assert_eq!(
        output.status.code(),
        Some(7),
        "wrapper should exit with the stub's exit code; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The stub's stdout passes through unchanged.
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("fake aider invoked"),
        "expected pass-through of stub stdout"
    );

    // The wrapper writes one JSON line per invocation to aider.log.
    let log = state_dir.path().join("aider.log");
    let log_text = std::fs::read_to_string(&log)
        .unwrap_or_else(|_| panic!("expected log file at {}", log.display()));
    assert!(!log_text.trim().is_empty(), "log file should not be empty");

    // Parse the emitted line through the adapter's parser and assert
    // the OTLP shape.
    let line = log_text.lines().next().expect("at least one log line");
    let payload = aider::parse_event_line(line, &ApplyOptions::default())
        .expect("parser must return a payload for a real wrapper line");

    let log_record = &payload["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
    let attrs = log_record["attributes"].as_array().unwrap();
    let by_key = |k: &str| {
        attrs
            .iter()
            .find(|a| a["key"] == k)
            .map(|a| {
                a["value"]["stringValue"]
                    .as_str()
                    .or_else(|| a["value"]["intValue"].as_str())
                    .unwrap()
                    .to_string()
            })
            .unwrap()
    };
    assert_eq!(by_key("trove.source"), "aider");
    assert_eq!(
        by_key("aider.exit_code"),
        "7",
        "exit_code should match the stub's"
    );
    assert_eq!(
        by_key("aider.argc"),
        "2",
        "argc should match the wrapper's argv (--help + file.py)"
    );
}
