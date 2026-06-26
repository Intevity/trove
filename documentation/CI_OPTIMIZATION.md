# CI optimization — cut macOS Actions spend

Status: **WS2 + WS3 implemented** (2026-06-25). WS1 is observe-only, WS4 needs a
self-hosted Mac, WS5 is account billing — all out of code scope. The workstream
prose below is kept as the design record.

## Implementation status

- **WS2 — universal macOS binary: done.** `release.yml`'s two macOS legs
  (`macos-arm64` + `macos-x64`) collapsed into one `macos-universal` leg
  (`--target universal-apple-darwin`); the toolchain step installs both real
  apple std targets; `build-collector.sh` gained a `universal-apple-darwin` case
  that builds both Go arches and `lipo`s them; `bundle-sidecar.ts` needed no
  change; the notary marker is now `notary-universal.json`; and
  `assemble-latest-json.mjs` maps the one universal tarball to all four darwin
  keys (`REQUIRED_KEYS` unchanged). `notary-staple.sh` was already arch-agnostic.
  Local smoke confirmed the sidecar is a fat `universal2` Mach-O (x86_64 + arm64).
- **WS3 — done, rescoped.** A literal "Linux `cargo check` dry-run" would be
  redundant: `ci.yml`'s `ci` job already compiles the Rust app on Linux every PR
  (`clippy --all-targets -D warnings` + `cargo test --workspace`). The only
  uncovered release step is _bundling_, so WS3 instead adds a PR-only, 1× Linux
  `bundle-linux` job that runs `tauri build --debug` (full bundler, unsigned, no
  release) gated on the existing src-tauri/sidecar paths-filter — catching
  `.deb`/`.rpm`/`.AppImage` + externalBin packaging breakage before a tag spends
  10× macOS minutes.
- **Pre-tag gate (still required before the first universal release):** run
  `rustup target add x86_64-apple-darwin` then
  `pnpm -F app tauri build --target universal-apple-darwin` locally with the
  lipo'd sidecar staged, and confirm the bundled app binary AND the inner
  sidecar are both fat (`file` / `lipo -info`). This validates the #1 risk —
  Tauri picking up `trove-otelcol-universal-apple-darwin` as the externalBin.
- **Related fix (separate PR):** the scheduled `notarize-poll` loop that burned
  10× macOS minutes on an un-finalizable stuck draft was fixed independently
  (completeness pre-flight + 24h age backstop in the `discover` job).

## Context

Trove's release workflow (`.github/workflows/release.yml`) builds four platforms.
On GitHub-hosted runners (private repo) the per-minute multipliers are **Linux
1×, Windows 2×, macOS 10×**. The release matrix runs **two** macOS jobs — both
on `macos-latest` (arm64): `macos-arm64` native and `macos-x64` _cross-compiled_
on the same arm64 runner — plus a short macOS **staple** job in
`notarize-finalize.yml`. That makes macOS roughly **90 % of billed minutes**:

| Leg                     | Wall-clock (cold cache) | Billed minutes            |
| ----------------------- | ----------------------- | ------------------------- |
| macOS × 2               | ~45 min each            | 2 × 45 × **10** = **900** |
| Windows                 | ~30 min                 | 30 × 2 = 60               |
| Linux                   | ~45 min                 | 45 × 1 = 45               |
| macOS staple (finalize) | ~5 min                  | 5 × 10 = 50               |

On 2026-06-24 two same-day full builds (v0.8.0 timed out → v0.8.1) tripped the
account's Actions **spending limit**, which blocked `notarize-wait`/`finalize`
mid-release (see `documentation` history / release-pipeline notes). This plan
reduces that spend.

## Goal & constraint

Reduce macOS billed minutes **while keeping Intel (x86_64) Mac support.**
→ Dropping the x64 build is **out of scope.** The chosen primary lever is a
**universal macOS binary** (one job builds both arches and `lipo`s them),
combined with reliable build caching.

## Current state (references as of this writing — verify line numbers before editing)

