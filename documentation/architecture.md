# Architecture

This document describes the runtime shape of Trove. It is the working reference for contributors; the canonical product spec is [`MVP_PLAN.md`](MVP_PLAN.md).

## Component overview

```
┌──────────────────────────────────────────────────────────────────┐
│                       Trove Tray App (Tauri 2)                   │
│                                                                  │
│  ┌─────────────────────┐         ┌────────────────────────────┐  │
│  │  WebView (TS/React) │ <────►  │   Tauri Core (Rust)        │  │
│  │  - Setup wizard     │  IPC    │  - Harness detection       │  │
│  │  - Per-harness UI   │         │  - Config patching (atomic)│  │
│  │  - Status dashboard │         │  - Keychain (OS secrets)   │  │
│  └─────────────────────┘         │  - Sidecar supervision     │  │
│                                  └──────────┬─────────────────┘  │
│                                             │ spawn/monitor      │
│                                  ┌──────────▼─────────────────┐  │
│                                  │  trove-otelcol (sidecar)   │  │
│                                  │  custom ocb-built binary   │  │
│                                  │  OTLP receiver + processors│  │
│                                  │  + per-backend exporters   │  │
│                                  └──────────┬─────────────────┘  │
│                                             │                    │
└─────────────────────────────────────────────┼────────────────────┘
                                              │ OTLP
                                              ▼
                              ┌───────────────────────────┐
                              │  User's chosen backend    │
                              │  SigNoz / Honeycomb /     │
                              │  Grafana / Datadog / OTLP │
                              └───────────────────────────┘
```

Sprint 1 ships everything to the right of the IPC boundary plus the supervisor that keeps the sidecar alive. Sprints 2–8 fill in detection, adapters, the wizard, the dashboard, and per-backend YAML codegen.

## The Collector sidecar

### Why a local Collector

Direct OTLP from each harness is what every vendor's docs show today. It works at small scale, but it has real problems:

- **Credential sprawl** — the user's API key would end up in `~/.claude/settings.json`, `~/.gemini/settings.json`, `~/.codex/config.toml`, the OpenCode plugin env, the Cursor hook scripts, and shell rc files. Rotating a key is then an archaeology project.
- **No buffering** — if the backend is down or rate-limited, telemetry from every harness is silently dropped at the source.
- **No normalization** — every harness uses its own metric namespace; cross-tool dashboards are painful.
- **No PII gate** — `OTEL_LOG_USER_PROMPTS=1` exists in multiple harnesses. A single redaction processor in one Collector is much safer than per-harness configs.
- **Cardinality control** — bound at one place rather than fighting it per tool.

A local Collector solves all of these. The user's backend credentials live exactly once, in the Collector config, which lives in a file Trove owns.

### Building the binary

