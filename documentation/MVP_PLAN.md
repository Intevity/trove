# Trove — MVP Plan

A cross-platform tray app that auto-detects AI coding harnesses on a developer's machine, patches each one's telemetry configuration to emit OTLP, and forwards the unified stream to whichever observability backend the user chooses (SigNoz, Honeycomb, Grafana Cloud, Datadog, self-hosted Collector, etc.). Vendor-neutral on both sides — neutral on the harnesses, neutral on the destination.

---

## TL;DR

- **Core insight**: every popular AI coding harness either ships native OTEL or has a community hook/plugin that does. None of them are easy to configure correctly, and there is currently no unified configurator. That's the wedge.
- **Architecture**: Tauri v2 shell (Rust core) + TypeScript/React WebView UI + a bundled, supervised custom-built `otelcol` sidecar acting as the local OTLP gateway on `127.0.0.1:4317/4318`.
- **Why a local gateway**: one chokepoint for credentials, retries, PII redaction, and normalization across the `claude_code.*` / `gemini_cli.*` / `opencode.*` / `qwen_code.*` namespaces.
- **MVP scope**: ten harnesses, four destination presets (SigNoz, Honeycomb, Grafana Cloud, Datadog) plus generic OTLP, three OS targets (macOS, Windows, Linux).
- **Execution model**: 12 one-week sprints (~3 months) optimized for solo-dev-with-AI-agents implementation. Each sprint is a single coherent feature slice with explicit acceptance criteria.

---

## Resolved design decisions

| Decision | Value | Rationale |
|---|---|---|
| License | MIT | Maximally permissive; no CLA overhead. |
| Default backend in wizard | SigNoz | Fully OSS, aligns with an OSS-first configurator. |
| Auto-update default | Off (opt-in) | Privacy/control posture; matches the "no surprise network activity" promise. |
| App self-telemetry | None ever | Trove never phones home, period. |
| Collector binary | Custom-built via `ocb` | Ships ~30–50 MB instead of ~200 MB. |
| Repo layout | Mirror claude-sentinel monorepo | Faster bootstrap; same conventions across both apps. |
| Domain / GitHub org | Deferred to post-MVP | Repo stays under personal GH until ready for public launch. |
| Hosted onboarding page | Out of scope, even post-MVP | CLI install + in-app wizard is enough. |

---

## Architecture

### Component diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                       Trove Tray App (Tauri v2)                  │
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
│                                  │  - OTLP receiver           │  │
│                                  │    grpc :4317, http :4318  │  │
│                                  │  - Processors (redact,     │  │
│                                  │    rename, batch, retry)   │  │
│                                  │  - Exporters (per backend) │  │
│                                  └──────────┬─────────────────┘  │
│                                             │                    │
└─────────────────────────────────────────────┼────────────────────┘
                                              │ OTLP
                                              ▼
                              ┌───────────────────────────┐
                              │  User's chosen backend    │
                              │  SigNoz / Honeycomb /     │
                              │  Grafana / Datadog /      │
                              │  custom OTLP / self-host  │
                              └───────────────────────────┘

  Each harness on the machine is configured to point at 127.0.0.1:4317
  (gRPC) or :4318 (HTTP) — they never see the user's backend credentials.
