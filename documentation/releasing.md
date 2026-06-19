# Releasing Trove

Trove ships via GitHub Releases plus an S3-hosted auto-update channel.
A tag push of the form `v*` triggers
[`release.yml`](../.github/workflows/release.yml), which builds platform
bundles in parallel on macOS arm64, macOS x64, Ubuntu x64, and Windows
x64 runners and uploads them to a draft release. macOS bundles are
Developer ID-signed and notarized; notarization is **decoupled** from
the build so the expensive macOS runners never wait on Apple's queue.
Once notarization is Accepted, a reusable finalize workflow staples the
artifacts, publishes the updater channel to S3, and promotes the draft.

The release pipeline is a deliberate mirror of
[claude-sentinel](https://github.com/Intevity/claude-sentinel)'s —
Sentinel leads, Trove follows. When changing this pipeline, check
whether Sentinel already has the pattern and port it.

---

## Pipeline overview

```
git tag v0.6.0 ──▶ release.yml
                    ├─ build-tauri (×4 legs, 30 min cap)
                    │   ├─ stamp version + updater endpoint from tag/vars
                    │   ├─ build sidecar → codesign it (macOS)
                    │   ├─ tauri-action: build + sign (NO notarize)
                    │   └─ notarytool submit --no-wait → notary-<arch>.json
                    │      markers attached to the DRAFT release
                    ├─ notarize-wait (1× ubuntu, ~20 min inline poll)
                    │   ├─ Accepted → finalize now (fast path)
                    │   └─ Pending  → defer, exit 0 (slow path)
                    └─ finalize → notarize-finalize.yml (reusable)
                        ├─ staple: staple .dmg + .app.tar.gz inner app,
                        │          re-tar, re-sign with the minisign key
                        ├─ publish-updates: assemble latest.json (all
                        │          platforms) + upload to S3 via OIDC
                        └─ publish-release: clear markers, drop the
                                   stale release latest.json, promote

notarize-poll.yml (cron 13,43 * * * *) — slow-path companion: finds
draft releases carrying notary-*.json markers, polls Apple once, and
invokes the same notarize-finalize.yml when everything is Accepted.
```

Key properties:

- **Version is stamped from the tag.** `tauri.conf.json` and
  `Cargo.toml` are patched in CI before the build; the committed
  versions are only the dev baseline. (Keep the workspace
  `package.json` versions roughly in sync for sanity, but they no
  longer gate the release.)
- **The updater endpoint is stamped from `vars.UPDATER_PUBLIC_BASE`**
  so the binary's endpoint and the S3 publish location can never
  drift. The committed endpoint in `tauri.conf.json` is the real
  channel and acts as the fallback when the variable is unset.
- **tauri-action signs but does not notarize.** The `APPLE_API_*`
  secrets are withheld from the build step; submission happens
  `--no-wait` right after, and stapling/publication runs on cheap
  runners once Apple accepts.
- **The Go sidecar is codesigned before `tauri build`.** Tauri does
  not sign `externalBin` binaries (tauri-apps/tauri#11992); an
  unsigned Mach-O inside the bundle would fail notarization.
- **S3 publishing is OIDC-only.** No AWS keys are stored; CI assumes
  the `trove-updates-publisher` role provisioned by
  [`terraform/`](../terraform/README.md), scoped to `PutObject` on
  `stable/*`.

---

## One-time infrastructure setup

1. **Provision the S3 channel** (operator runs, needs AWS admin
   credentials):

   ```bash
   cd terraform
   terraform init
   terraform plan    # set create_github_oidc_provider = true in
                     # terraform.tfvars ONLY if plan says the GitHub
                     # OIDC provider is missing from the account
   terraform apply
   terraform output -raw gh_cli_setup | sh
   ```

   The last command sets the four repo **variables**: `S3_BUCKET`,
   `AWS_REGION`, `UPDATER_PUBLIC_BASE`, `AWS_ROLE_ARN`. Until they are
   set, `publish-updates` is skipped and the GitHub release is the only
   channel (in-app updates won't resolve).

2. **Generate the updater key pair** (once):

   ```bash
   pnpm --filter @trove/app exec tauri signer generate -- -w ~/.tauri/trove.key
   # Writes the private key to ~/.tauri/trove.key; prints the matching
   # public-key string to stdout.
   ```

   - Paste the **public key** into `packages/app/src-tauri/tauri.conf.json`
     `plugins.updater.pubkey` (replacing the
     `REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY` placeholder) and commit it.
   - Store the **private key** file contents as the
     `TAURI_SIGNING_PRIVATE_KEY` secret and its passphrase as
     `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

3. **Set the secrets** (repo settings → Secrets and variables →
   Actions → Secrets); see the tables below.

---

## Required secrets

### Updater manifest signing (always required)

| Secret                               | Purpose                                                                                                                   |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------- |
| `TAURI_SIGNING_PRIVATE_KEY`          | Signs the auto-updater artifacts (`*.app.tar.gz.sig` etc. and `latest.json`). Matching pubkey lives in `tauri.conf.json`. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Decrypts the private key at signing time.                                                                                 |

The in-app updater verifies every fetched manifest + artifact against
the committed pubkey; a mismatch surfaces as
`IpcError::UpdaterCheckFailed` in the UI and refuses the install.

### macOS code-signing + notarization (mandatory on macOS legs)

| Secret                       | Purpose                                                                                                                  |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `APPLE_CERTIFICATE`          | Base64-encoded `.p12` of the Developer ID Application identity exported from Keychain Access (full chain).               |
| `APPLE_CERTIFICATE_PASSWORD` | Password supplied when exporting the `.p12`.                                                                             |
| `APPLE_SIGNING_IDENTITY`     | Common name of the cert, e.g. `Developer ID Application: Intevity (TEAMID1234)`.                                         |
| `APPLE_API_ISSUER`           | App Store Connect API **Issuer ID** (UUID; App Store Connect → Users and Access → Integrations → App Store Connect API). |
| `APPLE_API_KEY`              | App Store Connect API **Key ID** (not the key content).                                                                  |
| `APPLE_API_KEY_CONTENT`      | Base64-encoded `AuthKey_<KeyID>.p8` private key file.                                                                    |

> The pipeline **fails the macOS legs** if any of the six are missing —
> a macOS auto-update can only install a notarized bundle, so an
> unsigned macOS release should never ship. (The old
> `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID` app-specific-password
> triple is no longer used; notarization authenticates with the ASC API
> key, which is account-level and can be shared with claude-sentinel.)

### Windows Authenticode (optional, OIDC end to end)

Like the updater channel, **no client secret is stored**. When the six
`AZURE_*` repo **variables** below are set, the release pipeline
Authenticode-signs the Windows app `.exe`, NSIS `-setup.exe`, MSI, and
the embedded `trove-otelcol` sidecar with **Azure Trusted Signing** (via
Microsoft's `sign` tool); leave any unset and the Windows leg builds
**unsigned** (exactly how a fork behaves). These live under
Settings → Secrets and variables → Actions → **Variables** (not Secrets —
they carry no secret material).

| Variable                | Purpose                                                                                           |
| ----------------------- | ------------------------------------------------------------------------------------------------- |
| `AZURE_CLIENT_ID`       | Entra app registration the release job authenticates as via OIDC (no secret).                     |
| `AZURE_TENANT_ID`       | Entra tenant id.                                                                                  |
| `AZURE_SUBSCRIPTION_ID` | Subscription that holds the signing account.                                                      |
| `AZURE_TS_ENDPOINT`     | Signing account URI, e.g. `https://eus.codesigning.azure.net/` (region-specific).                 |
| `AZURE_TS_ACCOUNT`      | Trusted Signing account name.                                                                     |
| `AZURE_TS_PROFILE`      | Public Trust certificate profile name (set **last**, after portal identity validation completes). |

The build job runs in a `release` GitHub Environment so its OIDC token
subject is `repo:<owner>/<repo>:environment:release`; the Entra app
registration behind `AZURE_CLIENT_ID` needs a **federated credential
scoped to exactly that subject for this repo**. Create the environment
once with `gh api -X PUT repos/<owner>/<repo>/environments/release`.
Windows artifacts also carry minisign signatures, so in-app updates work
whether or not Authenticode is configured.

### Generating the Apple secrets

```bash
# Developer ID Application cert: Keychain Access → right-click the
# identity → Export → .p12. The export password is
# APPLE_CERTIFICATE_PASSWORD.
base64 -i ~/Downloads/trove-developer-id.p12 | pbcopy   # → APPLE_CERTIFICATE

# App Store Connect API key: App Store Connect → Users and Access →
# Integrations → App Store Connect API → Generate (role: App Manager
# is sufficient for notarization). Note the Key ID (APPLE_API_KEY) and
# Issuer ID (APPLE_API_ISSUER); download AuthKey_<KeyID>.p8 (one-time).
base64 -i ~/Downloads/AuthKey_XXXXXXXXXX.p8 | pbcopy    # → APPLE_API_KEY_CONTENT
```

---

## Pre-release checklist

- [ ] Every PR in the sprint is merged and CI is green on `main`.
- [ ] `pnpm -r build` succeeds locally.
- [ ] `cargo build --release --manifest-path packages/app/src-tauri/Cargo.toml` succeeds.
- [ ] Smoke each Tier 1 + Tier 2 adapter: apply, dashboard turns green,
      revert, dashboard returns to neutral.

The version no longer needs to be hand-synced before tagging — CI
stamps `tauri.conf.json` + `Cargo.toml` from the tag.

---

## Tag procedure

```bash
git fetch origin
git checkout main
git pull --ff-only

git tag -s v0.6.0 -m "Trove v0.6.0"
git push origin v0.6.0
```

Watch the Actions tab:

- **Fast path** (Apple notarizes within ~20 min): the same run staples,
  publishes S3, and promotes the draft.
- **Slow path**: `notarize-wait` posts a `::notice::` and the run ends
  green with the release still in draft. The scheduled
  `notarize-poll.yml` (every ~30 min) finalizes it automatically once
  Apple accepts. No action needed.
- **Recovery**: if the S3 publish failed after stapling, fix the cause
  and run `notarize-finalize.yml` manually (Actions → Notarize finalize
  → Run workflow, with the tag).

If a single matrix entry fails, the draft stays in draft state — fix
the issue, delete the draft + tag, and re-tag.

---

## Bundle layout

Each successful matrix entry contributes files to the release (and,
post-finalize, to `s3://<bucket>/stable/<version>/`):

| Platform      | Files                                                                                                          |
| ------------- | -------------------------------------------------------------------------------------------------------------- |
| macOS (arm64) | `Trove_<version>_aarch64.dmg`, `Trove_aarch64.app.tar.gz` (+ `.sig`)                                           |
| macOS (x64)   | `Trove_<version>_x64.dmg`, `Trove_x64.app.tar.gz` (+ `.sig`)                                                   |
| Linux         | `trove_<version>_amd64.AppImage`, `.deb`, `.rpm` (+ `.sig` files)                                              |
| Windows       | `Trove_<version>_x64-setup.exe` (NSIS), `Trove_<version>_x64_en-US.msi` (+ `.sig` files)                       |
| Updater       | `latest.json` — written to S3 by `publish-updates` (the release-attached copy is deleted; S3 is authoritative) |

`scripts/assemble-latest-json.mjs` builds the manifest with
bundle-suffixed platform keys plus bare fallbacks and **fails loudly if
any expected artifact or `.sig` is missing**. Note the deliberate
divergence from tauri-action: the bare `windows-x86_64` key points at
the NSIS `-setup.exe` (Tauri's recommended updater installer), not the
MSI. Confirm exact artifact names against the first real release run if
the Tauri CLI is upgraded.

---

## Verification playbook

### macOS — signed + notarized bundle

```bash
hdiutil attach Trove_<version>_aarch64.dmg
cp -R "/Volumes/Trove/Trove.app" /Applications/
hdiutil detach "/Volumes/Trove"

# Expects: 'accepted' + 'source=Notarized Developer ID'.
spctl --assess --verbose=4 /Applications/Trove.app

# Staple ticket present (works offline):
xcrun stapler validate /Applications/Trove.app
```

If `spctl` reports `source=Unnotarized Developer ID`, check the
`notarize-wait` / `notarize-poll` logs and run
`xcrun notarytool log <submission-id>` against the staged API key.

### S3 channel

```bash
curl -s "https://intevity-trove-updates.s3.us-east-1.amazonaws.com/stable/latest.json" | jq .
# Expect: version == the new tag, platforms covering darwin-aarch64,
# darwin-x86_64, linux-x86_64, windows-x86_64 (+ bundle-suffixed keys),
# every URL under /stable/<version>/.
```

### Windows — Authenticode

Right-click the downloaded `.exe` → **Properties** → **Digital
Signatures** → confirm the chain validates (only once the `AZURE_*`
variables are configured). On a Windows box you can also run
`Get-AuthenticodeSignature .\Trove_<ver>_x64-setup.exe` (or
`signtool verify /pa /v <file>`) — the signer should chain to the Azure
Trusted Signing Public Trust certificate. The release workflow already
gates on this via its "Verify Windows Authenticode signatures" step.

### In-app auto-updater loop

1. Install the **previous** release from its signed bundle.
2. Cut the next tag and wait for finalize to publish S3.
3. Tray icon → **Check for updates…** — the main window comes forward
   and the update modal offers "Install and restart". Click it; the
   app restarts on the new version.
4. Background path: Settings → Updates → enable "Automatically check
   for updates", then relaunch with
   `TROVE_UPDATE_CHECK_INTERVAL_SECS=60`. Within ~2 minutes a native
   notification announces the version (once per version); the modal
   greets you the next time the window opens.
5. Negative check: on the latest version, the tray item fires a
   "You're on the latest version." notification, and the Settings
   button reports up-to-date inline.

### Updater pubkey mismatch (negative test)

If update checks fail with a signature error, the most common cause is
that `tauri.conf.json` `plugins.updater.pubkey` no longer matches
`TAURI_SIGNING_PRIVATE_KEY`. Regenerate the key pair
(`tauri signer generate`), update both the secret and the config, and
re-tag.
