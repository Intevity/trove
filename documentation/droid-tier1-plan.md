# Plan: Droid Tier 1 Harness

## Context

Droid (factory.ai CLI) has been detection-only since it was first wired into Trove.
The goal is to promote it to a full Tier 1 adapter that writes `export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318` into the user's primary shell RC file so Droid emits OTLP to Trove's local collector.

factory.ai's SDK has a critical quirk: it **ignores `OTEL_RESOURCE_ATTRIBUTES`** and hardcodes `service.name=cli` — too generic to use for filtering. All Droid metrics use the `droid.*` namespace, which is the only reliable discriminator. The collector codegen therefore uses metric-name prefix matching instead of resource-attribute matching for filtering and tagging.

---

## Quirks doc verification

`documentation/droid-otlp-sdk-quirks.md` was written from a live debugging session in May 2026. All findings are confirmed against factory.ai's public documentation and your actual machine state:

| Finding                                                                               | Status                                                                          |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `OTEL_TELEMETRY_ENDPOINT` is the env var (not `OTEL_EXPORTER_OTLP_ENDPOINT`)          | ✅ Confirmed via factory.ai docs                                                |
| SDK ignores `OTEL_RESOURCE_ATTRIBUTES`                                                | ✅ Likely correct — see note below                                              |
| Metrics only — no traces, no logs                                                     | ✅ Confirmed via factory.ai docs                                                |
| All metrics use `droid.*` namespace                                                   | ✅ Confirmed via factory.ai docs                                                |
| `service.name` is set by the SDK to `"cli"` (not from `OTEL_RESOURCE_ATTRIBUTES`)     | ⚠️ Newly confirmed — `service.name=cli` arrives but is too generic to filter on |
| Only 3 metric names listed (`tool.invocations`, `tool.execution_time`, `git.commits`) | ⚠️ Incomplete — full list is 13 metrics (see mapping section)                   |

**`OTEL_RESOURCE_ATTRIBUTES` — confidence assessment**

Your machine's `~/.zshrc` currently has _both_ env vars set:

```sh
export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318
export OTEL_RESOURCE_ATTRIBUTES=harness.id=droid,harness.name=Droid,service.name=droid
```

The quirks doc says the env var is present in the process (confirmed via `/proc/<pid>/environ`) but the attributes never arrive in OTLP payloads. This is consistent with:

