#!/usr/bin/env bash
#
# Rebuilds packages/app/src-tauri/icons/icon.ico (the Windows bundle icon)
# from the shipped 512px master icons/icon.png.
#
# Why not `tauri icon`? Two reasons:
#   1. It emits an icon.ico with only 16/24/32/48/64/256 frames. Windows
#      Explorer renders desktop icons at 60-144 px under common DPI scales
#      (125 %/150 %) and upscales the nearest smaller frame when no better
#      one exists -> pixelated shortcut icons. We include the intermediate
#      frames (40/72/96/128) too.
#   2. CAUTION: assets/troveIcon.png (the input the 90127f2 commit message
#      names) is the dark 3-D variant and does NOT match the flat-teal
#      icons the app actually ships. Regenerating everything from it would
#      silently rebrand the app on all platforms. Until the brand source is
#      reconciled, icons/icon.png is the canonical flat-teal master and
#      only the .ico is derived.
#
# Frames are rendered as PNGs and packed by scripts/pack-ico.mjs (all
# PNG-compressed, the same layout the tauri CLI ships; ImageMagick's own
# BMP-in-ico encoder mangles the alpha channel).
#
# Prereqs: ImageMagick 7 (`magick`) and node in PATH.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ICONS_DIR="$ROOT/packages/app/src-tauri/icons"
MASTER="$ICONS_DIR/icon.png"

if ! command -v magick >/dev/null 2>&1; then
  echo "ERROR: ImageMagick (magick) not found in PATH." >&2
  exit 1
fi
if [[ ! -f "$MASTER" ]]; then
  echo "ERROR: master icon not found at $MASTER." >&2
  exit 1
fi

FRAME_DIR="$(mktemp -d -t trove-ico-frames)"
trap 'rm -rf "$FRAME_DIR"' EXIT

SIZES=(256 128 96 72 64 48 40 32 24 20 16)
FRAMES=()
for size in "${SIZES[@]}"; do
  frame="$FRAME_DIR/$size.png"
  magick "$MASTER" -resize "${size}x${size}" "$frame"
  FRAMES+=("$frame")
done

node "$ROOT/scripts/pack-ico.mjs" "$ICONS_DIR/icon.ico" "${FRAMES[@]}"

magick identify "$ICONS_DIR/icon.ico"
