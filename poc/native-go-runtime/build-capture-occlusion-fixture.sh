#!/bin/sh
# Build only. Launch with LaunchOwnedFixture.swift so this test never activates
# the owned application as a side effect of an `open` command.
set -eu
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
output_dir=${1:-"${TMPDIR:-/tmp}/dtw-own-occlusion-poc"}
bundle="$output_dir/DTWOwnOcclusion.app"
mkdir -p "$bundle/Contents/MacOS" "$output_dir/ModuleCache"
export CLANG_MODULE_CACHE_PATH="$output_dir/ModuleCache"
swiftc -module-cache-path "$output_dir/ModuleCache" \
  "$source_dir/CaptureOcclusionFixture.swift" \
  -o "$bundle/Contents/MacOS/DTWOwnOcclusion"
cat > "$bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>DTWOwnOcclusion</string>
  <key>CFBundleIdentifier</key><string>labs.caelis.dtw-own-occlusion-poc</string>
  <key>CFBundleExecutable</key><string>DTWOwnOcclusion</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
EOF
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --sign - "$bundle"
printf 'Built %s\n' "$bundle"