```

### Why a local Collector instead of direct OTLP from each harness

Direct OTLP from each harness is what every vendor's docs show today. It works but it has real problems at scale:

- **Credential sprawl**: the user's API key ends up in `~/.claude/settings.json`, `~/.gemini/settings.json`, `~/.codex/config.toml`, the OpenCode plugin env, the Cursor hook scripts, and shell rc files. Rotating a key is now an archaeology project. Some of these files get committed to repos by mistake.
- **No buffering**: if the backend is down or rate-limited, telemetry from every harness is silently dropped at the source.
- **No normalization**: `claude_code.session.count`, `gemini_cli.session.count`, `qwen-code.session.count`, `opencode.tool.loc.added` — every harness uses its own namespace. Cross-tool dashboards are painful.
- **No PII gate**: `OTEL_LOG_USER_PROMPTS=1` exists in multiple harnesses. A single redaction processor in one Collector is much safer than per-harness configs.
- **Cardinality control**: bound at one place rather than fighting it per tool.

Putting a Collector between the harnesses and the world solves all of these. The user's backend credentials live exactly once, in the Collector config, which lives in a file the app owns.

### Stack and rationale

| Layer | Choice | Why |
|---|---|---|
| Desktop shell | **Tauri v2** | Same as `claude-sentinel`. Small binary, native tray on all three OSes, mature plugin ecosystem (autostart, single-instance, deep links, OS keychain), Rust core is ideal for filesystem and process work. |
| UI | **TypeScript + React + Vite** | Tauri's first-class WebView path. React keeps dev velocity high. Use `@tauri-apps/api` for IPC. |
| Component lib | **shadcn/ui + Tailwind** | Mirror claude-sentinel's Tailwind setup; shadcn for forms, tables, dashboards. |
| Sidecar | **Custom `otelcol` binary built via `ocb`** | OpenTelemetry Collector Builder produces a slim binary with only the receivers/exporters/processors Trove needs (~30–50 MB vs ~200 MB for `otelcol-contrib`). |
| Sidecar config | YAML generated by Rust core | The app writes a `collector.yaml` to the app data dir on every config change and signals the sidecar to reload. |
| Secrets | **OS keychain** via `keyring-rs` | Backend tokens never touch disk in plaintext. |
| Build | **pnpm 10 workspaces, Node 24** | Mirrors claude-sentinel: `packages/app` (Tauri), `packages/shared` (types), `packages/harness-adapters` (adapter metadata + UI strings), `packages/collector-presets` (YAML templates). |
| Static checks | TypeScript strict, ESLint (typescript-eslint flat config), Prettier, `clippy` for Rust | Standard tooling; copy lint config from claude-sentinel. |
| Testing | Vitest (TS), Playwright for end-to-end Tauri tests, `cargo test` for Rust | 95% line/function/statement coverage threshold to match claude-sentinel. |
| CI/CD | GitHub Actions, matrix `{ macos-14, windows-2022, ubuntu-22.04 } × { x86_64, aarch64 }` | Tauri's `tauri-action` handles building and signing. Reuse claude-sentinel's `release.yml` shape. |
| Release | Tauri's built-in updater + GitHub Releases | Code-sign on macOS and Windows; AppImage and `.deb` for Linux. Updater **off by default**. |

**On the Node.js 24 question**: keep Node strictly in the build/dev toolchain. The runtime that ships to users is Tauri (Rust) + WebView (Chromium/WebKit) + the custom `trove-otelcol` sidecar. No Node sidecar (unlike claude-sentinel, which has a `packages/daemon` — Trove doesn't need it because the OpenTelemetry Collector handles all the runtime work).

### Patterns to reuse from claude-sentinel

- **Atomic settings patch** (`packages/app/src-tauri/src/...write_settings_patch.rs` pattern): temp-file-then-rename with managed-key list and idempotent add/remove.
- **Sidecar bundling** (`tauri.conf.json` `externalBin` + per-platform binaries in `packages/app/src-tauri/binaries/`).
- **Tray icon dynamic state** (gray/blue/orange/red retinting based on health) → adapt to green/amber/red for Trove.
- **Single-instance via port bind**: Trove can use the Collector's `:13133` health port for the same purpose.
- **GitHub Actions release matrix** with `TAURI_SIGNING_PRIVATE_KEY` for the auto-updater manifest.
- **pnpm workspace ordering**: `packages/shared` builds to `dist/` before dependents — caught in CI step ordering.
- **Window close intercept**: hide instead of close so the sidecar keeps running.

---

## Top 10 harnesses for MVP

Ranked by approximate user base and integration tractability. The "Strategy" column is how Trove patches the harness; the "Effort" column is rough engineering effort for that adapter.

| # | Harness | Telemetry path | Config target | Strategy | Effort |
|---|---|---|---|---|---|
| 1 | **Claude Code** (CLI) | Native OTLP | `~/.claude/settings.json` `env` block | JSON merge | S |
| 2 | **Gemini CLI** | Native OTLP | `~/.gemini/settings.json` `telemetry` object | JSON merge | S |
| 3 | **OpenAI Codex CLI** | Native OTLP | `~/.codex/config.toml` `[otel]` table | TOML merge | M |
| 4 | **Qwen Code** | Native OTLP (mirrors Gemini) | `~/.qwen/settings.json` | JSON merge | S |
| 5 | **OpenCode** | Plugin-based | `~/.config/opencode/opencode.json` + plugin install | Install plugin, register | M |
| 6 | **Cursor IDE** | Hooks | `~/.cursor/hooks.json` (and per-project) | Drop hook script + register | L |
| 7 | **Cursor CLI** (`cursor-agent`) | Hooks (partial coverage) | same as above | Same as Cursor IDE; document gaps | L |
| 8 | **Cline** (VSCode ext) | No native OTEL; API/log polling | VSCode `settings.json` + log file watch | File watcher + parser | L |
| 9 | **Aider** | No native OTEL | Wrapper script or shell rc env | Wrapper + log parser | M |
| 10 | **GitHub Copilot CLI** | No native OTEL | Wrapper script | Wrapper + log parser; flag as "best-effort" | L |

### Effort key

- **S** — a few hundred lines, mostly config schema + tests.
- **M** — adapter plus auxiliary install steps (plugin, wrapper).
- **L** — adapter plus shipping our own collector/hook code per harness, ongoing maintenance burden as the upstream changes.

### Tier classification

- **Tier 1 — Native OTEL (1, 2, 3, 4)**: trivial to support; this is the MVP-of-the-MVP. Get these four shipping in Sprints 3–4 and the app is already useful to a meaningful slice of users.
- **Tier 2 — Plugin/hook (5, 6, 7)**: requires installing a plugin or hook script. Patch the host config to register the plugin, ship the plugin binary or script as a bundled resource. Cursor CLI is documented as having partial event coverage — call this out in the UI.
- **Tier 3 — Best-effort (8, 9, 10)**: no native OTEL, no clean hook surface. Either wrap the binary with a shim (`PATH` shadow, alias, or a launcher entry) or watch its log files / API. Lower fidelity, more brittle. Ship them last and label them clearly.

### Substitution candidates if any of the above fall through

Cline, Continue, Windsurf (Codeium), Roo Code, Zed AI, JetBrains AI Assistant. Pick whichever has the most Trove user demand once telemetry is on (yes, the app can track its own usage — but only ever to the **user's own backend**, never to a Trove-controlled endpoint; see "Security and privacy" below).

---

## Repository layout

Mirrors claude-sentinel's monorepo conventions.

```
trove/
├── packages/
│   ├── app/                          # Tauri desktop app (mirrors claude-sentinel/packages/app)
│   │   ├── src/                      # React + TS UI
│   │   ├── src-tauri/
│   │   │   ├── src/
│   │   │   │   ├── detect/           # Harness detection
│   │   │   │   ├── adapters/         # Per-harness patchers (one file each)
│   │   │   │   ├── collector/       # Sidecar lifecycle, YAML codegen
│   │   │   │   ├── secrets/          # Keychain wrapper
│   │   │   │   ├── safety/           # Atomic write, backup, sentinels
│   │   │   │   └── ipc/              # Tauri commands
│   │   │   ├── binaries/             # Bundled trove-otelcol per platform
│   │   │   ├── Cargo.toml
│   │   │   └── tauri.conf.json
│   │   └── package.json
│   ├── shared/                       # Zod schemas, IPC message types (mirrors claude-sentinel/packages/shared)
│   ├── harness-adapters/             # TS-side adapter metadata + UI strings
│   └── collector-presets/            # YAML templates for SigNoz, Honeycomb, etc.
├── resources/
│   ├── otelcol/                      # ocb manifest + build script + per-platform output
│   └── hooks/                        # Cursor hook scripts, OpenCode plugin
├── scripts/
│   ├── build-collector.sh            # Runs ocb to produce trove-otelcol per platform
│   └── bundle-sidecar.ts             # Stages binaries into packages/app/src-tauri/binaries/
├── documentation/
│   ├── MVP_PLAN.md                   # This document
│   ├── adding-a-harness.md
│   ├── adding-a-backend.md
│   └── architecture.md
├── .github/workflows/
│   ├── ci.yml                        # Lint, test, type-check on every PR
│   ├── release.yml                   # Tagged builds, signing, GH release
│   └── nightly.yml                   # Catch upstream harness regressions
├── pnpm-workspace.yaml
├── package.json
├── tsconfig.base.json
├── eslint.config.ts
├── .prettierrc
├── LICENSE                           # MIT
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── SECURITY.md
└── README.md
```

---

## Data model

A few core schemas, all defined in `packages/shared` with Zod and re-exported as both runtime validators and TS types.

### `HarnessId`

A discriminated union of supported harness identifiers: `"claude-code" | "gemini-cli" | "codex-cli" | "qwen-code" | "opencode" | "cursor-ide" | "cursor-cli" | "cline" | "aider" | "copilot-cli"`.

### `Backend`

```ts
type Backend =
  | { kind: "signoz"; region: string; ingestionKey: SecretRef }
  | { kind: "honeycomb"; team: SecretRef; dataset: string }
  | { kind: "grafana-cloud"; endpoint: string; auth: SecretRef }
  | { kind: "datadog"; site: string; apiKey: SecretRef }
  | { kind: "otlp-generic"; endpoint: string; protocol: "grpc" | "http"; headers: Record<string, SecretRef> }
  | { kind: "otelcol-passthrough"; endpoint: string };  // for users already running their own collector