- Matrix with the two macOS legs: `.github/workflows/release.yml:43-66`
  (`macos-arm64` / `macos-x64`, each `os: macos-latest`, `args: --target <triple>`).
- `build-tauri` `timeout-minutes: 45` (raised from 30 in v0.8.1): `release.yml:40`.
- Rust cache: `Swatinem/rust-cache@v2`, `key: ${{ matrix.platform }}`,
  `shared-key: trove-rust-${{ runner.os }}-release`,
  `save-if: startsWith(github.ref, 'refs/tags/v')`: `release.yml:~173-183`.
- Sidecar build → stage → sign (macOS):
  - `Build trove-otelcol sidecar`: `release.yml:216` → `pnpm build:collector`
    with `TROVE_TARGET_TRIPLE=${{ matrix.rust_target }}`.
  - `Stage sidecar into Tauri binaries dir`: `release.yml:221` → `pnpm bundle:sidecar`
    (writes `packages/app/src-tauri/binaries/trove-otelcol-<triple>`).
  - `Sign sidecar binary (bottom-up, before tauri build)` (macOS): `release.yml:361`
    — `codesign --options runtime` on `binaries/trove-otelcol-${{ matrix.rust_target }}`.
  - `Build Tauri app`: `release.yml:408`.
  - `Submit to notary (no wait)` / `Attach notary submission id`: `release.yml:466` / `:494`
    (produces per-arch `notary-<arch>.json` on the draft release).
- Triple → Go mapping + output naming: `scripts/build-collector.sh:40-55`
  (`aarch64-apple-darwin→darwin/arm64`, `x86_64-apple-darwin→darwin/amd64`;
  output `resources/otelcol/dist/<triple>/trove-otelcol`).
- Sidecar staging: `scripts/bundle-sidecar.ts` (keys off `TROVE_TARGET_TRIPLE`).
- Tauri externalBin: `packages/app/src-tauri/tauri.conf.json:42`
  (`"externalBin": ["binaries/trove-otelcol"]` — Tauri appends the target triple,
  so a universal build looks for `binaries/trove-otelcol-universal-apple-darwin`).
- Updater manifest assembly: `scripts/assemble-latest-json.mjs:45-71`
  (classifies `*.app.tar.gz` by `aarch64` / `x64` into `darwin-aarch64*` /
  `darwin-x86_64*`; `REQUIRED_KEYS` lists both darwin keys).
- Notary poll reads markers generically: `scripts/notary-poll.mjs` `collectSubmissions`
  (globs `notary-*.json`).
- Staple job (macOS, 10×): `.github/workflows/notarize-finalize.yml:33-34`
  (`runs-on: macos-latest`, `xcrun stapler`).

---

## Workstream 1 — Confirm the Rust cache warms (low effort, do first)

The v0.8.1 `timeout-minutes: 30→45` change was specifically to let the
`rust-cache` **save** step finish on tag builds; before that the slow leg was
cancelled mid-save, so the cache never warmed and every release recompiled from
scratch (~45 min).

Action:

1. On the **next** release, confirm the macOS leg restores a warm cache (the
   `Swatinem/rust-cache` log prints "Cache restored…") and the build drops to
   ~10–15 min. If so, no change needed here.
2. Optional: set `cache-on-failure: false` (don't save a corrupt cache from a
   failed leg) and consider pruning `target` to deps-only if the _save_ step is
   still the slow part.

Do **not** try to share the cache with CI: the CI macOS job (`ci.yml`, the
`sidecar integration (macos-14)` lane) only builds the Go sidecar, not the Rust
app, so its cache wouldn't warm the app's `target`. The win is tag-to-tag reuse,
which is already enabled.

Expected effect: macOS legs ~45 → ~15 min ⇒ billed macOS ~900 → ~300 / release.
Effort: ~0 (verify only). Risk: none.

---

## Workstream 2 — Universal macOS binary (primary lever, keeps Intel)

