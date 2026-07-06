#!/usr/bin/env bash
# Assemble a double-clickable Colosseum.app from the release binary.
#
# A bare Unix executable opened from Finder always spawns a Terminal window —
# macOS has no equivalent of the Windows GUI-subsystem flag. Wrapping the
# binary in a minimal .app bundle is the supported way to launch without a
# console. The bundle is unsigned: it runs fine on the machine that built it,
# but other machines will show the Gatekeeper "unidentified developer" prompt
# unless it is codesigned/notarized.
#
# Usage:  ./build_macos.sh [--no-build]
# Output: dist/Colosseum.app

set -euo pipefail

cd "$(dirname "$0")"

if [[ "${1:-}" != "--no-build" ]]; then
  cargo build --release --bin colosseum
fi

VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
APP=dist/Colosseum.app
ICO=crates/colosseum-gui/assets/colosseum.ico

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp target/release/colosseum "$APP/Contents/MacOS/colosseum"

# Build the .icns from the shipped .ico (256 px master) with sips + iconutil.
ICONSET=$(mktemp -d)/colosseum.iconset
mkdir -p "$ICONSET"
MASTER=$(mktemp -t colosseum-icon).png
sips -s format png "$ICO" --out "$MASTER" >/dev/null
for size in 16 32 128 256; do
  sips -z "$size" "$size" "$MASTER" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$MASTER" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/colosseum.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>colosseum</string>
    <key>CFBundleIconFile</key>
    <string>colosseum</string>
    <key>CFBundleIdentifier</key>
    <string>gui.colosseum.Colosseum</string>
    <key>CFBundleName</key>
    <string>Colosseum</string>
    <key>CFBundleDisplayName</key>
    <string>Colosseum</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

# Ad-hoc signature: Apple Silicon refuses to run entirely unsigned binaries.
codesign --force --sign - "$APP" >/dev/null 2>&1 || true

echo "Built $APP (v$VERSION)"