```

`SecretRef` is an opaque keychain handle, never the secret itself.

### `HarnessConfig`

```ts
type HarnessConfig = {
  id: HarnessId;
  enabled: boolean;
  configPath: string;          // e.g. ~/.claude/settings.json
  lastPatchedAt: string;       // ISO timestamp
  trovePatchHash: string;      // hash of the block we wrote, for safe revert
  options: {
    logUserPrompts: boolean;
    customAttributes: Record<string, string>;
  };
};
```

### `AppState`

Persisted to `~/.config/trove/state.json` (XDG on Linux, equivalent on macOS/Windows). Contains the active backend reference, the per-harness configs, and the collector port assignments.

---

## Core flows

### First run

1. App launches, tray icon appears, main window opens.
2. **Detection sweep**: Rust core scans for harnesses (see "Detection strategies" below). Returns a list of `{ harness, detected, configPath, currentlyEmittingTelemetry }`.
3. **Backend wizard**: user picks a preset or chooses generic OTLP, enters credentials, and runs a test export. SigNoz is featured as the default. Credentials go into the OS keychain; only the keychain handle is persisted.
4. **Per-harness opt-in**: the user picks which detected harnesses to enable. For each, the app shows a diff preview of exactly what file will change and what will be written. No edits happen without explicit confirmation.
5. **Apply**: each enabled adapter writes its patch atomically (temp file, fsync, rename), tagging the inserted block with a sentinel comment so a future "Disable" can find and remove it cleanly.
6. **Sidecar start**: Rust core writes `collector.yaml` and spawns `trove-otelcol`.
7. **Health check**: app pings `127.0.0.1:4318/v1/metrics` with a synthetic payload, then watches for the corresponding signal at the backend (where the backend supports a query endpoint) or at minimum confirms the Collector accepted the payload.

### Detection strategies

In order of preference:

1. **Binary on PATH**: `which claude`, `which gemini`, `which codex`, `which opencode`, `which cursor-agent`, `which aider`, `which gh`, etc. Cheap, reliable.
2. **Standard config dirs**: presence of `~/.claude/`, `~/.gemini/`, `~/.codex/`, `~/.config/opencode/`, `~/.cursor/`, etc. Catches harnesses that aren't on PATH (e.g. invoked via `npx` or installed in a venv).
3. **App bundles** (macOS): `/Applications/Cursor.app`, `/Applications/Claude.app`. Mirror on Windows (`%LocalAppData%\Programs\...`) and Linux (`/opt/`, `~/.local/share/applications/`).
4. **VSCode extensions** (for Cline, Continue, etc.): scan `~/.vscode/extensions/` for known extension IDs.
5. **Process snooping (optional, off by default)**: a watcher that notices when known harness binaries are running. Useful for users with non-standard installs but introduces a permissions dialog on macOS, so make it opt-in.

### Configuration patching — the safety contract

This is the part where bugs hurt users. Every adapter must satisfy:

- **Atomicity**: write to `<file>.trove.tmp`, fsync, rename. Never partial writes.
- **Backup**: copy the original to `<file>.trove.bak.<timestamp>` before the first edit. Keep at most N backups.
- **Sentinels**: Trove-managed regions are wrapped in comments where the format allows them (`// trove:start ... // trove:end` for JSON-with-comments; `# trove:start ... # trove:end` for TOML/YAML). For pure JSON, use a top-level `_trove` key recording what we own and a content hash.
- **Idempotency**: re-running the patch with the same inputs is a no-op. Re-running with different inputs cleanly replaces the previous block.
- **Clean revert**: "Disable harness" removes only what Trove wrote, leaving everything else untouched.
- **Schema validation**: parse before writing; never destroy a malformed file silently. If the file can't be parsed, surface the error and refuse to patch — don't overwrite.
- **Conflict detection**: if the file's content hash has changed since we last touched it AND our managed block has been edited, surface a three-way merge UI rather than silently overwriting the user's changes.
- **Permissions preservation**: don't accidentally elevate a `0600` file to `0644`.

