#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."
mkdir -p bin/DTWFullFixture.app/Contents/MacOS
swiftc -module-cache-path bin/swift-module-cache -framework AppKit -framework WebKit poc/background-input/FullFixture.swift -o bin/DTWFullFixture.app/Contents/MacOS/DTWFullFixture
cat > bin/DTWFullFixture.app/Contents/Info.plist <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>DTWFullFixture</string><key>CFBundleIdentifier</key><string>dev.caelis.desktop-world.full-poc</string><key>CFBundleName</key><string>DTWFullFixture</string><key>CFBundlePackageType</key><string>APPL</string><key>NSPrincipalClass</key><string>NSApplication</string></dict></plist>
PLIST
codesign --force --sign - bin/DTWFullFixture.app
