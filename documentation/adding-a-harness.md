# Adding a new harness adapter

Trove patches each AI-coding harness's config file so it emits OTLP to
Trove's local Collector. An _adapter_ is the bit of code that knows
**where** the harness's config lives, **what shape** it's in, and **what
keys** Trove needs to write. Everything else — atomicity, backup,
idempotence, conflict detection, revert — is shared.

This page walks a fresh contributor through adding a hypothetical
harness called **`acme-cli`** (binary `acme`, JSON config at
`~/.acme/settings.json`) end to end. Substitute your own harness's
details as you go.

## Contract every adapter satisfies

Each adapter must:

- **Atomic write** the config — temp file, fsync, rename. Provided by
  `crate::safety::atomic::write_atomic`; you don't call it directly.
- **Back up** the file before the first edit, with retention pruned to
  N most recent. Provided by `crate::safety::backup`.
- **Sentinel-bracket** the managed region so revert can find and
  remove exactly what Trove wrote, byte-identically. Provided by
  `crate::safety::sentinels`.
- Be **idempotent** — re-applying with the same `ApplyOptions` is a
  no-op.
- Be **cleanly revertible** — `revert` followed by `apply`'s pre-state
  is byte-identical.
- **Refuse**, not silently overwrite, when the user has hand-edited
  inside the managed block. (Sprint 8 replaces the refusal with a
  three-way merge UI.)

The shared `adapters::common` module enforces all of these. Your
adapter file should be a thin wrapper that declares **where** to
patch and **what** to write.

## Prerequisites

1. Pick a `HarnessId` slug — kebab-case, e.g. `"acme-cli"`.
2. Decide on a config format: `Json`, `Jsonc`, `Toml`, or `Yaml`. The
   sentinel engine handles all four.
3. Confirm the host config path. `$HOME`-relative paths are
   strongly preferred — XDG fallbacks are fine but should be a
   secondary search path, not the primary.
4. Read the upstream harness's docs to confirm the exact env-var or
   config-key names you'll write. Cite the doc URL in your adapter's
   module-level doc-comment.

## Step 1 — Register the `HarnessId`

Add the slug in two places:

- `packages/shared/src/schemas.ts` — append to the `HarnessId` Zod
  enum. The TS side must agree with the Rust side or IPC fails.
- `packages/app/src-tauri/src/harness.rs` — append to the
  `HarnessId` enum and add the variant to `HarnessId::tier_1()` (or
  the future tier-2 / tier-3 helpers when those land).

```rust
#[serde(rename_all = "kebab-case")]
pub enum HarnessId {
    ClaudeCode,
    CodexCli,
    AntigravityCli,
    QwenCode,
    AcmeCli,        // ← new
    // ...
}
```

## Step 2 — Register detection paths

Two files in `packages/app/src-tauri/src/detect/`:

- `paths.rs::config_search_paths` — the home-relative path(s) to
  check. List the canonical path first; XDG-style fallbacks after.