Every adapter ships with golden-file tests covering: fresh install, second-run idempotency, user-edited file, malformed file, missing parent dir, read-only file, file owned by another user.

### Collector lifecycle

The `trove-otelcol` sidecar is supervised by the Rust core. Specifically:

- Started on app launch when at least one harness is enabled.
- Restarted automatically on YAML changes (via SIGHUP on Unix, recreate on Windows).
- Health-checked on the Collector's own `:13133/health` endpoint.
- Tray icon reflects state: green (running, healthy), amber (running, no recent telemetry), red (crashed or misconfigured).
- Logs from the sidecar are tee'd to `~/.local/state/trove/collector.log` (size-capped) and surfaced in the dashboard's "Logs" tab.

### Collector YAML codegen

Don't let users edit the YAML directly in MVP. The app generates it deterministically from the user's chosen backend and enabled harnesses. Example shape:

```yaml
receivers:
  otlp:
    protocols:
      grpc: { endpoint: 127.0.0.1:4317 }
      http: { endpoint: 127.0.0.1:4318 }

processors:
  batch: { timeout: 5s }
  attributes/redact:           # PII redaction, on by default
    actions:
      - { key: user.prompt, action: delete }
      - { key: prompt.text,  action: delete }
  resource/source:             # tag every signal with which harness it came from
    attributes:
      - { key: trove.source, value: ${HARNESS_ID}, action: insert }
  filter/cardinality:          # cap dimension cardinality
    metrics:
      datapoint:
        - 'attributes["session.id"] != nil'  # only on metrics where it's safe

exporters:
  otlphttp/user-backend:
    endpoint: ${env:USER_BACKEND_ENDPOINT}
    headers:
      ${env:USER_BACKEND_HEADERS}

service:
  pipelines:
    metrics: { receivers: [otlp], processors: [batch, attributes/redact, resource/source, filter/cardinality], exporters: [otlphttp/user-backend] }
    logs:    { receivers: [otlp], processors: [batch, attributes/redact, resource/source], exporters: [otlphttp/user-backend] }
    traces:  { receivers: [otlp], processors: [batch, attributes/redact, resource/source], exporters: [otlphttp/user-backend] }
```

A "Show advanced YAML" expert toggle can come post-MVP for users who want to customize.

---

## Security and privacy

This app touches developer machines, OAuth tokens, and prompt content. Treat security as a first-class concern.

- **Secrets in the OS keychain only**, never `state.json`. Use `keyring-rs`.
- **Prompt logging is off by default**. The wizard surfaces it as an explicit, separately-acknowledged toggle. All current Tier 1 harnesses also default it off — match their behavior, don't escalate it.
- **Redaction at the Collector**, not just at the source. Even if a harness emits a raw prompt, the default pipeline drops `user.prompt` / `prompt.text` attributes before export.
- **Network egress is limited to the user's chosen backend**. No telemetry to anthropic, openai, trove, anybody else. **Trove never phones home.** A telemetry-free promise must be visibly enforced by code.
- **Code signing** on macOS (Developer ID + notarization) and Windows. AppImage signing on Linux.
- **Auto-update is opt-in** with a clearly labeled toggle, **off by default**. The app must work entirely offline once installed.
- **Threat model document** in `SECURITY.md`: what the app can and can't see, what files it touches, what it sends where, how to revoke. Be specific and conservative.
- **Patching is reversible**. Every "Enable" has a one-click "Disable" that fully reverts. Test this with property-based fuzzing.

---

## Sprint plan

Twelve one-week sprints. Each sprint is a single coherent feature slice an agent can implement in one focused session with 2–4 PRs. Each sprint specifies: **goal**, **scope**, **files affected**, **acceptance criteria**, **tests**, and **milestone (if any)**.

Sprints are sequential and assume the prior sprint is merged. Total elapsed time: ~12 weeks (~3 months) from project bring-up to 1.0.

---

### Sprint 0 — Repo bootstrap & tooling

**Goal**: empty-app skeleton that opens a tray icon and a window, with full lint/typecheck/test CI green on PR.

