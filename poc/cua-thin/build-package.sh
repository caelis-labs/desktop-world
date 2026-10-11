#!/bin/sh
set -eu

source_root=${CUA_SOURCE_ROOT:-/Users/xueyongzhi/WorkDir/github/cua}
expected=ac967b9006aafa10870aadb5a645b038cba9721a
actual=$(git -C "$source_root" rev-parse HEAD)
if [ "$actual" != "$expected" ]; then
  echo "CUA source must be $expected, got $actual" >&2
  exit 2
fi
destination=${1:?usage: build-package.sh DESTINATION}
if [ -e "$destination" ]; then
  echo "destination exists: $destination" >&2
  exit 2
fi
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
target_dir=${DTW_CUA_TARGET_DIR:-/private/tmp/dtw-cua-thin-target}
CARGO_TARGET_DIR="$target_dir" cargo +1.98.0 build --manifest-path "$script_dir/Cargo.toml" --release --offline
if [ -n "${CUA_DRIVER_BIN:-}" ]; then
  driver_binary=$CUA_DRIVER_BIN
  expected_binary_sha=${CUA_DRIVER_BIN_SHA256:?CUA_DRIVER_BIN_SHA256 is required with CUA_DRIVER_BIN}
  actual_binary_sha=$(shasum -a 256 "$driver_binary" | awk '{print $1}')
  if [ "$actual_binary_sha" != "$expected_binary_sha" ]; then
    echo "CUA worker SHA-256 mismatch: expected $expected_binary_sha, got $actual_binary_sha" >&2
    exit 2
  fi
else
  driver_binary=$target_dir/release/cua-driver
  CARGO_TARGET_DIR="$target_dir" cargo +1.98.0 build --manifest-path "$source_root/libs/cua-driver/rust/Cargo.toml" --bin cua-driver --release --offline
fi
mkdir -p "$destination/bin" "$destination/libexec" "$destination/skills"
cp "$target_dir/release/dtw" "$destination/bin/dtw"
cp "$driver_binary" "$destination/libexec/cua-driver"
if [ "$(uname -s)" = Darwin ]; then
  # Rust's final link currently drops the Swift bridge rpath from transitive SDK crates.
  install_name_tool -add_rpath /usr/lib/swift "$destination/bin/dtw"
  codesign --force --sign - "$destination/bin/dtw" >/dev/null
fi
cp "$script_dir/package/plugin.json" "$script_dir/package/mcp.json" "$destination/"
cp -R "$script_dir/package/skills/desktop-world" "$destination/skills/desktop-world"
cp "$source_root/LICENSE.md" "$destination/CUA-MIT-LICENSE.txt"
cp "$script_dir/../../LICENSE" "$destination/DTW-MPL-LICENSE.txt"
cat > "$destination/THIRD_PARTY_NOTICES.md" <<EOF
# Third-party notices

CUA Driver SDK and private worker are from trycua/cua at commit $expected,
licensed MIT. The full notice is in CUA-MIT-LICENSE.txt.

DTW adapter source is Mozilla Public License 2.0; see DTW-MPL-LICENSE.txt.
Optional CUA perception/Spaces components are not included.
EOF
"$destination/bin/dtw" --version
echo "package: $destination"
