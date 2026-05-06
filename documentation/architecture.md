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
