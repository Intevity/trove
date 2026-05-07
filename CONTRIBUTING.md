# Contributing to Trove

Thank you for considering a contribution. Trove is a small project; the contribution loop is intended to be lightweight.

## Code of Conduct

This project adopts the [Contributor Covenant 2.1](CODE_OF_CONDUCT.md). By participating you agree to abide by it.

## Development setup

You need:

- **Node.js ≥ 24** (use `nvm use` — `.nvmrc` pins the version).
- **pnpm ≥ 10**. Install via [Corepack](https://nodejs.org/api/corepack.html): `corepack enable && corepack prepare pnpm@latest --activate`.
- **Rust stable** with `rustfmt` and `clippy`. Install via [rustup](https://rustup.rs/).
- **Go ≥ 1.23** — only required to build the bundled OpenTelemetry Collector sidecar (`pnpm build:collector`). Pre-built binaries can also be downloaded; see [Building the collector locally](#building-the-collector-locally).
- Platform-specific Tauri prerequisites: see <https://v2.tauri.app/start/prerequisites/>.

Then:

```bash
pnpm install
pnpm build:collector                 # one-time per platform; produces resources/otelcol/dist/<triple>/trove-otelcol
pnpm bundle:sidecar                  # stages the binary into packages/app/src-tauri/binaries/
pnpm --filter @trove/app tauri:dev   # boots the desktop app
```

`tauri:dev` will fail to spawn the sidecar until both `build:collector` and `bundle:sidecar` have run at least once on the current host.

The first `pnpm install` also installs the [`lefthook`](https://lefthook.dev/) git hooks via the `prepare` script. They run Prettier + ESLint on staged files at commit time and `pnpm typecheck && pnpm test` at push time.

## Running checks locally

These are the same checks CI runs:

```bash
pnpm lint           # ESLint (flat config in eslint.config.ts)
pnpm format:check   # Prettier
pnpm typecheck      # tsc --noEmit per workspace package
pnpm test           # Vitest with coverage (60% Sprint-0 thresholds)
cargo clippy --manifest-path packages/app/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path packages/app/src-tauri/Cargo.toml
```

Run `pnpm lint:fix` and `pnpm format` to auto-fix where possible.

## Commit messages

We follow [Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/). Examples:

- `feat(adapters): add cursor-cli adapter`
- `fix(collector): restart sidecar on YAML reload`
- `docs(readme): clarify quickstart`
- `chore(deps): bump tauri to 2.4`
- `test(safety): cover read-only file case in atomic.rs`

Use the imperative mood. Squash trivial follow-ups before opening a PR.

## Pull requests

- Keep PRs small and focused on one concern. Sprint plans budget 2–4 PRs per sprint; aim for that scale.
- Every PR must keep CI green: lint, typecheck, vitest, clippy, cargo test.
- Update tests in the same PR as the code they cover.
- If your change touches user-visible behavior, update the relevant doc (README, `documentation/`, or wizard copy) in the same PR.

## Building the collector locally

A walk-through of the runtime shape — collector lifecycle, supervisor states, file system paths — lives in [`documentation/architecture.md`](documentation/architecture.md). What follows is the developer's how-to.

Trove ships a custom-built OpenTelemetry Collector binary as a Tauri sidecar. The build is driven by [ocb](https://opentelemetry.io/docs/collector/extend/ocb/) and pinned to a single version in `resources/otelcol/manifest.yaml`.

```bash
pnpm build:collector     # ~1–2 min on first run (downloads + compiles components)
pnpm bundle:sidecar      # cheap; copies the binary into the Tauri staging dir
```

The flow:

1. `scripts/build-collector.sh` resolves the host Rust target triple (or uses `TROVE_TARGET_TRIPLE` / `CARGO_BUILD_TARGET` / `TAURI_ENV_TARGET_TRIPLE` for cross-builds), installs the pinned `ocb` via `go install` if it's not already in `PATH`, then runs `ocb --config resources/otelcol/manifest.yaml`. The binary lands at `resources/otelcol/dist/<triple>/trove-otelcol[.exe]`.
2. `scripts/bundle-sidecar.ts` copies that binary into `packages/app/src-tauri/binaries/trove-otelcol-<triple>[.exe]` — the platform-suffixed filename Tauri's `externalBin` resolver expects. Tauri strips the suffix at bundle time, so the runtime sees a plain `trove-otelcol[.exe]` next to the app executable.

Both `_build/`, `dist/`, and the staged binaries are gitignored — every contributor builds locally for their host. CI re-runs the build on macOS-14 in a separate `sidecar-mac` job (PR-only).

To bump the Collector version, change every `v0.x.y` in `resources/otelcol/manifest.yaml` and the `OCB_VERSION` constant in `scripts/build-collector.sh`, then rebuild and update the integration tests if any output format has shifted.

## Adding a new harness adapter

The full walkthrough lives in [`documentation/adding-a-harness.md`](documentation/adding-a-harness.md), including a copy-pasteable adapter template and the seven-case test suite contributors must satisfy. The short version: each adapter is a Rust module under `packages/app/src-tauri/src/adapters/` that declares a `const SPEC: HarnessSpec` and delegates to `adapters::common`. The safety contract (atomic write, backup, sentinel-bracketed managed region, idempotent apply, clean revert) is enforced by the shared module — your adapter only owns `build_region` and its tests.

## Adding a new backend preset

YAML templates live in `packages/collector-presets/`. Add a new file, register it in the wizard's preset list, and write an integration test that exercises a synthetic OTLP payload against a stub of that backend.

## Reporting bugs

Open an issue with: OS, harness version(s), and a redacted excerpt of the Collector log. Issue templates land in Sprint 11.

## Reporting security issues

Please do **not** open a public GitHub issue. See [`SECURITY.md`](SECURITY.md) for the disclosure channel.
