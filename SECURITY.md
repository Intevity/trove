# Security policy

Trove sits on a developer's machine, reads and writes a small set of
config files, holds an OS keychain entry per observability backend, and
forwards OTLP signals to whichever destination the user picked. This
document spells out the threat model so reviewers can verify Trove's
behaviour against the **never phone home, never overwrite silently**
guarantees the project is built on.

If you find a security issue, please **do not open a public GitHub
issue**. See [Reporting a vulnerability](#reporting-a-vulnerability) at
the bottom.

---

## What Trove can see

Trove reads only what it needs to do its job. Every read is bounded to
the user's home directory (or platform equivalent); Trove never walks
arbitrary directories and never reads source code, project files,
prompts, or `git` data.

| Surface             | What's read                                                                                                            | Where                                                                                                                                                             |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Harness configs     | The harness's existing settings file, to compute a diff and to detect Trove's own managed region                       | `~/.claude/settings.json`, `~/.gemini/settings.json`, `~/.codex/config.toml`, `~/.qwen/settings.json`, `~/.config/opencode/opencode.json`, `~/.cursor/hooks.json` |
| VSCode extensions   | Cline's `globalStorage/saoudrizwan.claude-dev/tasks/<task>/ui_messages.json` (read-only, log-watch adapter only)       | `~/Library/Application Support/Code/User/...` (macOS), `~/.config/Code/User/...` (Linux), `%APPDATA%\Code\User\...` (Windows)                                     |
| Detection probes    | Existence/version of harness binaries on `PATH`; presence of standard config dirs; presence of known macOS app bundles | `which claude`, `which gemini`, etc.; directory `stat` only                                                                                                       |
| Trove state         | The app's own `state.json` (no secrets — see below)                                                                    | `~/.config/trove/state.json` on Linux; platform equivalents on macOS/Windows                                                                                      |
| Keychain            | Backend credentials Trove stored there itself                                                                          | OS keychain (macOS Keychain, libsecret, Windows Credential Manager)                                                                                               |
| Collector telemetry | The bundled collector's own Prometheus endpoint on `127.0.0.1:8888`                                                    | Loopback only                                                                                                                                                     |
| Collector log       | `collector.log` to render in the dashboard's Logs tab                                                                  | `~/.local/state/trove/collector.log` on Linux; platform equivalents                                                                                               |

Process-snooping detection (watching for harness binaries by name) is
**off by default** because it triggers a permissions dialog on macOS.
Users can opt into it; the toggle states the consequences plainly.

---

## What Trove writes

Every write is **atomic** (temp file + `fsync` + rename) and **wrapped
in sentinel comments** (or a `_trove` top-level key for pure JSON), so
Trove can find and remove its own region cleanly on disable. Permissions
are preserved across writes — a `0600` config stays `0600`. A backup is
copied to `<file>.trove.bak.<timestamp>` before the first edit, with the
last N retained.

| Path                                                           | Why Trove writes there                                                                                                                                 | Reversible?                                        |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------- |
| `~/.claude/settings.json` (`env` block)                        | Point Claude Code at `127.0.0.1:4318`                                                                                                                  | Yes — one click                                    |
| `~/.gemini/settings.json` (`telemetry` object)                 | Point Gemini CLI at the local collector                                                                                                                | Yes                                                |
| `~/.codex/config.toml` (`[otel]` table)                        | Point OpenAI Codex CLI at the local collector                                                                                                          | Yes                                                |
| `~/.qwen/settings.json`                                        | Point Qwen Code at the local collector                                                                                                                 | Yes                                                |
| `~/.config/opencode/opencode.json` (+ plugin dir)              | Register Trove's vendored OpenCode plugin                                                                                                              | Yes                                                |
| `~/.cursor/hooks.json` (+ vendored hook script)                | Wire Cursor IDE/CLI hooks to emit OTLP                                                                                                                 | Yes                                                |
| Shell rc (`~/.zshrc` / `~/.bashrc`, sentinel-bracketed export) | Aider — set `OTEL_*` env vars and `PATH` for the `trove-aider` wrapper                                                                                 | Yes                                                |
| `trove-aider`, `trove-copilot` wrappers                        | Shipped under Trove's app data dir, added to `PATH` via the rc edit above                                                                              | Yes                                                |
| `~/.config/trove/state.json`                                   | Persist user choices: backend reference, per-harness configs, collector port assignments. **No secrets — only `SecretRef` handles into the keychain.** | Yes — wipe via "Reset wizard"                      |
| OS keychain entry per backend                                  | The actual ingestion key, API token, or auth header value                                                                                              | Yes — wipe via "Disable backend" or "Reset wizard" |
| `<app-data>/collector.yaml`                                    | Generated deterministically from state on every change                                                                                                 | Reload-on-change                                   |
| `~/.local/state/trove/collector.log` (or platform equivalent)  | Tee'd from the bundled collector's stdout/stderr; size-capped                                                                                          | Trove writes; user owns; safe to delete            |

Trove **does not** write to: source repos, project directories, system
config (`/etc`), other users' homes, or any file it didn't create or
detect on first run.

If a target file exists but cannot be parsed, Trove **refuses to
write** and surfaces the error rather than overwriting silently. If
the file's content hash has changed since the last Trove write **and**
the managed region has been edited, the conflict UI requires explicit
"keep mine / take Trove's / merge manually" before any write happens.

---

## What Trove sends, and where

Trove forwards OTLP signals to **the user's chosen backend, and
nowhere else**. There is no Trove-controlled telemetry endpoint — not
for crash reports, not for analytics, not for anything. The promise is
visibly enforced by the codebase:

- The bundled collector's exporter list is generated from the active
  backend preset only. There is no `otlp/trove`, `analytics/*`, or
  similar exporter anywhere in the build.
- The Tauri-side Rust does not link any telemetry SDK.
- The TypeScript side does not import a tracking script and the
  `Content-Security-Policy` in the WebView denies external `connect-src`.

PII redaction is on by default. The collector pipeline drops the
`user.prompt` and `prompt.text` attributes from every signal before
export, in addition to whatever each harness redacts at source.

`OTEL_LOG_USER_PROMPTS` (or its per-harness equivalent) defaults to
**off** in every adapter Trove ships, matching upstream defaults. The
toggle to enable prompt logging is **per-harness, not global**, and
the UI labels it as a sensitive operation.

---

## How to revoke

| To…                                   | Do this                                                                                                                                                                                                                                                                       |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Stop forwarding from a single harness | Open Trove → toggle the harness off. The adapter reverts its config block byte-for-identically to the pre-Trove state, removes the sentinel, and unsets shell-rc env vars where applicable.                                                                                   |
| Rotate a backend credential           | Open Trove → Backend → Change. Old keychain entries for that backend are deleted; the collector reloads with the new one.                                                                                                                                                     |
| Wipe Trove from the machine           | Trove → Settings → Reset (clears keychain entries, deletes `state.json`, removes wrapper scripts and shell-rc edits) **and** uninstall the app via the OS's standard uninstaller (removes the bundled sidecar binary).                                                        |
| Verify nothing was missed             | Each adapter's revert is byte-identical to the original file — verifiable with `diff` against the `.trove.bak.<timestamp>` backups Trove leaves in place. Backups are pruned after N apply/revert cycles; an unrevertable adapter would leave an audit trail in the meantime. |

---

## Auto-update posture

The Tauri auto-updater is **opt-in** and **off by default**. The
toggle in Settings is clearly labelled. When enabled, the updater
downloads from the Trove project's GitHub Releases and verifies each
release manifest against the embedded public key generated from
`TAURI_SIGNING_PRIVATE_KEY` (held only in CI secrets, not in the repo
or any developer's machine). The signed manifest's path is the only
route by which Trove will install code over itself; an attacker would
need to compromise both the GitHub Release artifacts _and_ the signing
key to push a malicious update.

With the updater off, Trove makes no outbound connection on launch.
It works fully offline.

---

## Reporting a vulnerability

Email the maintainer at **`jeff.wooden@intevity.com`** with:

1. A description of the issue and the affected version.
2. Steps to reproduce, ideally including the redacted log excerpt or
   config-file content needed to trigger it.
3. Your suggested impact assessment (read of files outside the
   documented scope? credential exfiltration? remote code execution?).

Response within **5 business days**. Confirmed issues get a fix on
`main` and a backport to the most recent signed release. We'll
coordinate disclosure timing with the reporter; the default is "fix
shipped, then 14-day window, then public credit + advisory" unless
the reporter prefers otherwise.

PGP welcome but not required — request the public key in the first
email if you'd like to use it.

---

## Out of scope

The following are **explicitly not** part of Trove's behaviour, and any
report that one is happening is itself a security issue:

- **No telephony of any kind to a Trove-controlled endpoint.** Not
  metrics, not crash reports, not anonymous usage stats, not
  "phone-home for license activation". Ever.
- **No analytics SDKs**, no Sentry, no Mixpanel, no Segment, no
  Bugsnag, no Datadog browser RUM. The WebView's CSP denies
  third-party `connect-src`.
- **No remote configuration.** Trove's behaviour is determined entirely
  by the on-disk `state.json`, the collector YAML it generates, and
  the bundled collector binary. There is no remote feature flag.
- **No bundled binaries beyond `trove-otelcol`** (the vendored OpenTelemetry
  Collector built from `resources/otelcol/manifest.yaml`) and the small
  vendored hook scripts under `resources/hooks/` and `resources/wrappers/`.
- **No scope creep into other applications' data.** Trove never reads
  the contents of source-code repositories, browser history, mail,
  password managers, or files outside the documented surfaces.

If you observe Trove doing any of the above, report it as a
vulnerability via the email address above and we will treat it as
top-priority.
