# Contributing to Trove

Thank you for considering a contribution. Trove is a small project; the contribution loop is intended to be lightweight.

## Code of Conduct

This project adopts the [Contributor Covenant 2.1](CODE_OF_CONDUCT.md). By participating you agree to abide by it.

## Development setup

You need:

- **Node.js ≥ 24** (use `nvm use` — `.nvmrc` pins the version).
- **pnpm ≥ 10**. Install via [Corepack](https://nodejs.org/api/corepack.html): `corepack enable && corepack prepare pnpm@latest --activate`.
- **Rust stable** with `rustfmt` and `clippy`. Install via [rustup](https://rustup.rs/).
- Platform-specific Tauri prerequisites: see <https://v2.tauri.app/start/prerequisites/>.

Then:

```bash
pnpm install
pnpm --filter @trove/app tauri:dev   # boots the desktop app
```

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

## Adding a new harness adapter

A full walkthrough lives in [`documentation/adding-a-harness.md`](documentation/adding-a-harness.md) (lands in Sprint 4). Until then, the short version: each adapter is a Rust module under `packages/app/src-tauri/src/adapters/` that implements the safety contract (atomic write, backup, sentinel-bracketed managed region, idempotent apply, clean revert) and ships golden-file tests.

## Adding a new backend preset

YAML templates live in `packages/collector-presets/`. Add a new file, register it in the wizard's preset list, and write an integration test that exercises a synthetic OTLP payload against a stub of that backend.

## Reporting bugs

Open an issue with: OS, harness version(s), and a redacted excerpt of the Collector log. Issue templates land in Sprint 11.

## Reporting security issues

Please do **not** open a public GitHub issue. See [`SECURITY.md`](SECURITY.md) for the disclosure channel.