- `harnesses.rs`:
  - `path_binary_name` — the binary on `PATH` (or `None` if there
    isn't one).
  - `read_trove_region_present` — add the harness's `Format` to the
    match.
  - `read_telemetry` and a per-harness `check_*_telemetry` function
    that decides `On` / `Off` / `Unknown` from the user's current
    config. Be conservative: parse failure → `Unknown`. The fence
    `# trove:start` is always a valid `On` signal because Trove only
    writes the fence when it has installed an exporter.

Add a unit test for each new arm of `check_*_telemetry` covering On,
Off, and Unknown — see the existing `codex_telemetry_*` tests in
`detect/harnesses.rs` for the shape.

## Step 3 — Write the adapter

Each adapter declares a `const SPEC: HarnessSpec` and delegates the
public API to `common`. The only function that must live in the
per-harness module is `build_region`, which turns `ApplyOptions` into
the `ManagedRegion` Trove will install.

Create `packages/app/src-tauri/src/adapters/acme_cli.rs`:

```rust
//! Acme CLI adapter — patches `~/.acme/settings.json`'s `telemetry`
//! object. Schema source: <link to upstream docs>.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ipc::IpcError;
use crate::safety::sentinels::{Format, ManagedRegion, SentinelError};

use super::common::{self, HarnessSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

const SPEC: HarnessSpec = HarnessSpec {
    config_dir: ".acme",
    config_file: "settings.json",
    format: Format::Json,
    build_region,
};

#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    common::config_path(&SPEC, home)
}

pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    common::preview(&SPEC, home, opts)
}

pub fn apply(home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    common::apply(&SPEC, home, opts)
}

pub fn revert(home: &Path) -> Result<(), IpcError> {
    common::revert(&SPEC, home)
}

/// Build the managed region. Edit *only* this function when the
/// upstream schema changes — everything else is boilerplate.
fn build_region(opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    let mut telemetry = serde_json::Map::new();
    telemetry.insert("enabled".to_string(), Value::Bool(true));
    telemetry.insert(
        "endpoint".to_string(),
        Value::String("http://127.0.0.1:4318".to_string()),
    );
    telemetry.insert("logPrompts".to_string(), Value::Bool(opts.log_user_prompts));

    let mut top = serde_json::Map::new();
    top.insert("telemetry".to_string(), Value::Object(telemetry));
    ManagedRegion::for_json_patches(&top)
}
```

Then register the module in `adapters/mod.rs`:

```rust
pub mod acme_cli;     // ← new, alphabetical
pub mod claude_code;
pub mod codex_cli;
// ...
```

### Format-specific notes

- **JSON / JSONC** — `build_region` returns
  `ManagedRegion::for_json_patches(top)` where `top` is the leaf-path
  patches you want merged into the host document. The sentinel engine
  records the leaf paths in a `_trove` block so revert deletes
  exactly what you wrote.
- **TOML / YAML** — `build_region` returns
  `ManagedRegion::for_text_block(payload, keys)` where `payload` is
  the literal text placed between `# trove:start` / `# trove:end`
  fences. Make the text deterministic so the hash is stable across
  re-runs. See `adapters/codex_cli.rs::build_region` for a TOML
  example, including the trick of skipping the bare `[ns]` table to
  avoid colliding with a user-written one.
- **`deny_unknown_fields`** — if the upstream schema is strict about
  unknown keys (codex-rs is), emit only documented fields. Document
  any `customAttributes` no-op behaviour in the doc-comment.

## Step 4 — Wire IPC dispatch

Add three match arms in `packages/app/src-tauri/src/ipc/commands.rs` —
one each in `preview_patch`, `apply_patch`, `revert_patch`:

```rust
HarnessId::AcmeCli => acme_cli::preview(&home, &options),
HarnessId::AcmeCli => acme_cli::apply(&home, &options),
HarnessId::AcmeCli => acme_cli::revert(&home),
```

Update the import to include `acme_cli`. If the
`apply_patch_for_unimplemented_harness_returns_not_implemented` test
or its preview/revert siblings target your harness, retarget them to
a still-unimplemented harness from a later tier (e.g. `Cline`,
`Aider`).

## Step 5 — Write the seven-case test suite

Inline `#[cfg(test)] mod tests` at the bottom of your adapter file.
Use `tempfile::tempdir()` to scope each test to an isolated `$HOME`.
The seven cases the safety contract requires:

1. **Fresh install** — empty $HOME, `apply` produces a parseable file
   with the expected keys + sentinel block.
2. **Idempotent re-apply** — second `apply` with the same options
   produces a byte-identical file.
3. **User-edited outside block** — pre-populate unrelated user keys,
   `apply`, assert user keys survive and Trove's keys are added.
4. **User-edited inside block** — `apply`, tamper with a managed
   value, second `apply` returns `IpcError::RegionConflict` and
   leaves the file untouched.
5. **Malformed file** — write garbage, `apply` returns
   `IpcError::ConfigUnparseable`, file untouched.
6. **Missing parent dir** — empty $HOME with no harness directory,
   `apply` creates the parent automatically.
7. **Read-only parent dir** (`#[cfg(unix)]`) — chmod 0o555 on the
   parent, `apply` returns `IpcError::Io`. Restore permissions
   before the assertion so `tempdir` cleanup works.

Plus the supplementary cases:

- `revert_restores_byte_identical_pre_apply_file` — load-bearing
  byte-identity assertion, including the trailing newline.
- `revert_on_missing_file_is_noop`
- `revert_when_no_trove_block_is_noop`
- `revert_on_malformed_file_returns_unparseable_error`
- `preview_on_missing_file_returns_fresh_status`
- `preview_after_apply_returns_idempotent_status`
- `preview_with_tampered_block_returns_conflict_status`
- `log_user_prompts_propagates_to_*_when_true`
- `changing_options_between_applies_yields_conflict_until_reverted`

See `adapters/claude_code.rs` (JSON) or `adapters/codex_cli.rs`
(TOML) for the full template.

## Step 6 — Extend the cross-harness integration test

`packages/app/src-tauri/tests/adapters_roundtrip.rs` covers all Tier 1
adapters in one round-trip:

- Add a `const ACME_ORIGINAL: &str` fixture with realistic user keys.
- Add `acme_cli::config_path` / `apply` / `revert` calls to
  `all_four_tier_1_adapters_apply_and_revert_byte_identical` (rename
  the test as needed).
- Add an assertion that `ACME_ORIGINAL` is preserved byte-identically
  post-revert (including the trailing newline).
- Extend `applying_one_adapter_does_not_disturb_the_other_files` and
  `fresh_install_works_when_no_files_exist` to cover the new adapter.

## Step 7 — Add the harness to its tier

Update `harness.rs::tier_1()` (or future `tier_2()` / `tier_3()`) so
the React UI lists it and the IPC's
`list_detected_harnesses_returns_a_row_per_tier_*_harness` test
passes.

## Run the suite

```bash
# Per-harness unit + integration tests
cargo test --manifest-path packages/app/src-tauri/Cargo.toml

# Cross-harness round-trip
cargo test --manifest-path packages/app/src-tauri/Cargo.toml \
  --test adapters_roundtrip -- --nocapture

# Coverage gate (matches CI)
cargo llvm-cov \
  --manifest-path packages/app/src-tauri/Cargo.toml \
  --workspace \
  --ignore-filename-regex '(main\.rs|lib\.rs|tray\.rs|collector/)' \
  --fail-under-lines 85 \
  --fail-under-functions 80

# Lint cleanliness
cargo clippy --manifest-path packages/app/src-tauri/Cargo.toml \
  --all-targets -- -D warnings
pnpm lint && pnpm format:check && pnpm typecheck
```

The CI gate at `.github/workflows/ci.yml` enforces the same
thresholds. Aim for ~95% coverage on the new adapter file via the
seven-case suite plus the supplementaries above.

## Reusable utilities (don't reimplement)

- `crate::safety::atomic::write_atomic` — temp + fsync + rename +
  permissions preservation.
- `crate::safety::backup::{backup_file, prune_backups}` — timestamped
  backups; `BACKUPS_TO_KEEP = 10` constant in `adapters/mod.rs`.
- `crate::safety::sentinels::{upsert_region, remove_region,
extract_region}` — all four formats, idempotent on the same input.
- `crate::adapters::common::{config_path, preview, apply, revert}` —
  parameterized over `HarnessSpec`; this is what your adapter
  delegates to.
- `crate::ipc::IpcError::{Io, ConfigUnparseable, RegionConflict,
Internal, HarnessNotImplemented}` — covers every error your
  adapter should surface.

## Adapter template

Copy this file into `packages/app/src-tauri/src/adapters/<id>.rs` and
fill in the four marked sections.

```rust
//! TODO: <one-line description>. Schema source: <upstream docs URL>.

use std::path::{Path, PathBuf};

use crate::ipc::IpcError;
use crate::safety::sentinels::{Format, ManagedRegion, SentinelError};

use super::common::{self, HarnessSpec};
use super::{ApplyOptions, PatchPreview, TrovePatch};

const SPEC: HarnessSpec = HarnessSpec {
    config_dir: "TODO",                        // ← e.g. ".acme"
    config_file: "TODO",                       // ← e.g. "settings.json"
    format: Format::Json,                      // ← Json | Jsonc | Toml | Yaml
    build_region,
};

#[must_use]
pub fn config_path(home: &Path) -> PathBuf {
    common::config_path(&SPEC, home)
}

pub fn preview(home: &Path, opts: &ApplyOptions) -> Result<PatchPreview, IpcError> {
    common::preview(&SPEC, home, opts)
}

pub fn apply(home: &Path, opts: &ApplyOptions) -> Result<TrovePatch, IpcError> {
    common::apply(&SPEC, home, opts)
}

pub fn revert(home: &Path) -> Result<(), IpcError> {
    common::revert(&SPEC, home)
}

fn build_region(_opts: &ApplyOptions) -> Result<ManagedRegion, SentinelError> {
    todo!("populate the harness's telemetry block from opts");
}

#[cfg(test)]
mod tests {
    // TODO: seven-case suite + revert/preview supplementaries.
}
```

## Plugin / hook style adapters (Tier 2)

Tier 1 harnesses each have a native OTLP-toggle: Trove flips a flag in
their config file and they emit telemetry directly. Tier 2 harnesses
(Cursor IDE, Cursor CLI, OpenCode) don't. Instead they expose a hook or
plugin surface where Trove provides a small piece of code that
listens to the harness's events and forwards OTLP HTTP/JSON to the
local Collector. Two patterns are in tree as of Sprint 7:

### Pattern A — bundled hook script (Cursor)

The two cursor adapters share `adapters/cursor_common.rs`, which writes
a managed region into `~/.cursor/hooks.json` referencing a vendored
Node script (`resources/hooks/cursor-otel-hook.cjs`) shipped with the
app. The bundled file is registered in `tauri.conf.json` under
`bundle.resources` and resolved at runtime via
`tauri::path::PathResolver::resource_dir()`.

What's different from the Tier 1 archetype:

- **Region payload depends on runtime context** (the absolute path of
  the bundled script, which only the Tauri layer can resolve). Static
  fn-pointers can't capture environment, so the SPEC's `build_region`
  field is a panic-loud placeholder; the adapter owns its own
  `apply` / `preview` that build the region with the runtime path and
  call `common::apply_with_region` / `common::preview_with_region`
  directly.
- **Two HarnessIds, one host file**. Cursor IDE and Cursor CLI both
  patch `~/.cursor/hooks.json`. They're two separate adapters so the
  UI can offer per-harness Enable / Disable toggles (and surface a
  "partial event coverage" advisory only on the CLI row), but they
  share a single managed region. Re-applying via the second adapter
  is a no-op because the canonical region hash is identical.
- **IPC dispatch passes `app: tauri::AppHandle`** so the runtime path
  can be resolved. `preview_patch_inner` is factored out as a free
  function taking a hook-resolver closure so unit tests can exercise
  the dispatch without synthesising an `AppHandle`.

If you're adding another bundled-hook-style harness, the two cursor
adapter files plus `cursor_common.rs` are the canonical reference.

### Pattern B — npm-resolved plugin (OpenCode)

The OpenCode adapter (`adapters/opencode.rs`) is a plain JSON-merge
SPEC writing two leaves into `~/.config/opencode/opencode.json`:

- `$schema` for editor autocomplete.
- `plugin: ["@devtheops/opencode-plugin-otel"]` — OpenCode's Bun
  runtime resolves and installs the package itself at next launch.

Trove ships no vendored source for the plugin. The original Sprint 7
plan called for vendoring, but the upstream package has 14
OpenTelemetry / OpenCode SDK runtime dependencies that need ~100 MB of
`node_modules` to actually run, and OpenCode's runtime doesn't bundle
them. The npm-reference path lands a working integration with a much
narrower surface and matches the upstream README's install instructions.

What stays normal: the adapter is shape-identical to the Tier 1
JSON-merge adapters (`claude_code.rs` / `qwen_code.rs`). The seven
golden cases apply unchanged.

### When to use which pattern

- **Bundled hook**: harness expects an executable / script path in its
  config and invokes it as a subprocess for each event. Cursor's
  hook surface is the canonical example.
- **Npm-resolved plugin**: harness ships its own runtime that loads
  plugins from npm by name. OpenCode is the canonical example.
- **Vendored source + dependencies**: only consider this if the host
  harness's runtime can directly load a vendored TS/JS file with no
  external deps. Even then, weigh the bundle-size cost — Trove's
  binary target is 30–50 MB, and a vendored plugin tree's
  `node_modules` will dominate it.

## Where to look for inspiration

- **JSON merge** — `adapters/claude_code.rs` (env block) or
  `adapters/qwen_code.rs` (telemetry object).
- **TOML fenced block** — `adapters/codex_cli.rs`.
- **Bundled hook + shared writer (Tier 2)** —
  `adapters/cursor_common.rs` + `cursor_ide.rs` + `cursor_cli.rs`, or
  `adapters/antigravity_cli.rs` (Antigravity CLI's hooks.json bridge).
- **Npm-resolved plugin (Tier 2)** — `adapters/opencode.rs`.
- **Shared scaffolding** — `adapters/common.rs` (read it once;
  understanding `HarnessSpec` and `working_value` makes everything
  else obvious; for plugin-style adapters also read
  `apply_with_region` / `preview_with_region`).
- **Cross-harness end-to-end** — `tests/adapters_roundtrip.rs`.

## Tier 3 — best-effort (no native OTLP, no clean hook surface)

Tier 3 ships when the upstream tool exposes neither native OTLP nor a
plugin/hook surface Trove can wedge into. There are two patterns
Sprint 9 established; pick whichever matches your harness:

### Pattern A — log-watch only (Cline)

When the tool already writes structured records to a known location:

1. Add a `<tool>_data_root(home: &Path) -> PathBuf` style helper that
   returns the tool's data dir.
2. Write a polling watcher (e.g. `adapters/cline_watcher.rs`) that
   tails files under that dir, hashes contents to detect changes,
   and emits one OTLP `LogRecord` per change via
   `crate::otlp_emit::post_logs_json`.
3. The adapter's `preview` / `apply` / `revert` are _synthetic_ — no
   host-file region:
   - `preview` returns `PreviewStatus::Fresh` with a descriptive
     `after` string (the dashboard renders this as the "what will
     happen" diff).
   - `apply` returns a `TrovePatch` whose
     `last_written_region_payload` is a small JSON snapshot of the
     user's `ApplyOptions`. The IPC layer also spawns the watcher
     and registers it in `TierThreeWatchers`.
   - `revert` is a filesystem no-op; the IPC layer aborts the
     watcher and removes the `state.json` row.
4. Detection lives in `detect/paths.rs`: probe the tool's data dir
   and / or its VSCode extension folder.

### Pattern B — shell-rc wrapper (Aider, Copilot CLI)

When the tool runs from a terminal command and we can intercept
invocations through a shell function:

1. Add a vendored wrapper script under `resources/wrappers/` (POSIX
   shell). The wrapper runs the real binary, captures `argc`,
   `exit_code`, `duration_ms`, and writes one JSON-line per
   invocation to `~/.local/state/trove/<tool>.log` (overrideable
   via `TROVE_STATE_DIR` for tests).
2. Register the script in `tauri.conf.json` under
   `bundle.resources` so Tauri stages it in the app's
   `resource_dir()` at install time.
3. The adapter delegates host-file work to
   `adapters/wrapper_common::{apply_to_primary_shell_rc, revert_primary_shell_rc, preview_for_primary_shell_rc}`.
   It only owns the `WrapperSpec` (function name, wrapper path,
   label) and the OTLP parser that turns one log line into one
   OTLP `LogRecord`-shaped JSON value.
4. Detection: probe the underlying tool's binary on PATH (`aider`,
   `gh`).
5. When you need to avoid shadowing a multi-purpose binary (e.g.
   `gh` powers `gh repo`, `gh pr`, etc.), name your shell function
   distinctly (`gh-copilot`) and surface the rename in the row's
   `COVERAGE_NOTES` tooltip.

In both patterns:

- `HarnessId::has_adapter()` flips on once the adapter is registered.
- The `adapter_available` field on `DetectedHarness` drives the
  Enable / Disable button — no parallel hard-coded list on the TS
  side.
- The IPC layer (`commands::spawn_tier3_watcher` and
  `revert_patch`'s abort branch) handles watcher lifecycle. Adapter
  authors don't manage tokio tasks directly.
- Add an integration test under `tests/` that exercises the
  end-to-end signal path: synthetic data → parser → OTLP shape
  assertions. See `tests/cline_e2e.rs` and `tests/wrapper_e2e.rs`
  for templates.

## When the upstream schema changes

The nightly CI run (Sprint 11) re-runs every adapter's golden-file
suite against the latest published version of each harness. When it
fails, file an issue tagged `adapter-regression`, update the
relevant adapter's `build_region` and tests in the same PR, and bump
the version pin if the upstream introduced one. If the schema
changed in a way Trove can't represent today (a new exporter kind,
say), surface the limitation in the UI before the user enables the
adapter — never silently emit a config the harness will reject.
