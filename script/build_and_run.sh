#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-run}"
APP_NAME="DWNativeFixture"
APP_BUNDLE="$ROOT_DIR/bin/$APP_NAME.app"
APP_BINARY="$APP_BUNDLE/Contents/MacOS/$APP_NAME"
case "$MODE" in run|--verify|--debug|--logs|--telemetry|--build-only) ;; *) echo "usage: $0 [--build-only|--verify|--debug|--logs|--telemetry]" >&2; exit 2;; esac
if [[ "$MODE" != --build-only ]]; then pkill -x "$APP_NAME" >/dev/null 2>&1 || true; fi
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$ROOT_DIR/artifacts"
swiftc -module-cache-path "$ROOT_DIR/bin/swift-module-cache" -framework AppKit "$ROOT_DIR/tests/native-fixtures/macos/main.swift" -o "$APP_BINARY"
cat > "$APP_BUNDLE/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>DWNativeFixture</string>
<key>CFBundleIdentifier</key><string>dev.caelis.desktop-world.fixture</string>
<key>CFBundleName</key><string>DWNativeFixture</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
codesign --force --sign - "$APP_BUNDLE"
launch() { /usr/bin/open -n "$APP_BUNDLE" --args --title "${DW_FIXTURE_TITLE:-Desktop World Native Fixture}" --log "${DW_FIXTURE_LOG:-$ROOT_DIR/artifacts/native-fixture.jsonl}"; }
case "$MODE" in
 --build-only) ;;
 --debug) lldb -- "$APP_BINARY" ;;
 --logs|--telemetry) launch; /usr/bin/log stream --info --style compact --predicate 'process == "DWNativeFixture"' ;;
 --verify) launch; sleep 1; pgrep -x "$APP_NAME" >/dev/null ;;
 *) launch ;;
esac
