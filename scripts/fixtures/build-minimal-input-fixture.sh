#!/bin/sh
# Build only. Launch is a separate, explicit step so the user's focus is not
# changed while compiling the minimal keyboard-control fixture.
set -eu
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
fixture_dir="$source_dir/../../testdata/appkit"
output_dir=${1:-"${TMPDIR:-/tmp}/dtw-minimal-input-poc"}
variant=${2:-plain}
case "$variant" in
  plain) executable=DTWMinimalInput; bundle_id=labs.caelis.dtw-minimal-input-poc; swift_flag= ;;
  event) executable=DTWEventInput; bundle_id=labs.caelis.dtw-event-input-poc; swift_flag='-D EVENT_PROBE' ;;
  deferred) executable=DTWDeferredEventInput; bundle_id=labs.caelis.dtw-deferred-event-input-poc; swift_flag='-D DEFERRED_EVENT_PROBE' ;;
  subclass) executable=DTWSubclassInput; bundle_id=labs.caelis.dtw-subclass-input-poc; swift_flag='-D SUBCLASS_ONLY' ;;
  override) executable=DTWOverrideInput; bundle_id=labs.caelis.dtw-override-input-poc; swift_flag='-D OVERRIDE_ONLY' ;;
  *) printf 'variant must be plain, subclass, override, event, or deferred\n' >&2; exit 2 ;;
esac
bundle="$output_dir/$executable.app"
mkdir -p "$bundle/Contents/MacOS" "$output_dir/ModuleCache"
export CLANG_MODULE_CACHE_PATH="$output_dir/ModuleCache"
swiftc -module-cache-path "$output_dir/ModuleCache" $swift_flag "$fixture_dir/MinimalInputFixture.swift" -o "$bundle/Contents/MacOS/$executable"
cat > "$bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$executable</string>
  <key>CFBundleIdentifier</key><string>$bundle_id</string>
  <key>CFBundleExecutable</key><string>$executable</string>
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