**Scope**:
- pnpm 10 workspace with `packages/app` (Tauri 2 + Vite + React 18 + TS strict) and `packages/shared` (placeholder Zod schemas).
- Tauri config with single hidden-by-default tray window, intercept close-to-hide.
- ESLint flat config, Prettier, tsc strict, `clippy --all-targets -- -D warnings` — copied from claude-sentinel.
- `LICENSE` (MIT), `README.md` with one-paragraph description and screenshot placeholder, `CONTRIBUTING.md`, `SECURITY.md` skeleton, `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1).
- `.github/workflows/ci.yml`: pnpm lint, prettier check, vitest, `cargo clippy`, `cargo test`. Ubuntu only for speed.
- Conventional Commits guidance in `CONTRIBUTING.md`. `lefthook` or `husky` pre-commit for format + lint.

**Files**: workspace root, `packages/app/`, `packages/shared/`, `.github/workflows/ci.yml`, top-level docs.

**Acceptance**:
- `pnpm install && pnpm -r build` succeeds locally on macOS arm64.
- `pnpm dev` opens the tray app; clicking the tray icon shows a "Hello, Trove" window; closing it hides (does not quit).
- CI is green on a PR that touches a TS file and a Rust file.

**Tests**: one Vitest "renders header" test, one `cargo test` smoke test, one Playwright launch-and-screenshot test.

**Milestone**: none — internal scaffolding only.

---

### Sprint 1 — Custom otelcol via `ocb` + sidecar lifecycle

**Goal**: a per-platform `trove-otelcol` binary (~30–50 MB) is built reproducibly, bundled into the Tauri app, and supervised by the Rust core with health checks.

**Scope**:
- `resources/otelcol/manifest.yaml`: ocb manifest enumerating only the components we need — receivers (`otlp`), exporters (`otlphttp`, `signoz`/`grafana`/`datadog` if first-class), processors (`batch`, `attributes`, `resource`, `filter`), extensions (`health_check`, `pprof`).
- `scripts/build-collector.sh`: runs `ocb` for each target triple (`darwin-arm64`, `darwin-x64`, `linux-x64`, `windows-x64`), writes binaries to `resources/otelcol/dist/<triple>/trove-otelcol[.exe]`.
- `scripts/bundle-sidecar.ts`: stages binaries into `packages/app/src-tauri/binaries/` with the platform-triple suffix Tauri's `externalBin` expects.
- `tauri.conf.json` updated with `externalBin` entry for `trove-otelcol`.
- `packages/app/src-tauri/src/collector/`: `lifecycle.rs` (spawn, monitor, restart on YAML change, kill on app exit), `health.rs` (poll `:13133/health` with backoff), `logs.rs` (tee child stdout/stderr to `~/.local/state/trove/collector.log` with size cap).
- A minimal hand-written `collector.yaml` for the smoke test (otlp receiver → log exporter, no real backend yet).

**Files**: `resources/otelcol/`, `scripts/build-collector.sh`, `scripts/bundle-sidecar.ts`, `packages/app/src-tauri/src/collector/`, `tauri.conf.json`.

**Acceptance**:
- Running `pnpm build:collector` produces a working binary on the host platform.
- App launch spawns the sidecar; `:13133/health` returns 200 within 3s.
- Killing the sidecar externally triggers a restart within 5s.
- App exit cleans up the sidecar (no zombie).

**Tests**: Rust integration test that spawns the bundled binary, hits health, and shuts it down. Run on macOS in CI.

**Milestone**: none.

---

### Sprint 2 — Filesystem safety primitives + shared schemas

**Goal**: a battle-tested toolkit for atomic writes, backups, sentinel-bracketed regions, and content-hash-based conflict detection — usable by every adapter.

**Scope**:
- `packages/app/src-tauri/src/safety/`:
  - `atomic.rs`: `write_atomic(path, bytes)` — temp + fsync + rename, preserves source mode.
  - `backup.rs`: `backup_file(path)` — copy to `<file>.trove.bak.<timestamp>`, prune to N most recent.
  - `sentinels.rs`: insert/replace/remove a managed region in JSON, JSONC, TOML, YAML. JSON uses a `_trove` top-level key with embedded hash.
  - `conflict.rs`: 3-way detection — compares `(stored_hash, current_hash, our_managed_block_hash)` and returns `Clean | UserEdited | Conflict`.
- `packages/shared/src/schemas.ts`: Zod schemas for `HarnessId`, `Backend`, `HarnessConfig`, `AppState`. Re-export TS types.
- `packages/shared/src/ipc-messages.ts`: typed IPC message contracts (request/response shapes).

**Files**: `packages/app/src-tauri/src/safety/`, `packages/shared/src/`.

**Acceptance**:
- All four format roundtrips (JSON / JSONC / TOML / YAML) preserve user content outside the managed region byte-for-byte.
- Property test: 1000 random patches → revert → file is byte-identical to original.
- Permissions are preserved across atomic write (`0600` stays `0600`).

**Tests**: golden-file tests per format with explicit cases for: fresh, idempotent, user-edited-outside-block, user-edited-inside-block, malformed, missing parent dir, read-only file. Property-based test using `proptest`.

**Milestone**: none.

---

### Sprint 3 — Detection sweep + Tier 1 adapters (Claude Code, Gemini CLI)

**Goal**: detect every Tier 1 harness on the user's machine, and patch two of them with the full safety contract.

**Scope**:
- `packages/app/src-tauri/src/detect/`: PATH probing, config-dir presence, app-bundle scanning. Returns `Vec<DetectedHarness>` with current telemetry status.
- IPC commands exposed via `#[tauri::command]`: `list_detected_harnesses`, `preview_patch(harness_id)`, `apply_patch(harness_id, options)`, `revert_patch(harness_id)`.
- `packages/app/src-tauri/src/adapters/claude_code.rs`: writes the `env` block in `~/.claude/settings.json` pointing at `127.0.0.1:4318`.
- `packages/app/src-tauri/src/adapters/gemini_cli.rs`: writes the `telemetry` object in `~/.gemini/settings.json`.
- React UI: a "Detected harnesses" list with per-row toggle and a diff-preview modal.

**Files**: `packages/app/src-tauri/src/detect/`, `packages/app/src-tauri/src/adapters/claude_code.rs`, `packages/app/src-tauri/src/adapters/gemini_cli.rs`, `packages/app/src-tauri/src/ipc/`, `packages/app/src/components/HarnessList.tsx`.

**Acceptance**:
- On a machine with Claude Code installed, the UI lists it as detected.
- Toggling on, previewing, and applying writes a valid `settings.json` that Claude Code accepts on next run.
- Toggling off cleanly removes Trove's block.
- Same for Gemini CLI.

