#!/bin/sh
set -eu
root=${1:?usage: build-run.sh NEW_TEMP_DIRECTORY}
[ ! -e "$root" ] || { echo 'Use a new directory' >&2; exit 2; }
mkdir -p "$root/DTWSceneFixture.app/Contents/MacOS"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
xcrun swiftc -module-cache-path "$root/swift-cache" "$script_dir/main.swift" -o "$root/DTWSceneFixture.app/Contents/MacOS/DTWSceneFixture"
cat > "$root/DTWSceneFixture.app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>DTWSceneFixture</string><key>CFBundleIdentifier</key><string>dev.dtw.scene-fixture</string><key>CFBundlePackageType</key><string>APPL</string><key>NSPrincipalClass</key><string>NSApplication</string></dict></plist>
PLIST
open -n "$root/DTWSceneFixture.app" --args "$root"
echo "$root/state.json"
