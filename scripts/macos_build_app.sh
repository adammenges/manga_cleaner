#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="Manga Cleaner Native"
BUNDLE_ID="com.mangacleaner.native"
BIN_NAME="manga_cleaner_native"
DIST_DIR="$ROOT_DIR/dist"
APP_DIR="$DIST_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
OUT_ICNS="$ROOT_DIR/icon/manga_cleaner_native.icns"
APP_EXE_NAME="$APP_NAME"

"$ROOT_DIR/scripts/macos_make_icns.sh" "$ROOT_DIR/icon/base.png" "$OUT_ICNS"

cargo build --release --bin "$BIN_NAME" --manifest-path "$ROOT_DIR/Cargo.toml"

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

cp "$ROOT_DIR/target/release/$BIN_NAME" "$MACOS_DIR/$APP_EXE_NAME"
chmod +x "$MACOS_DIR/$APP_EXE_NAME"
cp "$OUT_ICNS" "$RESOURCES_DIR/"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>
  <string>$APP_EXE_NAME</string>
  <key>CFBundleIconFile</key>
  <string>manga_cleaner_native.icns</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

# Optional ad-hoc signing so Finder/Gatekeeper treat it more like a normal app bundle.
if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP_DIR" >/dev/null 2>&1 || true
fi

touch "$APP_DIR"

echo "[OK] App bundle created: $APP_DIR"
echo "[TIP] Launch with: open \"$APP_DIR\""