1. factory.ai uses a **custom env var** (`OTEL_TELEMETRY_ENDPOINT`, not the OTel standard `OTEL_EXPORTER_OTLP_ENDPOINT`) — a strong signal their SDK bypasses OTel autoconfigure
2. factory.ai docs list **zero** configurable resource attributes — if `OTEL_RESOURCE_ATTRIBUTES` worked, it would be documented
3. The standard OTel spec requires SDKs to honor it, but only the autoconfigure layer actually reads it; raw SDK init does not (this is a known Java SDK pattern, issue #5238)

**The conclusion from the quirks doc is well-supported.** The `scripts/otlp-tap.py` live capture test in the verification section is the definitive check — run it before shipping if you want a direct payload dump as evidence.

**Design consequence**: the new adapter writes **only** `OTEL_TELEMETRY_ENDPOINT`. Writing `OTEL_RESOURCE_ATTRIBUTES` is harmless but wasteful if the SDK ignores it — and if a future SDK version does honor it, the collector's `transform/harness-tag` processor handles the tagging idempotently anyway, so there's no correctness benefit to writing it now.

The doc needs to be **committed to the worktree** (it currently exists only in the main working tree).

---

## Installation state on user's machine

- Droid binary: `/usr/bin/droid` ✅
- Config: `~/.factory/settings.json` (not `~/.droid` — that directory does not exist)
- Shell RC: `~/.zshrc` has a legacy un-namespaced `# trove:start` block:
  ```sh
  # trove:start
  export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318
  export OTEL_RESOURCE_ATTRIBUTES=harness.id=droid,harness.name=Droid,service.name=droid
  # trove:end
  ```
  This was written by an earlier incomplete implementation and must be **migrated** to the per-adapter namespaced fence on first apply. The `OTEL_RESOURCE_ATTRIBUTES` line will be dropped (not written by the new adapter).

---

## Droid metric → Tier A mapping

Full list from factory.ai docs, mapped conservatively (no double-count defaults):

| Droid metric                      | Type          | Default mapping                                                        |
| --------------------------------- | ------------- | ---------------------------------------------------------------------- |
| `droid.tool.invocations`          | Counter       | `events(event.kind=tool.call)`                                         |
| `droid.tool.execution_time`       | Histogram (s) | `turn.duration(event.kind=tool.call)`                                  |
| `droid.code.files_modified`       | Counter       | `events(event.kind=file.edit)`                                         |
| `droid.slash_command.invocations` | Counter       | `events(event.kind=chat.turn)`                                         |
| `droid.mcp.tool_invocations`      | Counter       | ❌ Off by default — likely double-counts with `droid.tool.invocations` |
| `droid.skill.invocations`         | Counter       | ❌ Off by default — overlap with tool.invocations unclear              |
| `droid.hook.invocations`          | Counter       | ❌ Off by default — internal hook system                               |
| `droid.code.files_read`           | Counter       | ❌ No Tier A equivalent                                                |
| `droid.code.lines_modified`       | Counter       | ❌ No Tier A equivalent                                                |
| `droid.git.commits`               | Counter       | ❌ Off by default — overlaps semantically with files_modified          |
| `droid.git.pull_requests`         | Counter       | ❌ Off by default — no direct Tier A equivalent                        |
| `droid.auth.login_success`        | Counter       | ❌ No Tier A equivalent                                                |
| `droid.repo.metadata`             | Gauge         | ❌ No Tier A equivalent                                                |

**Token data gap:** Droid's OTLP output contains no token or cost metrics. However, `~/.factory/logs/droid-log-single.log` contains per-API-call `[Agent] Streaming result` entries with rich token data (see Phase 2 below).

---

## Phase 2: Token data via log watcher

`~/.factory/logs/droid-log-single.log` is a single-file rolling log. Each `[Agent] Streaming result` line is a JSON payload with per-API-call token data:

```json
{
  "count": 3,
  "cacheReadInputTokens": 226602,
  "contextCount": 564,
  "outputTokens": 2039,
  "reason": "tool-calls",
  "tags": {
    "modelId": "claude-opus-4-6",
    "sessionId": "d4d06660-...",
    "droidMode": "interactive-cli",
    "version": "0.133.0"
  }
}
```

Field mapping to Tier A:
| Log field | Tier A target | Notes |
|---|---|---|
| `cacheReadInputTokens` | `trove.harness.tokens{token.type=cache_read}` | Cheaper read-cache hits |
| `contextCount` | `trove.harness.tokens{token.type=input}` | Full-price input tokens |
| `outputTokens` | `trove.harness.tokens{token.type=output}` | Output tokens |
| `tags.modelId` | resource attribute `model.id` | For per-model cost lookup |
| `tags.sessionId` | carry-through for dedup | Skip events already processed |

**Cost**: With `modelId` present, cost can be computed via Trove's existing pricing table lookup.  
**Pattern**: Follows `gemini_watcher.rs` — tail the log file, parse JSON lines, emit OTLP counters via the internal collector endpoint.  
**New file**: `adapters/droid_watcher.rs` (or `watchers/droid.rs`, whichever matches the Gemini file's location in `src/`)  
**IPC**: `spawn_tier3_watcher` currently has Droid in the `None` arm; Phase 2 moves it to a `Some(spawn_droid_watcher(...))` arm.

Phase 2 is intentionally scoped out of this PR. The Tier 1 OTLP adapter is independently shippable. The watcher can be added in a follow-up once we confirm the log path is stable across Droid versions.

---

## Files to change

### 1. `documentation/droid-otlp-sdk-quirks.md`

Copy the file from the main working tree into the worktree. No content changes needed.

### 2. `detect/paths.rs`

Change the detection path for `HarnessId::Droid`:

```rust
// Before:
HarnessId::Droid => paths.push(home.join(".droid")),
// After:
HarnessId::Droid => paths.push(home.join(".factory").join("settings.json")),
```

### 3. `detect/harnesses.rs`

- Add `check_droid_telemetry(home: &Path) -> TelemetryStatus` that reads the primary shell RC and returns `On` if the `# trove:droid:start` fence is present, `Off` if the legacy `# trove:start` + `OTEL_TELEMETRY_ENDPOINT` block is present, and `Unknown` otherwise.
- Update `read_telemetry` match to dispatch to `check_droid_telemetry(...)` for `HarnessId::Droid`. **Note**: `read_telemetry` receives the config file path (`~/.factory/settings.json`), but droid's telemetry indicator lives in the shell RC — the new function reads the shell RC directly (via `wrapper_common::primary_shell_rc`), ignoring the passed path.
- Update `read_trove_region_present` match to return `true` when the Droid fence is in the primary shell RC. Like the telemetry check, it reads the shell RC independently.

### 4. `adapters/wrapper_common.rs` — new export-style helpers (DRY/reusable)

Add alongside the existing `WrapperSpec` pattern:

```rust
/// Spec for shell-RC adapters that inject plain `export KEY=VALUE` lines
/// rather than shell-function wrappers. The `legacy_body_probe` field enables
/// migration of legacy un-namespaced `# trove:start` blocks: if the probe
/// string appears in the body of a found legacy block, the block is adopted
/// and migrated to the namespaced form on the next upsert.
pub struct ExportSpec {
    pub adapter_id: &'static str,
    pub vars: &'static [(&'static str, &'static str)],
    pub legacy_body_probe: Option<&'static str>,
}
```

New public functions (mirroring the `WrapperSpec` trio):

- `build_export_block(spec: &ExportSpec) -> String` — renders `export K=V\n` lines
- `apply_export_to_primary_shell_rc(home, spec, opts)` — upserts, atomically writes, backs up
- `preview_export_for_primary_shell_rc(home, spec, opts)` — returns `PatchPreview` with `Fresh/Idempotent/Conflict` status
- `revert_export_primary_shell_rc(home, spec)` — strips the block (using `legacy_body_probe` to also strip any matching legacy block)

Implement `locate_export_block(content: &str, spec: &ExportSpec)` (private) that:

1. Checks for namespaced fence first
2. Falls back to legacy fence if `legacy_body_probe` is `Some` and the block body contains the probe string

### 5. `adapters/droid.rs` — new file

```rust
const SPEC: ExportSpec = ExportSpec {
    adapter_id: "droid",
    vars: &[("OTEL_TELEMETRY_ENDPOINT", "http://127.0.0.1:4318")],
    legacy_body_probe: Some("OTEL_TELEMETRY_ENDPOINT"),
};
```

`config_path`, `preview`, `apply`, `revert` each delegate to the matching `wrapper_common::*_export_*` function. Seven-case test suite + all supplementaries (see `adding-a-harness.md`).

**Note**: `apply` writes only `OTEL_TELEMETRY_ENDPOINT`. The adapter does NOT write:

- `OTEL_RESOURCE_ATTRIBUTES` — factory.ai SDK ignores it
- `OTEL_TELEMETRY_HEADERS` — these are transport-level HTTP headers (bearer auth); they cannot be promoted to OTLP resource attributes without custom sidecar infrastructure (`include_metadata: true` on the receiver + a metadata-to-attribute processor, neither of which Trove ships). Document in the module-level comment that `OTEL_TELEMETRY_HEADERS` is available if a user routes to a remote auth-gated collector, but Trove does not write it (the local sidecar needs no auth).

### 6. `adapters/mod.rs`

Add `pub mod droid;` in alphabetical position.

### 7. `harness.rs`

- Move `HarnessId::Droid` from the detection-only comment to `tier_1()`.
- Add method:
  ```rust
  /// Returns the metric-name prefix used for OTLP filtering and resource
  /// tagging in the collector, when the harness's SDK ignores
  /// `OTEL_RESOURCE_ATTRIBUTES` and `service.name` is too generic.
  /// Returns `None` for harnesses where standard service.name matching works.
  pub fn metric_name_tag_prefix(self) -> Option<&'static str> {
      match self {
          Self::Droid => Some("droid"),
          _ => None,
      }
  }
  ```
- Update `has_adapter` test and `detection_only_harnesses_have_no_adapter` test.

### 8. `ipc/commands.rs`

Three match arms in `preview_patch_inner`, `apply_patch`, `revert_patch`:

```rust
HarnessId::Droid => droid::preview(&home, &options),
HarnessId::Droid => droid::apply(&home, &options),
HarnessId::Droid => droid::revert(&home),
```

Update `harness_config_path` for Droid:

```rust
HarnessId::Droid => droid::config_path(&home),
```

No `spawn_tier3_watcher` changes needed — Droid is Tier 1 (native OTLP, no watcher).
Droid stays in the `None` arm of `spawn_tier3_watcher`.

### 9. `collector/codegen.rs`

Three changes, all driven by `id.metric_name_tag_prefix()` rather than `id == HarnessId::Droid`:

**a. `native_service_name_candidates`**: Change Droid's return from `&[]` to `&["droid"]`.
The value `"droid"` is a placeholder to keep Droid in `tag_harnesses`/`diag_harnesses` (both filtered by `!candidates.is_empty()`). The actual tagging and filtering will use name-based matching per (b) and (c).

**b. `build_harness_tag_block`**: For each harness where `id.metric_name_tag_prefix().is_some()`, emit a `context: metric` block (instead of `context: resource`) using `IsMatch(name, "^<prefix>\\.")` conditions:

```yaml
- context: metric
  statements:
    - 'set(resource.attributes["harness.id"], "droid") where IsMatch(name, "^droid\\.")'
    - 'set(resource.attributes["harness.name"], "Droid") where IsMatch(name, "^droid\\.")'
```

**c. `apply_diag_pipelines`**: For harnesses where `id.metric_name_tag_prefix().is_some()`, use `metrics.metric` OTTL context (not `metrics.datapoint`):

```yaml
filter/diag-droid:
  error_mode: ignore
  traces:
    span:
      - 'true' # drop all — Droid emits no traces
  metrics:
    metric:
      - 'not IsMatch(name, "^droid\\.")' # keep only droid.* metrics
  logs:
    log_record:
      - 'true' # drop all — Droid emits no logs
```

### 10. `mappings/defaults.rs`

Replace `detection_only_defaults(HarnessId::Droid)` with `droid_defaults()`:

```rust
fn droid_defaults() -> HarnessMapping {
    HarnessMapping {
        harness_id: HarnessId::Droid,
        enabled: true,
        sources: vec![
            SynthesizeFromNative { native: "droid.tool.invocations", target: Events, inject: {"event.kind": "tool.call"} },
            SynthesizeFromNative { native: "droid.tool.execution_time", target: TurnDuration, inject: {"event.kind": "tool.call"} },
            SynthesizeFromNative { native: "droid.code.files_modified", target: Events, inject: {"event.kind": "file.edit"} },
            SynthesizeFromNative { native: "droid.slash_command.invocations", target: Events, inject: {"event.kind": "chat.turn"} },
            // droid.mcp.tool_invocations — omitted by default, likely
            // overlaps with droid.tool.invocations; user can add via UI.
        ],
        cost_overrides: BTreeMap::new(),
    }
}
```

Update the `defaults_for` match to dispatch to `droid_defaults()` instead of `detection_only_defaults(id)` for Droid.

### 11. `tests/adapters_roundtrip.rs`

- Add `const DROID_ZSHRC_ORIGINAL: &str` fixture with a realistic `.zshrc` containing user content
- Add Droid to the all-adapters round-trip: `droid::apply` + assert Trove keys added, `droid::revert` + assert byte-identical restoration
- Extend `applying_one_adapter_does_not_disturb_the_other_files` for Droid
- Add `droid_migrates_legacy_fence_on_first_apply` test

---

## Verification

```bash
# Compile + unit tests
cargo test --manifest-path packages/app/src-tauri/Cargo.toml

# Cross-harness round-trip (includes new Droid case)
cargo test --manifest-path packages/app/src-tauri/Cargo.toml \
  --test adapters_roundtrip -- --nocapture

# Lint
cargo clippy --manifest-path packages/app/src-tauri/Cargo.toml \
  --all-targets -- -D warnings

# Coverage gate (≥85% line, ≥80% function)
cargo llvm-cov --manifest-path packages/app/src-tauri/Cargo.toml \
  --workspace \
  --ignore-filename-regex '(main\.rs|lib\.rs|tray\.rs|collector/)' \
  --fail-under-lines 85 --fail-under-functions 80

# TS/lint
pnpm lint && pnpm format:check && pnpm typecheck

# Live smoke test — confirm the adapter writes the fence, then Droid emits metrics
# 1. Run: cargo tauri dev (or install + launch the app)
# 2. Enable Droid in Harnesses tab
# 3. Verify ~/.zshrc has # trove:droid:start block (and legacy block is gone)
# 4. Source the shell RC: source ~/.zshrc
# 5. Run: python3 scripts/otlp-tap.py  (Terminal 1)
# 6. Run: droid  (Terminal 2, in any project directory)
# 7. Wait ~60s for Droid's flush interval
# 8. Confirm droid.* metrics arrive in Terminal 1
# 9. Confirm no harness.id in arriving resource attrs (expected — SDK ignores it)
# 10. Confirm Prometheus internal metrics show outgoing_items > 0 for filter/diag-droid:
#     curl http://127.0.0.1:18888/metrics | grep diag-droid
```

---

## Gotchas to keep in mind

1. **`service.name=cli`** arrives in Droid's OTLP but is too generic — never use it as a filter key for Droid.
2. **OTel filter semantics**: `true` = drop, `false` = pass. `'true'` is a literal constant that drops every record. Don't accidentally use `'false'` as a "drop all" — it passes everything through and causes bleed from other harnesses.
3. **Delta temporality**: Droid flushes every 60 seconds with delta aggregation. The `metricstransform action: insert` copies the data point including its temporality.
4. **Legacy fence migration**: The first `apply` must adopt and replace the legacy `# trove:start` block. Tests should cover this explicitly.
5. **No `OTEL_RESOURCE_ATTRIBUTES`**: Do not write this variable — the SDK ignores it and writing it wastes space without benefit.
6. **Phase 2 token watcher**: `~/.factory/logs/droid-log-single.log` path confirmed on this machine; verify it exists before implementing the watcher. The log file is a single rolling file — the watcher must handle rotation (file truncation/replacement) gracefully, same as `gemini_watcher.rs`.
7. **`contextCount` vs `cacheReadInputTokens`**: Both are input tokens but billed differently. Map them to separate `token.type` attribute values so cost computation can apply per-type pricing.
