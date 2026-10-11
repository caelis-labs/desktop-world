#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
expected=ac967b9006aafa10870aadb5a645b038cba9721a
destination=${1:?usage: scripts/build-plugin.sh DESTINATION}
case "$destination" in
  /*) ;;
  *) destination=$PWD/$destination ;;
esac
destination=${destination%/}
if [ -z "$destination" ] || [ -e "$destination" ] || [ -L "$destination" ]; then
  echo "destination must be a new directory: $destination" >&2
  exit 2
fi

owned_source=
staging=
cleanup() {
  if [ -n "$staging" ] && [ -d "$staging" ]; then rm -rf -- "$staging"; fi
  if [ -n "$owned_source" ] && [ -d "$owned_source" ]; then rm -rf -- "$owned_source"; fi
}
trap cleanup 0

if [ -n "${CUA_SOURCE_ROOT:-}" ]; then
  source_root=$(CDPATH= cd -- "$CUA_SOURCE_ROOT" && pwd -P)
else
  if [ -n "${DTW_CUA_SOURCE_DIR:-}" ]; then
    if [ -e "$DTW_CUA_SOURCE_DIR" ] || [ -L "$DTW_CUA_SOURCE_DIR" ]; then
      echo "temporary CUA source path must be new: $DTW_CUA_SOURCE_DIR" >&2
      exit 2
    fi
    mkdir -p "$(dirname -- "$DTW_CUA_SOURCE_DIR")"
    mkdir -- "$DTW_CUA_SOURCE_DIR"
    owned_source=$DTW_CUA_SOURCE_DIR
  else
    owned_source=$(mktemp -d "${TMPDIR:-/private/tmp}/dtw-cua-source.XXXXXXXX")
  fi
  source_root=$owned_source
  git -C "$source_root" init -q
  git -C "$source_root" remote add origin https://github.com/trycua/cua.git
  git -C "$source_root" fetch -q --no-tags --depth 1 origin "$expected"
  git -C "$source_root" checkout -q --detach FETCH_HEAD
fi

actual=$(git -C "$source_root" rev-parse HEAD)
if [ "$actual" != "$expected" ] || [ -n "$(git -C "$source_root" status --porcelain)" ]; then
  echo "CUA source must be clean at $expected; got $actual" >&2
  exit 2
fi

target_dir=${DTW_BUILD_DIR:-${TMPDIR:-/private/tmp}/dtw-cua-build}
mkdir -p "$target_dir/dtw" "$target_dir/cua" "$(dirname -- "$destination")"
offline=
if [ "${DTW_CARGO_OFFLINE:-0}" = 1 ]; then offline=--offline; fi

# Cargo's git dependency and the private worker use the same checked commit.
# The redirect avoids a second fetch while retaining a portable pinned URL in
# Cargo.toml and Cargo.lock. It changes only this build process's git config.
CARGO_NET_GIT_FETCH_WITH_CLI=true \
GIT_CONFIG_COUNT=1 \
GIT_CONFIG_KEY_0="url.file://$source_root.insteadOf" \
GIT_CONFIG_VALUE_0=https://github.com/trycua/cua.git \
CARGO_TARGET_DIR="$target_dir/dtw" \
  cargo +1.98.0 build --manifest-path "$repo_root/cmd/dtw/Cargo.toml" --release --locked $offline

if [ -n "${CUA_DRIVER_BIN:-}" ]; then
  driver_binary=$CUA_DRIVER_BIN
  expected_binary_sha=${CUA_DRIVER_BIN_SHA256:?CUA_DRIVER_BIN_SHA256 is required with CUA_DRIVER_BIN}
  actual_binary_sha=$(shasum -a 256 "$driver_binary" | awk '{print $1}')
  if [ "$actual_binary_sha" != "$expected_binary_sha" ]; then
    echo "CUA worker SHA-256 mismatch: expected $expected_binary_sha, got $actual_binary_sha" >&2
    exit 2
  fi
else
  CARGO_TARGET_DIR="$target_dir/cua" \
    cargo +1.98.0 build --manifest-path "$source_root/libs/cua-driver/rust/Cargo.toml" \
      --bin cua-driver --release --locked $offline
  driver_binary=$target_dir/cua/release/cua-driver
fi

staging=$(mktemp -d "${destination}.stage.XXXXXXXX")
mkdir -p "$staging/bin" "$staging/libexec" "$staging/skills"
cp "$target_dir/dtw/release/dtw" "$staging/bin/dtw"
cp "$driver_binary" "$staging/libexec/cua-driver"
if [ "$(uname -s)" = Darwin ]; then
  # cmd/dtw/build.rs supplies the final Swift runtime rpath for CLI and tests.
  codesign --force --sign - "$staging/bin/dtw" >/dev/null
  codesign --force --sign - "$staging/libexec/cua-driver" >/dev/null
  codesign --verify "$staging/bin/dtw"
  codesign --verify "$staging/libexec/cua-driver"
fi
cp "$repo_root/packaging/plugin/plugin.json" "$repo_root/packaging/plugin/mcp.json" "$staging/"
cp -R "$repo_root/packaging/plugin/skills/desktop-world" "$staging/skills/desktop-world"
cp "$repo_root/LICENSE" "$repo_root/NOTICE" "$repo_root/THIRD_PARTY_NOTICES.md" "$staging/"
cp "$source_root/LICENSE.md" "$staging/CUA-MIT-LICENSE.txt"
cp "$source_root/libs/cua-driver/rust/THIRD_PARTY_NOTICES.md" "$staging/CUA-THIRD_PARTY_NOTICES.md"
python3 "$repo_root/scripts/license-audit.py" --cua-source "$source_root" --output "$staging/LICENSING"
python3 "$repo_root/tests/acceptance/licenses_probe.py" "$staging"
(cd "$staging" && find . -type f ! -name SHA256SUMS -exec shasum -a 256 {} + | LC_ALL=C sort > SHA256SUMS)
"$staging/bin/dtw" --version
mv -- "$staging" "$destination"
staging=
echo "package: $destination"
