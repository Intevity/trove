#!/usr/bin/env bash
#
# Builds the trove-otelcol sidecar binary via the OpenTelemetry Collector
# Builder (ocb) for the current host platform (or the platform specified by
# TROVE_TARGET_TRIPLE / CARGO_BUILD_TARGET / TAURI_ENV_TARGET_TRIPLE).
#
# Output: resources/otelcol/dist/<triple>/trove-otelcol[.exe]
#
# Prereqs: Go 1.23+ in PATH (for `go install` of ocb if not already cached)
#          and rustc (only needed when no triple env var is set).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RES_DIR="$ROOT/resources/otelcol"
MANIFEST="$RES_DIR/manifest.yaml"
BUILD_DIR="$RES_DIR/_build"
DIST_ROOT="$RES_DIR/dist"

OCB_VERSION="v0.151.0"
OCB_PKG="go.opentelemetry.io/collector/cmd/builder@${OCB_VERSION}"

# --- Resolve target triple --------------------------------------------------
# Precedence: explicit override > cargo cross-compile > tauri cross-compile >
# host triple from rustc -vV.

TRIPLE="${TROVE_TARGET_TRIPLE:-${CARGO_BUILD_TARGET:-${TAURI_ENV_TARGET_TRIPLE:-}}}"
if [[ -z "$TRIPLE" ]]; then
  if ! command -v rustc >/dev/null 2>&1; then
    echo "ERROR: rustc not found; install Rust or set TROVE_TARGET_TRIPLE." >&2
    exit 1
  fi
  TRIPLE="$(rustc -vV | awk '/^host:/ { print $2 }')"
fi
if [[ -z "$TRIPLE" ]]; then
  echo "ERROR: failed to resolve target triple." >&2
  exit 1
fi

# --- Map Rust triple -> Go GOOS/GOARCH --------------------------------------

case "$TRIPLE" in
  aarch64-apple-darwin)      GOOS=darwin  GOARCH=arm64 ;;
  x86_64-apple-darwin)       GOOS=darwin  GOARCH=amd64 ;;
  x86_64-unknown-linux-gnu)  GOOS=linux   GOARCH=amd64 ;;
  aarch64-unknown-linux-gnu) GOOS=linux   GOARCH=arm64 ;;
  x86_64-pc-windows-msvc|x86_64-pc-windows-gnu) GOOS=windows GOARCH=amd64 ;;
  *)
    echo "ERROR: unsupported target triple: $TRIPLE" >&2
    exit 1
    ;;
esac

EXT=""
[[ "$GOOS" == "windows" ]] && EXT=".exe"

DIST_DIR="$DIST_ROOT/$TRIPLE"
DIST_BIN="$DIST_DIR/trove-otelcol${EXT}"

# --- Resolve ocb binary -----------------------------------------------------
# Order: explicit override (TROVE_OCB_BIN) > pre-installed `ocb` or `builder`
# in PATH > go install at the pinned version.

resolve_ocb() {
  if [[ -n "${TROVE_OCB_BIN:-}" && -x "${TROVE_OCB_BIN}" ]]; then
    echo "$TROVE_OCB_BIN"; return
  fi
  if command -v ocb >/dev/null 2>&1; then command -v ocb; return; fi
  if command -v builder >/dev/null 2>&1; then command -v builder; return; fi

  if ! command -v go >/dev/null 2>&1; then
    echo "ERROR: 'go' not found and ocb is not installed; install Go 1.23+ first." >&2
    return 1
  fi
  local gobin
  gobin="${GOBIN:-${GOPATH:-$HOME/go}/bin}"
  echo "[build-collector] installing ocb $OCB_VERSION into $gobin" >&2
  GO111MODULE=on go install "$OCB_PKG"
  echo "$gobin/builder"
}

OCB="$(resolve_ocb)"
if [[ ! -x "$OCB" ]]; then
  echo "ERROR: ocb binary not found or not executable: $OCB" >&2
  exit 1
fi

echo "[build-collector] target triple : $TRIPLE"
echo "[build-collector] GOOS / GOARCH : $GOOS / $GOARCH"
echo "[build-collector] ocb           : $OCB"
echo "[build-collector] manifest      : $MANIFEST"

# --- Build ------------------------------------------------------------------
# The manifest's `output_path: ./_build` resolves relative to the dir we
# invoke ocb from. We clean it first so a stale binary from a previous
# triple can't masquerade as the current one.

rm -rf "$BUILD_DIR"

(
  cd "$RES_DIR"
  GOOS="$GOOS" GOARCH="$GOARCH" CGO_ENABLED=0 \
    "$OCB" --config "$MANIFEST"
)

SRC_BIN="$BUILD_DIR/trove-otelcol${EXT}"
if [[ ! -x "$SRC_BIN" ]]; then
  echo "ERROR: builder produced no binary at $SRC_BIN" >&2
  ls -la "$BUILD_DIR" >&2 || true
  exit 1
fi

mkdir -p "$DIST_DIR"
mv -f "$SRC_BIN" "$DIST_BIN"
chmod +x "$DIST_BIN"

SIZE_BYTES="$(wc -c < "$DIST_BIN" | tr -d ' ')"
SIZE_MB="$(awk -v b="$SIZE_BYTES" 'BEGIN { printf "%.1f", b/1048576 }')"
echo "[build-collector] produced     : $DIST_BIN (${SIZE_MB} MB)"
