# Releasing Trove

Trove ships via GitHub Releases. A tag push of the form `v*` triggers
[`release.yml`](../.github/workflows/release.yml), which builds platform
bundles in parallel on macOS arm64, macOS x64, Ubuntu x64, and Windows
x64 runners and uploads them to a draft release. A final job promotes
the draft to published once every matrix entry succeeds.

This document is the release runbook for Sprint 8 (public beta, v0.5.0).
Sprint 10 will layer in full Apple notarization + Windows code signing
on top of the same workflow.

---

## Pre-release checklist

Before pushing a tag, run through the following on `main`:

- [ ] Every PR in the sprint is merged and CI is green on `main`.
- [ ] All version-bearing files agree on the new version. The fields
      tauri-action embeds into bundle filenames live in:
  - `packages/app/src-tauri/tauri.conf.json` (`"version"`)
  - `packages/app/src-tauri/Cargo.toml` (`version`)
  - root `package.json`
  - `packages/app/package.json`
  - `packages/shared/package.json`
  - `packages/collector-presets/package.json`

  A quick check: `grep -rn '"version": "0\.[0-9]' --include="*.json" --include="*.toml" | grep -v node_modules | grep -v dist | grep -v Cargo.lock` should show the same number across all of them.

- [ ] `pnpm -r build` succeeds at the new version locally.
- [ ] `cargo build --release --manifest-path packages/app/src-tauri/Cargo.toml` succeeds.
- [ ] On a Linux machine (or a Linux-target Tauri build): `pnpm tauri build --target x86_64-unknown-linux-gnu` produces both an `.AppImage` and a `.deb` under `packages/app/src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/`.
- [ ] Smoke each Tier 1 + Tier 2 adapter: apply, dashboard turns green,
      revert, dashboard returns to neutral. (Tier 3 lands in Sprint 9.)

---

## Required secrets

The workflow expects two secrets to be set in repo settings →
**Secrets and variables → Actions**:

| Secret                               | Purpose                                                                                                                       | When to set                                                                                                                                                          |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TAURI_SIGNING_PRIVATE_KEY`          | Signs the auto-updater's `latest.json` manifest. The matching public key is embedded in `tauri.conf.json` (Sprint 10 wiring). | Once, before the first tag push. Generate with `pnpm --filter @trove/app exec tauri signer generate -- -w ~/.tauri/trove.key` and paste the contents of `trove.key`. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Decrypts the private key at signing time.                                                                                     | The password supplied to `tauri signer generate`.                                                                                                                    |

`GITHUB_TOKEN` is provisioned automatically by Actions; tauri-action
uses it to create the draft release.

Sprint 10 adds:

| Secret                                                | Purpose                                                       |
| ----------------------------------------------------- | ------------------------------------------------------------- |
| `APPLE_CERTIFICATE`                                   | base64-encoded `.p12` Developer ID Application cert.          |
| `APPLE_CERTIFICATE_PASSWORD`                          | Password for the `.p12`.                                      |
| `APPLE_SIGNING_IDENTITY`                              | Common name of the cert (e.g. `Developer ID Application: …`). |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`         | Apple notary submission credentials.                          |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Authenticode signing cert + password.                         |

> **Do not** set any signing-related secret to an empty string while the
> rest of the chain is incomplete — tauri-action's keychain import step
> fails on an empty `APPLE_CERTIFICATE` and won't fall back to unsigned.
> Either set every Apple secret with valid values or none of them.

---

## Tag procedure

Once the checklist is green:

```bash
# Confirm you're on the canonical commit.
git fetch origin
git checkout main
git pull --ff-only

# Sign + push the tag. tauri-action picks up `github.ref_name` as the
# release name, so the tag must match the on-disk version exactly.
git tag -s v0.5.0 -m "Trove v0.5.0"
git push origin v0.5.0
```

Watch the workflow in the Actions tab. On success, the release moves
from draft to published automatically (`publish-release` job at the
end of the matrix).

If a single matrix entry fails, the draft stays in draft state — fix
the issue, delete the draft + tag, and re-tag.

---

## Bundle layout

Each successful matrix entry contributes one or more files to the
release:

| Platform      | Files                                                                                |
| ------------- | ------------------------------------------------------------------------------------ |
| macOS (arm64) | `Trove_<version>_aarch64.dmg`, `Trove.app.tar.gz`, `Trove.app.tar.gz.sig`            |
| macOS (x64)   | `Trove_<version>_x64.dmg`, `Trove.app.tar.gz`, `Trove.app.tar.gz.sig`                |
| Linux         | `trove_<version>_amd64.AppImage`, `trove_<version>_amd64.deb`, plus `*.sig` files    |
| Windows       | `Trove_<version>_x64-setup.exe`, `Trove_<version>_x64_en-US.msi`, plus `*.sig` files |
| Updater       | `latest.json` (signed; consumed by `tauri-plugin-updater` in Sprint 10)              |

`bundle.targets: "all"` in `tauri.conf.json` is what produces both the
AppImage and the `.deb` on the Linux runner; no per-platform overlay
is required.

---

## What Sprint 10 changes

Sprint 10 keeps the matrix and tauri-action invocation but layers on:

1. Apple Developer ID signing + notarization for macOS bundles.
2. Authenticode signing for Windows bundles.
3. Updater toggle in the in-app Settings (off by default).
4. Verification of `latest.json` against the embedded public key.

Until that lands, the v0.5.0 release ships unsigned macOS and Windows
binaries; the README should call out the right-click → Open / bypass
SmartScreen workaround.