[`resources/otelcol/manifest.yaml`](../resources/otelcol/manifest.yaml) is an [OpenTelemetry Collector Builder (ocb)](https://opentelemetry.io/docs/collector/extend/ocb/) manifest pinned to a single upstream version. It enumerates exactly the components Trove needs:

| Kind       | Components                                                                      |
| ---------- | ------------------------------------------------------------------------------- |
| Receivers  | `otlpreceiver` (gRPC :4317 + HTTP :4318)                                        |
| Exporters  | `otlpexporter`, `otlphttpexporter`, `datadogexporter`, `debugexporter`          |
| Processors | `batchprocessor`, `attributesprocessor`, `resourceprocessor`, `filterprocessor` |
| Extensions | `healthcheckextension`, `pprofextension`                                        |

`scripts/build-collector.sh` resolves the host Rust target triple (or honors `TROVE_TARGET_TRIPLE` / `CARGO_BUILD_TARGET` / `TAURI_ENV_TARGET_TRIPLE` for cross-compiles), maps it to `GOOS`/`GOARCH`, installs `ocb` at the pinned version via `go install` if not already in `PATH`, and writes the binary to `resources/otelcol/dist/<triple>/trove-otelcol[.exe]`.

`scripts/bundle-sidecar.ts` then copies that binary into `packages/app/src-tauri/binaries/trove-otelcol-<triple>[.exe]` — the platform-suffixed filename Tauri's [`externalBin`](https://v2.tauri.app/develop/sidecar/) resolver expects. Tauri strips the suffix at bundle time, so the runtime sees a plain `trove-otelcol[.exe]` next to the app executable.

### Bumping the Collector version

Coordinated edit:

1. Change every `v0.x.y` in `resources/otelcol/manifest.yaml` (one for each component plus `dist.otelcol_version`).
2. Update the `OCB_VERSION` constant in `scripts/build-collector.sh`.
3. Re-run `pnpm build:collector && pnpm bundle:sidecar`.
4. Verify the integration test in `packages/app/src-tauri/tests/collector_integration.rs` still passes.

## Metrics schema

Trove emits two tiers of metrics through the local Collector:

- **Tier A — harness-agnostic, generated by Trove.** A fixed set of
  five metrics that every harness adapter contributes to. Cross-harness
  dashboards live here.
- **Tier B — harness-native, passed through untouched.** Anthropic's
  `claude_code.token.usage`, Gemini's `telemetry.*`, etc. flow straight
  to the user's backend without renaming or reshaping. Drill-down
  dashboards live here.

A future user-facing mapping system (planned in
[`MAPPING_PLAN.md`](MAPPING_PLAN.md)) will let users synthesize Tier A
data points from Tier B for native-OTel harnesses, and edit the
per-harness defaults.

### Tier A metrics

| Metric                        | Type                                                                 | Unit      | Per–data-point attributes                                                                            |
| ----------------------------- | -------------------------------------------------------------------- | --------- | ---------------------------------------------------------------------------------------------------- |
| `trove.harness.events`        | Sum (Δ, monotonic)                                                   | `1`       | `event.kind` ∈ {`chat.turn`, `tool.call`, `shell.exec`, `file.edit`, `session.start`, `session.end`} |
| `trove.harness.tokens`        | Sum (Δ, monotonic)                                                   | `{token}` | `direction` ∈ {`input`, `output`}, `model`                                                           |
| `trove.harness.cost.usd`      | Sum (Δ, monotonic)                                                   | `USD`     | `model`, `cost.method` ∈ {`exact`, `estimated`}                                                      |
| `trove.harness.turn.duration` | Histogram (bounds `[0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300, 600]` s) | `s`       | `event.kind`                                                                                         |
| `trove.harness.errors`        | Sum (Δ, monotonic)                                                   | `1`       | `error.kind` ∈ {`rate_limit`, `auth`, `tool_failure`, `network`, `policy`, `unknown`}                |

All Tier A points carry these **resource attributes** (set once at
harness-adapter startup, not per-point): `service.name`, `harness.id`,
`harness.name`, `user.name`, `user.email`, `trove.source`. The resource
attributes are how SigNoz / Honeycomb / Grafana scope a metric series to
one harness — they don't inflate per-point cardinality.

### What's allowed on metrics vs logs

Hard rule: **a metric attribute must be from a small bounded set you
can enumerate by hand.** Everything else goes on the matching log
record. This is what keeps cardinality bounded forever as we add
harnesses.

| Field                                                                         |       Metric attribute?       | Log attribute? |
| ----------------------------------------------------------------------------- | :---------------------------: | :------------: |
| `harness.id`, `model`, `event.kind`, `direction`, `error.kind`, `cost.method` |              ✅               |       ✅       |
| `cursor.shell.exit_code`, `tool.outcome`, `tool.name`                         |        ❌ (unbounded)         |       ✅       |
| `conversation.id`, `generation.id`, `task.id`                                 |        ❌ (high-card)         |       ✅       |
| `command` text, `cwd`, file paths, error messages                             |              ❌               |       ✅       |
| `*.bytes` (numeric value)                                                     | ❌ (numeric, not categorical) |       ✅       |

Each adapter declares which Tier A metrics it can emit; sparse rows
across harnesses are expected (e.g. only Cline + native-OTel harnesses
can report tokens accurately; hooks have to estimate).

### Cost: exact vs estimated

For native-OTel harnesses (claude-code, codex, gemini, qwen) and the
Cline watcher, token counts come from the harness itself and cost is
**exact**: `cost.method = "exact"`.

For hook-based harnesses (Cursor IDE/CLI) and wrapper-based ones
(Aider, Copilot CLI, OpenCode), Trove only observes prompt/response
byte length, not the upstream tokenizer's count. We approximate
tokens as `bytes / 4` (an industry-standard rough heuristic; accuracy
~70–90% for English/code) and multiply by a per-model rate table baked
into the hook. Those points carry `cost.method = "estimated"`.

The tradeoff is intentional: dashboards default to summing both
methods (directionally useful — "did I spend $5 or $50 today?"); users
who want invoice-accurate numbers filter to `cost.method = "exact"`.
The rate table itself ships in
[`resources/hooks/cursor-otel-hook-impl.cjs`](../resources/hooks/cursor-otel-hook-impl.cjs)
(under `COST_RATES_USD_PER_1K_TOK`) and is revisited each Trove
release.

### Per-event correlation in stateless hooks

The Cursor hook (`cursor-otel-hook-impl.cjs`) runs as a fresh Node
process per event, so there's no in-memory state to compute the
duration between a `beforeSubmitPrompt` and its matching
`afterAgentResponse`. The hook stores a one-line timestamp file in
`$TMPDIR/trove-cursor-turns/<conversation>__<generation>.t` on the
`before*` event and reads + deletes it on the `after*` event. If the
marker is missing or stale (> 1 hour) the histogram observation is
skipped for that turn — the counter and tokens still emit. Other
stateless adapters should follow the same pattern.

## Supervisor lifecycle

The Rust core owns the Collector child for the app's lifetime. The implementation lives at [`packages/app/src-tauri/src/collector/`](../packages/app/src-tauri/src/collector/).

### State machine

```
                ┌──────────┐
                │   Idle   │  (initial — before first spawn)
                └────┬─────┘
                     │ Supervisor::start
                     ▼
                ┌──────────┐
                │ Starting │  (child spawned, health probe in flight)
                └────┬─────┘
        health 200   │       child.wait() returns
        ┌────────────┴─────────────┐
        ▼                          ▼
   ┌──────────┐               ┌──────────┐
   │ Running  │ ───────────► │ Crashed  │  (unexpected exit; backoff,
   └────┬─────┘  child.wait   └────┬─────┘   then re-enter Starting)
        │                          │
        │ shutdown_rx fires        │ shutdown_rx fires
        ▼                          ▼
   ┌──────────┐               ┌──────────┐
   │ Stopping │ ────────────► │ Stopped  │
   └──────────┘               └──────────┘
```

`Failed { reason }` is the terminal state for unrecoverable spawn errors (binary missing, permission denied, etc.) — the supervisor task exits without further restart attempts. Sprint 6's dashboard surfaces this to the user.

### Restart policy

- Initial backoff: 500 ms, doubled to a max of 5 s.
- A run that survives at least 30 s resets the backoff to 500 ms before the next failure (so a long-running collector that crashes once doesn't immediately go to a 5 s wait).
- Restart count is monotonic — it never resets.

### Shutdown

- `SupervisorHandle::shutdown()` flips a oneshot, the loop calls `start_kill()` on the child, then waits up to 5 s for graceful exit before sending SIGKILL.
- `RunEvent::ExitRequested` (Tauri's exit hook) calls `shutdown()` synchronously via `block_on`.
- On Unix, `SIGINT` / `SIGTERM` are bridged to `app.exit(0)` so external kills follow the same path. Without this, the OS reaps the parent immediately and the child orphans onto PID 1.

### Health probe

[`collector::health::wait_until_healthy`](../packages/app/src-tauri/src/collector/health.rs) polls `http://127.0.0.1:13133/health` (the OTel Collector's `health_check` extension) every 100 ms with a 1 s per-request timeout, until the deadline. It treats network errors like a non-200 response — the supervisor doesn't care _why_ it isn't ready, only that it isn't yet.

## File system layout

| What                             | macOS                                               | Linux (XDG)                          | Windows                                   |
| -------------------------------- | --------------------------------------------------- | ------------------------------------ | ----------------------------------------- |
| Bundled binary                   | `<App>/Contents/MacOS/trove-otelcol`                | `<bundle>/trove-otelcol`             | `<bundle>\trove-otelcol.exe`              |
| `collector.yaml`                 | `~/Library/Application Support/com.intevity.trove/` | `~/.config/com.intevity.trove/`      | `%APPDATA%\com.intevity.trove\`           |
| `collector.log` (+ `.1` rotated) | `~/Library/Logs/com.intevity.trove/`                | `~/.local/state/com.intevity.trove/` | `%LOCALAPPDATA%\com.intevity.trove\Logs\` |

The `collector.yaml` is the smoke configuration in Sprint 1 (loopback OTLP receiver → debug exporter). Sprint 5 replaces it with a backend-specific YAML codegen'd from the user's wizard answers.

`collector.log` is size-capped at 10 MiB with a single-rotation to `collector.log.1`. Sprint 6's dashboard streams the tail of this file into the UI's Logs tab.

## Patterns reused from claude-sentinel

Trove and [claude-sentinel](https://github.com/jeffwooden/claude-sentinel) share several conventions; both apps mirror the same monorepo layout and Tauri 2 patterns.

| Pattern                        | claude-sentinel reference                                    | Trove use                                                        |
| ------------------------------ | ------------------------------------------------------------ | ---------------------------------------------------------------- |
| Sidecar via `externalBin`      | `tauri.conf.json` `"externalBin"`                            | `tauri.conf.json` `"externalBin": ["binaries/trove-otelcol"]`    |
| Platform-triple staging        | `packages/daemon/scripts/build-sidecar.mjs`                  | `scripts/bundle-sidecar.ts`                                      |
| Sidecar path resolution        | `daemon::sidecar_path`                                       | `lib::sidecar_binary_path` (`current_exe().parent().join(name)`) |
| Async supervisor structure     | `daemon::spawn` (`tauri::async_runtime::spawn` from `setup`) | `Supervisor::start`                                              |
| Window close → hide            | `lib.rs` `WindowEvent::CloseRequested` intercept             | identical                                                        |
| Single-instance via known port | daemon health on `:47284`                                    | collector health on `:13133` (Sprint 6)                          |

## CI shape

| Job                                | OS           | Trigger    | What it does                                                                                                             |
| ---------------------------------- | ------------ | ---------- | ------------------------------------------------------------------------------------------------------------------------ |
| `lint / typecheck / test (ubuntu)` | ubuntu-22.04 | every push | pnpm lint/typecheck/vitest, builds collector to satisfy `externalBin`, then clippy + cargo test (incl. integration test) |
| `playwright e2e (ubuntu)`          | ubuntu-22.04 | PR only    | Vite e2e against the React UI                                                                                            |
| `sidecar-mac`                      | macos-14     | PR only    | Builds the collector for darwin and runs the cargo integration test against it for platform-specific coverage            |

Sprint 10 adds a `release` workflow that produces signed artifacts for all three platforms.
