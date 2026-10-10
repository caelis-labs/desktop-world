#!/bin/sh
# Isolated POC fixtures. Building does not launch, focus, or move the pointer.
set -eu

source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
output_dir=${1:-"${TMPDIR:-/tmp}/dtw-focus-poc-apps"}
mkdir -p "$output_dir"
mkdir -p "$output_dir/ModuleCache"
export CLANG_MODULE_CACHE_PATH="$output_dir/ModuleCache"

for role in Target Human; do
  bundle="$output_dir/DTWFocus${role}.app"
  mkdir -p "$bundle/Contents/MacOS"
  swiftc -module-cache-path "$output_dir/ModuleCache" "$source_dir/FocusFixture.swift" -o "$bundle/Contents/MacOS/DTWFocus${role}"
  cat > "$bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>DTW Focus ${role}</string>
  <key>CFBundleDisplayName</key><string>DTW Focus ${role}</string>
  <key>CFBundleIdentifier</key><string>labs.caelis.dtw-focus-poc-${role}</string>
  <key>CFBundleExecutable</key><string>DTWFocus${role}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>1</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
  plutil -lint "$bundle/Contents/Info.plist"
  codesign --force --sign - "$bundle"
done
printf 'Built two distinct fixture bundles in %s\n' "$output_dir"
