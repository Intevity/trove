#!/usr/bin/env bash
#
# Build, install, and launch /Applications/Trove.app — the macOS
# implementation behind `pnpm build:app` (the cross-platform dispatcher
# scripts/build-app.mjs calls this on Darwin). Also runnable directly. This is
# the ONLY supported way to push a locally-built bundle into /Applications; do
# NOT `cp -R` over the existing app yourself (see "Why rm -rf first", below).
#
# Local dev builds skip the UPDATER key by design:
#   - The build below bundles only the `.app` and disables updater-artifact
#     creation (via src-tauri/tauri.dev.conf.json), so Tauri never asks for
#     the updater signing key password (the `~/.tauri/*.key` prompt you get
#     from a full `pnpm build:app:release`).
#   - The bundle is still code-signed with the local "Trove Dev" identity
#     (bundle.macOS.signingIdentity in tauri.conf.json) during the build, so
#     no re-sign step is needed after copying. Release builds (Developer ID
#     signed + notarized) go through CI.
#
# Why rm -rf first (instead of just `cp -R`):
#   1. `cp -R src.app /Applications/` when the destination ALREADY exists is a
#      MERGE, not a replace. macOS protects an installed app's existing files,
#      so copying over them fails with "cp: ... Operation not permitted" for
#      every file in the bundle. Removing the old app first means cp writes a
#      fresh tree with nothing to overwrite.
#   2. A merge also leaves stale files from the previous bundle (a renamed
#      binary, a removed Resource) lingering inside the app.
#
# This script is macOS-ONLY. Linux and Windows have no /Applications install
# step — build and run the bundle directly. See README → "Running a local
# build" for the per-OS dev loop.
#
# Usage: pnpm build:app   (cross-platform; runs this on macOS)
#    or: ./scripts/install-app.sh   (macOS only, direct)

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "✗ scripts/install-app.sh is macOS-only (it uses /Applications, codesign, open)." >&2
  echo "  Use the cross-platform entrypoint instead — it builds + runs per OS:" >&2
  echo "      pnpm build:app" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_PATH="/Applications/Trove.app"
BUNDLE_PATH="$REPO_ROOT/packages/app/src-tauri/target/release/bundle/macos/Trove.app"

cd "$REPO_ROOT"

# The running app holds its files open and macOS App Management will block the
# rm -rf below; a lingering `trove-otelcol` sidecar does the same. Rather than
# make you quit by hand (and risk a stale process silently surviving so `open`
# below just re-focuses the OLD binary instead of the freshly built one), quit
# gracefully and then escalate by PID until the bundle is fully idle. The
# pattern matches the installed app AND its sidecar via their bundle path — it
# does NOT match a `pnpm dev` vite server (whose command is a repo path, not
# `Trove.app/Contents/MacOS/`), so unrelated dev processes are never touched.
TROVE_PROC_PATTERN="Trove.app/Contents/MacOS/"
if pgrep -f "$TROVE_PROC_PATTERN" >/dev/null 2>&1; then
  echo "→ Trove is running; quitting it (app + sidecar) so the new build takes over..."
  osascript -e 'quit app "Trove"' >/dev/null 2>&1 || true
  # Graceful first, then SIGTERM, then SIGKILL — polling for exit between each.
  for sig in TERM TERM KILL; do
    for _ in 1 2 3 4 5 6; do
      pgrep -f "$TROVE_PROC_PATTERN" >/dev/null 2>&1 || break 2
      sleep 0.5
    done
    echo "  …still running; sending SIG$sig by PID"
    pkill -"$sig" -f "$TROVE_PROC_PATTERN" >/dev/null 2>&1 || true
  done
  if pgrep -f "$TROVE_PROC_PATTERN" >/dev/null 2>&1; then
    echo "✗ Could not stop the running Trove processes:" >&2
    pgrep -fl "$TROVE_PROC_PATTERN" >&2 || true
    echo "  Quit it from the tray / Activity Monitor, then re-run." >&2
    exit 1
  fi
  echo "  ✓ Trove stopped."
fi

echo "→ Building app bundle (this is the long step)..."
# Only the `.app`, updater artifacts disabled → no `~/.tauri/*.key` prompt.
pnpm --filter @trove/app exec tauri build \
  --bundles app --config src-tauri/tauri.dev.conf.json

if [[ ! -d "$BUNDLE_PATH" ]]; then
  echo "✗ Build produced no bundle at:" >&2
  echo "  $BUNDLE_PATH" >&2
  exit 1
fi

# Remove first, then copy fresh (see "Why rm -rf first" in the header).
if [[ -d "$APP_PATH" ]]; then
  echo "→ Removing existing $APP_PATH..."
  if ! rm -rf "$APP_PATH" 2>/dev/null; then
    echo "✗ Could not remove $APP_PATH (Operation not permitted)." >&2
    echo "  macOS is protecting the installed app. Either:" >&2
    echo "    - Confirm Trove is fully quit (tray + Activity Monitor), or" >&2
    echo "    - Grant your terminal 'App Management' under System Settings →" >&2
    echo "      Privacy & Security → App Management, then re-run." >&2
    exit 1
  fi
fi

echo "→ Copying bundle to $APP_PATH..."
cp -R "$BUNDLE_PATH" "$APP_PATH"

echo "→ Verifying signature..."
codesign --verify --verbose=1 "$APP_PATH"

echo "→ Launching..."
open "$APP_PATH"

echo
echo "✓ Installed and launched (local dev build)."