Collapse the two macOS legs into **one** job that builds a universal2
(`arm64 + x86_64`) app via `--target universal-apple-darwin`. This removes one
full job's setup/cache overhead (×10) and one notary submission, and still ships
Intel support (one universal `.dmg`/`.app.tar.gz` runs on both).

### Files to change

**1. `release.yml` matrix (`:43-66`)** — replace the two macOS entries with one:

```yaml
- os: macos-latest
  platform: macos-universal
  rust_target: universal-apple-darwin # used for sidecar name + sign + tauri --target
  args: --target universal-apple-darwin
```

**2. Rust toolchain target install** — the universal build needs **both** std
targets. Where the toolchain is set up (`dtolnay/rust-toolchain@stable`), install
`aarch64-apple-darwin` and `x86_64-apple-darwin` for the macOS job (the single
`matrix.rust_target` no longer suffices). Add a macOS-only step or expand
`targets:`.

**3. Sidecar: build both arches + `lipo` into one universal binary.** The sidecar
is an externalBin; for a universal app Tauri expects
`binaries/trove-otelcol-universal-apple-darwin`. Two sub-options — recommend (a):

- (a) Teach `scripts/build-collector.sh` + `scripts/bundle-sidecar.ts` a
  `universal-apple-darwin` mode: build the collector for `aarch64-apple-darwin`
  **and** `x86_64-apple-darwin`, then
  `lipo -create <arm64> <x64> -output …/trove-otelcol` and stage it as
  `binaries/trove-otelcol-universal-apple-darwin`.
- (b) Keep the scripts arch-specific and add release.yml steps (macOS-only) that
  call `pnpm build:collector` twice (once per triple) and `lipo` + stage.

`build-collector.sh:40-55` currently errors on an unknown triple, so
`universal-apple-darwin` must be handled explicitly (it is **not** a single
GOOS/GOARCH).

**4. Sign sidecar (macOS) — `release.yml:361`.** It signs
`binaries/trove-otelcol-${{ matrix.rust_target }}`; with `rust_target:
universal-apple-darwin` this resolves to the lipo'd universal binary. Verify the
`codesign --options runtime` + `--verify --strict` pass on a fat binary (they do).

**5. Build Tauri app — `release.yml:408`.** `args: --target universal-apple-darwin`
flows through. Confirm the produced artifact names — Tauri emits
`Trove_<ver>_universal.dmg` and `Trove_universal.app.tar.gz` (verify the exact
suffix; it is **not** `aarch64`/`x64`).

**6. Updater manifest — `scripts/assemble-latest-json.mjs:45-57`.** The classifier
must map the single universal tarball to **both** darwin keys so `REQUIRED_KEYS`
(`darwin-aarch64*` + `darwin-x86_64*`) are satisfied by one file:

```js
if (name.endsWith('.app.tar.gz')) {
  if (name.includes('universal'))
    return ['darwin-aarch64-app', 'darwin-aarch64', 'darwin-x86_64-app', 'darwin-x86_64'];
  if (name.includes('aarch64')) return ['darwin-aarch64-app', 'darwin-aarch64'];
  if (name.includes('x64') || name.includes('x86_64'))
    return ['darwin-x86_64-app', 'darwin-x86_64'];
}
```

The existing "both X and Y map to key" guard (`:97`) stays correct — only the one
universal file maps to those keys. Keep `REQUIRED_KEYS` unchanged (Intel users
still served).

**7. Notary submission — `release.yml:466`/`:494`.** Now one universal app → one
submission → one `notary-universal.json` marker. Adjust the marker name/loop
(currently per-arch). `notary-poll.mjs`/`notarize-poll.yml` glob `notary-*.json`,
so they adapt automatically.

**8. Staple job — `notarize-finalize.yml:33`.** Confirm it staples by globbing
`*.dmg`/`*.app.tar.gz` (not hardcoded arch names). It still must run on macOS
(`xcrun stapler`), but now there is one dmg + one tarball instead of two each.

### Risk / unknowns to validate