**Tests**: golden-file suites for each adapter (the seven cases from Sprint 2). End-to-end test using a temp `$HOME` to confirm round-trip apply→revert→byte-identical.

**Milestone**: none.

---

### Sprint 4 — Tier 1 adapters complete (Codex CLI, Qwen Code) + adapter docs

**Goal**: the remaining two Tier 1 adapters are in, and the contract for adding a new adapter is documented well enough that an agent can add the next one without reading source.

**Scope**:
- `packages/app/src-tauri/src/adapters/codex_cli.rs`: TOML merge for `~/.codex/config.toml` `[otel]` table.
- `packages/app/src-tauri/src/adapters/qwen_code.rs`: JSON merge for `~/.qwen/settings.json`.
- Detection entries for both.
- `documentation/adding-a-harness.md`: step-by-step, with a template adapter file.
- Refactor any duplication that emerged across the four adapters into a shared trait/helper.

**Files**: `packages/app/src-tauri/src/adapters/codex_cli.rs`, `packages/app/src-tauri/src/adapters/qwen_code.rs`, detection updates, `documentation/adding-a-harness.md`, possibly `packages/app/src-tauri/src/adapters/common.rs`.

**Acceptance**:
- All four Tier 1 harnesses pass their golden-file suites.
- The adapter docs walk a fresh contributor through adding a hypothetical new harness end to end, including tests.

**Tests**: golden-file suites per adapter; one cross-harness integration test that detects-and-patches all four against a temp `$HOME`.

**Milestone**: none.

---

### Sprint 5 — Backend wizard + keychain + collector YAML codegen

**Goal**: user can complete the first-run wizard end to end: pick SigNoz, paste credentials, and watch a synthetic payload reach the backend.

**Scope**:
- `packages/app/src-tauri/src/secrets/`: `keyring-rs` wrapper; opaque `SecretRef` returned to UI.
- React backend wizard: steps for picking preset (SigNoz default, Honeycomb, Grafana Cloud, Datadog, Generic OTLP), entering credentials, and clicking "Test export".
- `packages/collector-presets/`: YAML templates per preset. `packages/app/src-tauri/src/collector/codegen.rs` interpolates the user's backend env vars into the active `collector.yaml`.
- "Test export" flow: app constructs a synthetic OTLP payload, posts it to `:4318`, watches the Collector logs/metrics for acceptance.
- `AppState` persistence to `~/.config/trove/state.json` (XDG-correct path on each OS) — secrets only as `SecretRef` handles.

**Files**: `packages/app/src-tauri/src/secrets/`, `packages/app/src-tauri/src/collector/codegen.rs`, `packages/collector-presets/`, `packages/app/src/components/wizard/`.

**Acceptance**:
- Fresh launch presents the wizard; selecting SigNoz, entering an ingestion key, and clicking "Test export" produces a green check within 5s on a working backend.
- Credentials end up in the OS keychain; `state.json` contains no secret material; `grep -r "<secret>"` of the disk turns up nothing.
- Reload of the Collector picks up the new YAML without dropping in-flight signals.

**Tests**: integration test using a stub OTLP HTTP receiver standing in for the user's backend; assertion that the synthetic payload reaches it. Vitest tests for the wizard React component.

**Milestone**: **Internal alpha** (macOS arm64 only).

---

### Sprint 6 — Status dashboard + tray state + end-to-end smoke

**Goal**: a working status dashboard that shows live per-harness health, sidecar state, and recent telemetry counts, plus a tray icon that reflects overall health at a glance.

**Scope**:
- React dashboard: per-harness rows (enabled? last signal seen? error?), sidecar panel (state / uptime / log tail), and a "Test pipeline" button that re-runs the synthetic export.
- Tray icon dynamic colors: green (all good) / amber (running but no recent telemetry) / red (crashed or misconfigured) — adapt the claude-sentinel retinting pattern.
- `packages/app/src-tauri/src/collector/metrics_tap.rs`: scrape Collector's pprof/health endpoints to derive recent counts; expose via IPC.
- Logs tab streams the tail of `collector.log`.
- One Playwright e2e: launch app → run wizard → enable Claude Code → trigger synthetic export → assert dashboard turns green.

**Files**: `packages/app/src/components/Dashboard*`, `packages/app/src-tauri/src/collector/metrics_tap.rs`, `packages/app/src-tauri/src/tray.rs`.

**Acceptance**:
- Tray icon transitions across the three states correctly.
- Dashboard shows non-zero recent counts after the synthetic export.
- Playwright e2e is green in CI.

**Tests**: Playwright e2e (above). Vitest tests for tray-state derivation logic.

**Milestone**: end-to-end MVP smoke complete.

---

### Sprint 7 — Tier 2 plugin/hook adapters (OpenCode + Cursor IDE + Cursor CLI)

**Goal**: three plugin/hook-style harnesses are supported, each shipping a vendored hook/plugin from `resources/`.

**Scope**:
- `resources/hooks/cursor-otel-hook.{js,sh}`: vendored Cursor hook script (forked or reimplemented from the spirit of `cursor-otel-hook`).
- `resources/hooks/opencode-trove-plugin/`: vendored OpenCode plugin pinned to a known-good version.
- `packages/app/src-tauri/src/adapters/opencode.rs`: install plugin into OpenCode's plugin dir, register in `~/.config/opencode/opencode.json`.
- `packages/app/src-tauri/src/adapters/cursor_ide.rs`: drop hook script + register in `~/.cursor/hooks.json`.
- `packages/app/src-tauri/src/adapters/cursor_cli.rs`: same hook surface; UI label states "partial event coverage" with a link to upstream issue.
- Detection updates for all three.

