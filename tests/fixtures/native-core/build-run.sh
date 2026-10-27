#!/bin/sh
set -eu
root=$1
if [ -e "$root" ]; then echo "Fixture output must be a new directory" >&2; exit 1; fi
mkdir -p "$root/DTWNativeFixture.app/Contents/MacOS"
cat > "$root/DTWNativeFixture.app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>DTWNativeFixture</string><key>CFBundleIdentifier</key><string>dev.dtw.native-fixture</string><key>CFBundleName</key><string>DTWNativeFixture</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>
PLIST
xcrun swiftc -module-cache-path "$root/swift-cache" "$(dirname "$0")/main.swift" -o "$root/DTWNativeFixture.app/Contents/MacOS/DTWNativeFixture"
open -n "$root/DTWNativeFixture.app" --args "$root"
python3 - "$root/state.json" <<'READY'
import pathlib, sys, time
state = pathlib.Path(sys.argv[1])
deadline = time.monotonic() + 10
while not state.exists():
    if time.monotonic() >= deadline:
        raise SystemExit("Fixture did not publish its startup state")
    time.sleep(0.02)
READY
echo "$root/state.json"
