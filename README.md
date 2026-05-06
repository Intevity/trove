# Trove

A cross-platform tray app that auto-detects AI coding harnesses on your machine, patches each one's telemetry configuration to emit OTLP, and forwards the unified stream to whichever observability backend you choose — SigNoz, Honeycomb, Grafana Cloud, Datadog, a generic OTLP endpoint, or your own self-hosted Collector. Vendor-neutral on both sides: neutral on the harnesses, neutral on the destination.

> _Screenshot placeholder — `documentation/screenshots/tray.png` lands once the dashboard is wired up in Sprint 6._

## Status

Pre-MVP. The implementation plan lives in [`documentation/MVP_PLAN.md`](documentation/MVP_PLAN.md). Sprint 0 (this scaffolding) ships an empty Tauri shell with green CI. Useful functionality begins in Sprint 1.

## What it does

Each AI coding harness on your machine — Claude Code, Gemini CLI, Codex CLI, Qwen Code, OpenCode, Cursor, Cline, Aider, GitHub Copilot CLI, and more — either ships native OpenTelemetry support or has a community hook that does. None of them are easy to wire up correctly, and there is no unified configurator. Trove is that configurator and acts as a local OTLP gateway:

- **One chokepoint** for credentials, retries, PII redaction, and cross-tool normalization.
- **Your backend, your credentials.** Trove never phones home — telemetry only goes to the destination you pick.
- **Reversible.** Every "Enable" has a one-click "Disable" that fully reverts the patched config.

## Quickstart (development)

```bash
# Requires: Node 24+, pnpm 10+, Rust stable (for Tauri)
pnpm install
pnpm --filter @trove/app tauri:dev   # opens the desktop app
```

Useful scripts:

| Command             | What it does                                                |
| ------------------- | ----------------------------------------------------------- |
| `pnpm dev`          | Run all package dev scripts in parallel                     |
| `pnpm build`        | Build all packages (TS only — Tauri build is `tauri:build`) |
| `pnpm test`         | Run vitest with coverage                                    |
| `pnpm typecheck`    | Type-check every package                                    |
| `pnpm lint`         | ESLint across the workspace                                 |
| `pnpm format:check` | Verify Prettier formatting                                  |

## Architecture

A short tour:

- **Tauri 2** Rust shell with a React/TypeScript WebView UI.
- **Custom OpenTelemetry Collector** built via `ocb` and bundled as a sidecar (~30–50 MB), supervised by the Rust core.
- **OS keychain** for backend credentials (via `keyring-rs`) — secrets never live in JSON state files.
- **Atomic, sentinel-bracketed config patching** so every "Enable" can be cleanly reverted.

For the full story see [`documentation/MVP_PLAN.md`](documentation/MVP_PLAN.md).

## License

MIT — see [`LICENSE`](LICENSE).

## Security

Trove never sends telemetry to a Trove-controlled endpoint. Threat model and reporting in [`SECURITY.md`](SECURITY.md).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for setup, testing, and the conventions we follow. Code of Conduct in [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
