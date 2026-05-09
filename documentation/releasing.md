# Releasing Trove

Trove ships via GitHub Releases. A tag push of the form `v*` triggers
[`release.yml`](../.github/workflows/release.yml), which builds platform
bundles in parallel on macOS arm64, macOS x64, Ubuntu x64, and Windows
x64 runners and uploads them to a draft release. A final job promotes
the draft to published once every matrix entry succeeds.

This document is the release runbook from Sprint 8 (public beta) onward.
Sprint 10 layered Apple Developer ID signing + notarization and Windows
Authenticode signing on top of the same workflow, plus the in-app
auto-updater. Sections below cover the operator steps for both.

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

The workflow consults the following secrets in repo settings →
**Secrets and variables → Actions**. Updater-manifest signing was
already wired in Sprint 8; the macOS / Windows entries land with
Sprint 10 and only run when fully populated.

### Updater (always required)

| Secret                               | Purpose                                                                                                                                                                                                                                | How to set                                                                                                                                             |
| ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `TAURI_SIGNING_PRIVATE_KEY`          | Signs the auto-updater's `latest.json` manifest. The matching public key lives in `packages/app/src-tauri/tauri.conf.json` `plugins.updater.pubkey` (replace the `REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY` placeholder before tag push). | Once. Generate with `pnpm --filter @trove/app exec tauri signer generate -- -w ~/.tauri/trove.key`. Paste the contents of `trove.key` into the secret. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Decrypts the private key at signing time.                                                                                                                                                                                              | The password supplied to `tauri signer generate`.                                                                                                      |

The matching public key is printed alongside the private key during
generation; that string goes into `tauri.conf.json`. The in-app
updater (Sprint 10 PR 1) verifies every fetched `latest.json` against
this pubkey, so a mismatch surfaces as `IpcError::UpdaterCheckFailed`
in the UI and refuses the install.

### macOS code-signing + notarization (Sprint 10)

| Secret                       | Purpose                                                                                                          |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `APPLE_CERTIFICATE`          | Base64-encoded `.p12` of the Developer ID Application identity exported from Keychain Access (full chain).       |
| `APPLE_CERTIFICATE_PASSWORD` | Password supplied when exporting the `.p12`.                                                                     |
| `APPLE_SIGNING_IDENTITY`     | Common name of the cert, e.g. `Developer ID Application: Intevity (TEAMID1234)`.                                 |
| `APPLE_ID`                   | Apple ID logged into the Apple Developer account.                                                                |
| `APPLE_PASSWORD`             | App-specific password (not the Apple ID password). Generate at https://appleid.apple.com → Sign-In and Security. |
| `APPLE_TEAM_ID`              | 10-character Team ID from https://developer.apple.com/account → Membership.                                      |

### Windows Authenticode (Sprint 10)

| Secret                         | Purpose                                                 |
| ------------------------------ | ------------------------------------------------------- |
| `WINDOWS_CERTIFICATE`          | Base64-encoded `.pfx` Authenticode signing certificate. |
| `WINDOWS_CERTIFICATE_PASSWORD` | Password for the `.pfx`.                                |

`GITHUB_TOKEN` is provisioned automatically by Actions; `tauri-action`
uses it to create the draft release.

> **All-or-nothing rule.** If the macOS chain is incomplete, leave
> every Apple secret unset rather than setting some to empty strings.
> An empty `APPLE_CERTIFICATE` value triggers a keychain-import failure
> in `tauri-action` that aborts the entire matrix entry instead of
> falling back to unsigned. The same applies to the Windows pair.

### Generating the secrets

**Updater key pair**:

```bash
pnpm --filter @trove/app exec tauri signer generate -- -w ~/.tauri/trove.key
# Records the private key to ~/.tauri/trove.key; prints the matching
# public-key string to stdout. Paste the public key into
# packages/app/src-tauri/tauri.conf.json `plugins.updater.pubkey`.
```

**Apple `.p12` export and base64 encoding**:

```bash
# After enrolling in Apple Developer + creating a Developer ID
# Application cert in Keychain Access, right-click the identity → Export
# → File Format: Personal Information Exchange (.p12). Choose a strong
# password — that is APPLE_CERTIFICATE_PASSWORD.
base64 -i ~/Downloads/trove-developer-id.p12 | pbcopy
# Paste the clipboard contents into the APPLE_CERTIFICATE secret.
```

**Apple `Team ID`**: see https://developer.apple.com/account → Membership.

**Apple app-specific password**: https://appleid.apple.com → Sign-In and
Security → App-Specific Passwords → Generate. Label it `Trove
notarization`. Store the resulting 16-character password in
`APPLE_PASSWORD`.

**Windows `.pfx`**: produced by your code-signing certificate vendor
(SSL.com, DigiCert, etc.). Same base64 trick:

```bash
base64 -i ~/Downloads/trove-authenticode.pfx | pbcopy
```

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

## Sprint 10 verification playbook

Sprint 10 ships the wiring; the operator confirms it after the first
signed tag is cut. Run through these checks once per release:

### macOS — signed + notarized bundle

```bash
# Download and mount the .dmg from the published GitHub Release.
hdiutil attach Trove_<version>_aarch64.dmg
cp -R "/Volumes/Trove/Trove.app" /Applications/
hdiutil detach "/Volumes/Trove"

# Expects: 'accepted' + 'source=Notarized Developer ID'.
spctl --assess --verbose=4 /Applications/Trove.app
```

If `spctl` reports `source=Unnotarized Developer ID`, notarization
silently failed — check the `tauri-action` step logs for the
`notarytool submit` invocation and submit a `notarytool log <id>`
follow-up against the staged credentials.

### Windows — Authenticode

Right-click the downloaded `.exe` → **Properties** → **Digital
Signatures** tab → confirm cert chain validates without warnings.
SmartScreen reputation builds over downloads; the _signature_ check is
the immediate one.

### In-app auto-updater loop

1. Install the previous release (e.g. `v0.5.x`) from the signed
   bundle.
2. Open Trove → **Updates** section in the dashboard → tick "Automatically check for updates".
3. Confirm the toggle persists by quitting + relaunching:
   `~/Library/Application Support/com.intevity.trove/state.json`
   should show `"autoUpdateEnabled": true` and `"schemaVersion": 4`.
4. Cut the next tag (`v0.5.y`).
5. Click "Check for updates now…" — the panel should report the new
   version is available. Tauri's updater downloads + installs the new
   bundle and triggers `app.restart()`. Verify the upgrade landed by
   checking the about dialog / `state.json` `schemaVersion`.

### Updater pubkey mismatch (negative test)

If the user receives an "update check failed" with a signature error,
the most common cause is that `tauri.conf.json` `plugins.updater.pubkey`
no longer matches `TAURI_SIGNING_PRIVATE_KEY`. Regenerate the key pair
(`tauri signer generate`), update both the secret and the config, and
re-tag.
