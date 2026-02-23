#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE_PNG="${1:-$ROOT_DIR/icon/base.png}"
OUT_ICNS="${2:-$ROOT_DIR/icon/manga_cleaner_native.icns}"

if [[ ! -f "$BASE_PNG" ]]; then
  echo "[ERROR] Missing base icon PNG: $BASE_PNG" >&2
  exit 2
fi

if ! command -v sips >/dev/null 2>&1; then
  echo "[ERROR] sips is required (macOS)." >&2
  exit 2
fi

if ! command -v iconutil >/dev/null 2>&1; then
  echo "[ERROR] iconutil is required (macOS)." >&2
  exit 2
fi

ICONSET_DIR="$(mktemp -d /tmp/manga_iconset.XXXXXX).iconset"
mkdir -p "$ICONSET_DIR"

write_icon() {
  local px="$1"
  local name="$2"
  sips -s format png -z "$px" "$px" "$BASE_PNG" --out "$ICONSET_DIR/$name" >/dev/null
}

# Required iconset sizes for macOS .icns
write_icon 16   icon_16x16.png
write_icon 32   icon_16x16@2x.png
write_icon 32   icon_32x32.png
write_icon 64   icon_32x32@2x.png
write_icon 128  icon_128x128.png
write_icon 256  icon_128x128@2x.png
write_icon 256  icon_256x256.png
write_icon 512  icon_256x256@2x.png
write_icon 512  icon_512x512.png
write_icon 1024 icon_512x512@2x.png

mkdir -p "$(dirname "$OUT_ICNS")"
iconutil -c icns "$ICONSET_DIR" -o "$OUT_ICNS"
rm -rf "$ICONSET_DIR"

echo "[OK] Wrote icon: $OUT_ICNS"