- **Tauri universal externalBin packing** is the #1 risk: confirm Tauri picks up
  `trove-otelcol-universal-apple-darwin` (vs. expecting both per-arch files).
  Validate locally first: `pnpm -F app tauri build --target universal-apple-darwin`
  with a lipo'd sidecar staged, then `lipo -info` the sidecar inside the built
  `.app` and `file Trove.app/Contents/MacOS/Trove` (expect "two architectures").
- Universal app is ~2× size — fine for a desktop app; just larger downloads.
- `minimumSystemVersion` / entitlements unchanged.

Expected effect (on top of WS1): macOS jobs 2 → 1, one fewer notary submission,
~one job's overhead removed. Billed macOS roughly halves again vs. two warm
legs. Effort: **medium** (scripts + manifest + notary marker plumbing). Risk:
medium — gate behind a local universal build + one test release.

---

## Workstream 3 — Stop paying for redundant full builds (process, free)

- The `timeout-minutes: 45` fix already removes the cancel-and-retry double-build
  that caused v0.8.0 → v0.8.1.
- On a _post-build_ failure (e.g. notary/finalize/billing), recover with
  `gh run rerun <run-id> --failed` — it re-runs only the failed downstream jobs
  and **never rebuilds macOS**. (This is how v0.8.1 was finalized.)
- Optional: add a **PR dry-run** that runs `cargo check`/a Linux-only
  `tauri build` so build breakage is caught before a tag spends macOS minutes.
  Keep it Linux (1×).

---

## Workstream 4 — Optional: self-hosted macOS runner (eliminates macOS billing)

If releases become frequent, move the macOS job to a self-hosted Mac (Mac mini or
a dedicated machine). Self-hosted minutes are free on private repos ⇒ macOS cost
→ ~0.

Trade-offs: you own uptime + Xcode/toolchain upkeep, and signing/notary secrets
execute on your hardware — use a **dedicated, isolated** runner scoped to this
repo only, not a shared dev machine. Combine with WS2 (one universal job) to
minimize what the self-hosted box must do.

---

## Workstream 5 — Billing housekeeping (orthogonal)

- Raise the Actions spending limit so a release can't wedge mid-run again.
- Consider an org plan with more included minutes if release cadence grows.
- (Nuclear, likely N/A for a private product: a public repo gets free standard
  runners.)

---

## Estimated savings (billed minutes / release)

| Scenario                          | macOS | Win/Lin | Total | vs. today |
| --------------------------------- | ----- | ------- | ----- | --------- |
| Today (cold, 2 legs)              | ~950  | ~105    | ~1055 | —         |
| WS1 only (warm, 2 legs)           | ~350  | ~105    | ~455  | ~57 %     |
| WS1 + WS2 (warm universal)        | ~220  | ~105    | ~325  | ~69 %     |
| WS1 + WS2 + WS4 (self-hosted mac) | ~0    | ~105    | ~105  | ~90 %     |

(Universal still compiles both arches, so it's not a clean halving of compile
time; the savings come from one fewer job's setup/cache overhead and one fewer
notary submission, on top of the warm cache.)

## Recommended sequence

1. **WS1** — verify warm cache on the next release (free).
2. **WS2** — implement the universal build; validate with a local
   `--target universal-apple-darwin` build, then one test release tag.
3. **WS3** — add the Linux PR dry-run.
4. **WS4** — only if release cadence justifies running a self-hosted Mac.

## Verification

- Local universal smoke: build with a lipo'd sidecar; assert both the app binary
  and the bundled `trove-otelcol` are fat (`lipo -info` / `file`).
- Test release: tag a throwaway `vX.Y.Z-rc` (or use a test repo), confirm the
  draft has one `_universal.dmg` + one `_universal.app.tar.gz` (+ `.sig`),
  `latest.json` contains all four darwin keys pointing at the universal tarball,
  notarization accepts, finalize staples + publishes.
- Updater regression: an existing arm64 **and** an Intel install both see and
  apply the update (both resolve to the universal artifact).

## Rollback

Each workstream is independent. If the universal build misbehaves, revert the
WS2 commits to restore the two-leg matrix; WS1/WS3 are unaffected.
