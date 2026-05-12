#!/bin/sh
# Trove's Cursor hook — Node-resolution shim.
#
# Cursor invokes this file on every beforeShellExecution / afterShellExecution
# event. The actual hook logic is JavaScript and lives in the sibling file
# `cursor-otel-hook-impl.cjs`; this wrapper exists because macOS GUI apps
# launched from the Dock or Spotlight inherit launchd's minimal PATH
# (`/usr/bin:/bin:/usr/sbin:/sbin`) — nothing in there resolves to `node`
# under a Homebrew / nvm / volta / fnm / asdf install. Without this shim,
# the impl's `#!/usr/bin/env node` shebang fails ENOENT and the event is
# silently dropped before any telemetry reaches Trove's collector.
#
# Resolution order (first hit wins):
#   1. node already on PATH (works when Cursor was launched from a terminal)
#   2. /opt/homebrew/bin/node, /usr/local/bin/node, /opt/local/bin/node, /usr/bin/node
#   3. nvm's most-recently-installed version under $HOME/.nvm/versions/node
#   4. volta — $HOME/.volta/bin/node
#   5. fnm — $HOME/.local/share/fnm/node-versions/*/installation/bin/node
#   6. ask the user's login shell to resolve `node` (slow but handles asdf
#      and any other version manager that puts node on PATH via rc files)
#
# Keep this file at the path the cursor adapter (cursor_common.rs::build_region)
# records in ~/.cursor/hooks.json. The .cjs extension is preserved for backward
# compatibility with the patches already on disk; the kernel reads the shebang
# below and runs /bin/sh regardless of the file's extension.
#
# `--health` short-circuits: the adapter's smoke-test only checks executability.

if [ "$1" = "--health" ]; then
  printf 'ok\n'
  exit 0
fi

SELF_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
IMPL="$SELF_DIR/cursor-otel-hook-impl.cjs"

# Diagnostic trace so we can confirm Cursor *does* invoke this wrapper —
# the file is the first thing we look at when "I don't see any cursor
# metrics in SigNoz" comes back. Best-effort: every step swallows IO
# errors with `|| true` so a missing log dir or read-only home cannot
# crash the hook. The log path mirrors the collector log directory
# (~/Library/Logs/com.intevity.trove on macOS).
TROVE_HOOK_LOG="${HOME:-/tmp}/Library/Logs/com.intevity.trove/cursor-hook-invocations.log"
mkdir -p "$(dirname "$TROVE_HOOK_LOG")" 2>/dev/null || true
trace() {
  printf '[%s] pid=%d phase=%s event=%s extra=%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$$" "${1:-?}" "${2:-?}" "${3:-}" \
    >> "$TROVE_HOOK_LOG" 2>/dev/null || true
}

trace entry "${EVENT_PEEK:-pre-stdin}" "args=$*"

# Buffer stdin once so we can both peek at the event name (for the no-node
# fallback's gate response) and forward it intact to the impl.
INPUT=$(cat)
EVENT=$(printf '%s' "$INPUT" | sed -n 's/.*"hook_event_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)
trace stdin-read "$EVENT" "bytes=$(printf '%s' "$INPUT" | wc -c | tr -d ' ')"

resolve_node() {
  if command -v node >/dev/null 2>&1; then
    command -v node
    return 0
  fi
  for candidate in /opt/homebrew/bin/node /usr/local/bin/node /opt/local/bin/node /usr/bin/node; do
    if [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  if [ -d "$HOME/.nvm/versions/node" ]; then
    NVM_NODE=$(ls -t "$HOME/.nvm/versions/node"/*/bin/node 2>/dev/null | head -1)
    if [ -n "$NVM_NODE" ] && [ -x "$NVM_NODE" ]; then
      printf '%s\n' "$NVM_NODE"
      return 0
    fi
  fi
  if [ -x "$HOME/.volta/bin/node" ]; then
    printf '%s\n' "$HOME/.volta/bin/node"
    return 0
  fi
  if [ -d "$HOME/.local/share/fnm/node-versions" ]; then
    FNM_NODE=$(ls -t "$HOME/.local/share/fnm/node-versions"/*/installation/bin/node 2>/dev/null | head -1)
    if [ -n "$FNM_NODE" ] && [ -x "$FNM_NODE" ]; then
      printf '%s\n' "$FNM_NODE"
      return 0
    fi
  fi
  if [ -n "$SHELL" ] && [ -x "$SHELL" ]; then
    LOGIN_NODE=$("$SHELL" -lc 'command -v node' 2>/dev/null | tr -d '\r' | head -1)
    if [ -n "$LOGIN_NODE" ] && [ -x "$LOGIN_NODE" ]; then
      printf '%s\n' "$LOGIN_NODE"
      return 0
    fi
  fi
  return 1
}

NODE_BIN=$(resolve_node) || {
  trace no-node-found "$EVENT" "PATH=$PATH"
  # No node anywhere on the system. Don't gate Cursor's shell: emit the
  # permissive response for beforeShellExecution; stay silent on after.
  if [ "$EVENT" = "beforeShellExecution" ]; then
    printf '{"permission":"allow"}\n'
  fi
  exit 0
}
trace node-resolved "$EVENT" "node=$NODE_BIN"

# Forward the buffered stdin to the JS impl. Use printf '%s' (not echo) so
# percent signs in command strings survive intact.
printf '%s' "$INPUT" | "$NODE_BIN" "$IMPL"
IMPL_RC=$?
trace impl-exit "$EVENT" "rc=$IMPL_RC"
exit $IMPL_RC
