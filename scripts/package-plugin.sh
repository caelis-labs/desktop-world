#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
version="${1:?usage: package-plugin.sh vX.Y.Z[-rc.N]}"
[[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[1-9][0-9]*)?$ ]] || { echo 'stable or numbered candidate version required' >&2; exit 1; }
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo 'native plugin packaging requires macOS arm64' >&2; exit 1; }
[[ -z "$(git status --porcelain)" ]] || { echo 'clean checkout required' >&2; exit 1; }
out="$PWD/artifacts/release/$version"
[[ ! -e "$out" ]] || { echo "output already exists: $out" >&2; exit 1; }
name="desktop-world-plugin-${version}-darwin-arm64"
mkdir -p "$out/$name"
./scripts/build-native-plugin.sh "$out/$name" "$version"
(cd "$out" && tar -czf "$name.tar.gz" "$name" && shasum -a 256 "$name.tar.gz" > SHA256SUMS)
echo "revision=$(git rev-parse HEAD) package=$out/$name.tar.gz"
