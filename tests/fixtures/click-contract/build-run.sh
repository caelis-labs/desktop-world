#!/bin/sh
set -eu
root=${1:?usage: build-run.sh NEW_TEMP_DIRECTORY}
[ ! -e "$root" ] || { echo 'Use a new directory' >&2; exit 2; }
app="$root/DTWClickFixture.app"
mkdir -p "$app/Contents/MacOS"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
xcrun swiftc -module-cache-path "${TMPDIR:-/private/tmp}/dtw-cua-swift-cache" \
  "$script_dir/main.swift" -o "$app/Contents/MacOS/DTWClickFixture"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>DTWClickFixture</string><key>CFBundleIdentifier</key><string>dev.dtw.issue41-fixture</string><key>CFBundlePackageType</key><string>APPL</string><key>NSPrincipalClass</key><string>NSApplication</string></dict></plist>
PLIST
codesign --force --sign - "$app"
open -n "$app" --args "$root"
echo "$root/state.json"