**Files**: `resources/hooks/`, three new adapter files, detection entries, `documentation/adding-a-harness.md` updated for plugin-style adapters.

**Acceptance**:
- Each adapter passes the seven golden-file cases (where applicable; plugin install adds a fresh-vs-upgrade case).
- Plugin install is idempotent and reversible — uninstall leaves OpenCode's plugin dir clean.
- The Cursor CLI partial-coverage advisory is visible in the UI before the user enables it.

**Tests**: golden-file suites; one integration test that exercises the OpenCode plugin against a stub plugin host.

**Milestone**: none.

---

### Sprint 8 — Conflict UI + remaining backend presets → public beta

**Goal**: handle the case where a user has hand-edited a config file Trove manages, and round out the backend preset list. Tag for public beta on macOS + Linux.

**Scope**:
- 3-way merge UI: when `conflict.rs` returns `Conflict`, the app surfaces a side-by-side view (original / yours / Trove's) with explicit "keep mine", "take Trove's", "merge manually" actions. Never silently overwrite.
- Grafana Cloud preset (endpoint + Bearer auth pattern) + Datadog preset (site + API key) + their YAML templates.
- Polish pass on the wizard and dashboard for first-time users.
- Linux packaging: `.deb` and AppImage via `tauri-action`.

**Files**: `packages/app/src/components/ConflictResolver/`, `packages/collector-presets/{grafana,datadog}.yaml`, `packages/app/src-tauri/src/safety/conflict.rs` integration, packaging config.

**Acceptance**:
- Editing a managed block by hand and re-running "Apply" lands in the conflict UI, not in a silent overwrite.
- All four backend presets pass test-export against their respective stub backends.
- A signed (or at minimum reproducibly-built) Linux AppImage is downloadable from a release artifact.

**Tests**: Playwright e2e covering the conflict flow. Vitest tests for the conflict resolver UI states.

**Milestone**: **Public beta** (macOS + Linux). Tag `v0.5.0`.

---

### Sprint 9 — Tier 3 best-effort adapters (Cline, Aider, Copilot CLI)

**Goal**: three best-effort adapters that don't have native OTEL but extract what they can via wrappers and log parsing. UI labels them clearly as "best-effort".

**Scope**:
- `packages/app/src-tauri/src/adapters/cline.rs`: VSCode `settings.json` patch + log file watcher under `~/.config/Code/User/globalStorage/<cline-ext-id>/`.
- `packages/app/src-tauri/src/adapters/aider.rs`: ship a small wrapper script (`trove-aider`) on PATH or via shell rc env; parse its log for token counts and command durations; emit OTLP from a small in-Rust emitter.
- `packages/app/src-tauri/src/adapters/copilot_cli.rs`: wrapper script around `gh copilot`; parse its output for invocation counts.
- Each adapter has a "best-effort" badge in the UI with a tooltip explaining what it does and doesn't capture.

**Files**: three new adapter files, `resources/wrappers/{trove-aider,trove-copilot}`, detection entries.

**Acceptance**:
- Each adapter detects, enables, and emits at least one signal end-to-end against a fixture run.
- UI clearly distinguishes Tier 3 visually from Tier 1/2.

**Tests**: golden-file suites; integration test that runs the wrapper against a captured-stdout fixture and asserts the emitted OTLP shape.

**Milestone**: none.

---

### Sprint 10 — Auto-update + code signing pipelines

**Goal**: signed, notarized macOS and Windows builds with Tauri's updater wired in (off by default), shipping via GitHub Releases.

**Scope**:
- `tauri-plugin-updater` integration; settings toggle in the UI; **default off**, clearly labeled.
- macOS pipeline: Apple Developer ID signing + notarization (`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`, `APPLE_CERTIFICATE`, `APPLE_SIGNING_IDENTITY`). Mirror claude-sentinel's commented-out infrastructure but flip it on.
- Windows pipeline: code-signing cert (`WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD`).
- `release.yml`: matrix `(macos-14, windows-2022, ubuntu-22.04) × (x64, arm64)`. `tauri-action` produces signed artifacts and an `latest.json` updater manifest. Manifest is signed with `TAURI_SIGNING_PRIVATE_KEY`.
- Release docs in `documentation/releasing.md` (how to cut a tag, secrets needed).

**Files**: `.github/workflows/release.yml`, `tauri.conf.json` updater config, `packages/app/src/components/Settings/AutoUpdate.tsx`, release docs.

**Acceptance**:
- A tag push produces signed macOS `.app` (and `.dmg`), notarized; signed Windows `.msi`/`.exe`; Linux AppImage + `.deb`. All show as artifacts on the GitHub Release.
- The auto-updater toggle is visible in Settings, defaults to off, and works when toggled on (verified on a side-by-side install of v0.x and v0.y).

**Tests**: manual: fresh-install both platforms from the signed artifacts; toggle updater on; cut a v0.y release; observe update prompt.

**Milestone**: **Release Candidate**.

---

### Sprint 11 — Diagnostics, sample dashboards, nightly CI, 1.0 release

**Goal**: ship 1.0. Self-test diagnostics for "why isn't my data showing up?", sample dashboards committed to the repo, nightly CI catching upstream regressions.

**Scope**:
- "Diagnostics" tab in the dashboard: runs a battery of checks (sidecar healthy? backend reachable? at least one harness enabled? recent signal observed?) with green/red rows and links to fix.
- `documentation/dashboards/`: sample Grafana JSON, SigNoz JSON, Honeycomb board export — derived from the `trove.source` resource attribute so they work across harnesses.
- `.github/workflows/nightly.yml`: re-runs every adapter's golden-file suite against the latest published version of each harness (pulled fresh nightly). Failure opens an issue tagged `adapter-regression`.
- Final pass on `SECURITY.md` (threat model: what we see, what we touch, what we send, how to revoke), issue templates (bug / adapter request / backend request), PR template.
- Tag `v1.0.0`.

**Files**: `packages/app/src/components/Diagnostics/`, `documentation/dashboards/*.json`, `.github/workflows/nightly.yml`, `SECURITY.md`, `.github/ISSUE_TEMPLATE/*`, `.github/PULL_REQUEST_TEMPLATE.md`.

**Acceptance**:
- Diagnostics tab correctly identifies a deliberately-broken backend config (fail) and a healthy one (pass).
- Sample dashboards load cleanly into a fresh SigNoz/Grafana/Honeycomb instance.
- Nightly CI runs once and produces a report.
- `v1.0.0` is tagged, signed, and published.

**Tests**: e2e smoke against the 1.0 build covering the golden first-run path.

**Milestone**: **1.0 release**.

---

### Post-1.0 (not sprint-scoped)

- More harnesses (Continue, Windsurf, Roo Code, Zed AI, JetBrains AI Assistant, etc.).
- More backend presets (New Relic, Dynatrace, Last9, Highlight, ClickHouse direct).
- Advanced YAML editor for power users.
- Built-in mini dashboard (recent activity, daily token spend) — *only* metrics computed from data already passing through the local Collector, no new collection.
- A `trovectl` CLI surface for headless / CI use.
- Optional: a "team mode" where multiple developers' Trove instances can share a managed config bundle (`.trove/team.json` committed to the repo).
- Domain + GitHub org lock-in (trademark search + purchase) before any public-launch announcement.

---

## Tooling and CI/CD

- **GitHub Actions** with three workflows:
  - `ci.yml` on every PR: `pnpm lint && pnpm typecheck && pnpm test && cargo clippy --all-targets -- -D warnings && cargo test`. Run on Ubuntu only for speed; gate slow-path checks on labels.
  - `release.yml` on tag push: matrix of `(macos-14, windows-2022, ubuntu-22.04) × (x64, arm64)`. Use `tauri-apps/tauri-action` for the heavy lifting. Sign macOS (Developer ID + notarization) and Windows. Upload to GitHub Releases.
  - `nightly.yml`: re-runs the harness adapter golden-file tests against the *latest published version* of each harness. This is how we catch upstream regressions before a user files an issue.
- **Pre-commit hooks** via `lefthook` or `husky`: format, lint, run affected tests.
- **Conventional Commits** + automated changelog via `changesets` or `release-please`.
- **Renovate** or **Dependabot** for dependency updates, including Collector component bumps.

---

## Open-source housekeeping

- **License**: **MIT**. No CLA.
- **Governance**: BDFL to start; document maintainer expectations in `MAINTAINERS.md` once a second maintainer joins.
- **`CONTRIBUTING.md`** must answer: how to add a new harness adapter (with template — see Sprint 4 deliverable), how to add a new backend preset, how to run the test suite, the safety contract for adapters.
- **`SECURITY.md`**: PGP key or `security.txt`, response SLO, scope, threat model. Finalized in Sprint 11.
- **`CODE_OF_CONDUCT.md`**: Contributor Covenant 2.1.
- **Issue templates**: bug (with required harness version, OS, redacted Collector log), adapter request, backend request. Sprint 11.
- **Discussion**: GitHub Discussions for Q&A, Discord/Slack only after community demand exists.
- **README**: lead with a screenshot of the tray + the dashboard. Most OSS READMEs bury the demo.
- **Sample dashboards** (Grafana JSON, SigNoz JSON, Honeycomb boards) committed to the repo. Sprint 11. Steal liberally from `claude-code-otel`, `signoz/codex-monitoring`, etc., where the licenses permit.

---

## Risks and known unknowns

- **Cursor CLI hook coverage is currently partial** (only `beforeShellExecution` / `afterShellExecution` reliably fire as of the latest reports). Plan around it: ship the hooks in Sprint 7, document the gaps in the UI, file upstream issues, and don't promise more than the upstream tool delivers.
- **OpenCode telemetry is plugin-based and the plugins are still evolving**. Pin to a known-good plugin version, vendor or fork it to control the supply chain, and follow upstream actively.
- **Collector binary size** is mitigated from day one by the `ocb` custom build (Sprint 1). Watch for component drift on Renovate/Dependabot bumps.
- **macOS gatekeeper / Windows SmartScreen** will flag a new tray app that touches user config files. Code-signing and notarization are non-negotiable from Sprint 10. Budget time for Apple Developer enrollment if not already done.
- **Tauri sidecar UX on macOS**: spawning a sidecar binary from a notarized `.app` requires hardened runtime entitlements done correctly. Verify in Sprint 10's signing pipeline, not after.
- **Harness vendor drift**: every harness will rev its config schema unprompted. The nightly CI run (Sprint 11) is your early warning system. Plan for an adapter to break every few months and have a "fall back to read-only mode" path that warns the user instead of corrupting their config.
- **Prompt content liability**: even with redaction defaults, some users will turn it on. Make the UI for that toggle scary in proportion to its consequences. Consider a per-harness opt-in rather than a global one.
- **Existing competitors**: `tobilg/ai-observer` (different shape — backend, not configurator), `cursor-usage-tracker` (Cursor Enterprise only). Position clearly: Trove is the *configurator and gateway*, not the backend. Recommend pairing with whatever backend the user prefers, including ai-observer.
- **Name collision check (post-MVP)**: "Trove" is a common English word with multiple existing companies but no dominant collision in the developer-tools or observability space. Verify trademark availability in software/SaaS classes before any public commits, and lock in the domain and GitHub org name before launch.
