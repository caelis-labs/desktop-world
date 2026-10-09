#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="${1:?usage: package-plugin.sh vX.Y.Z}"
[[ "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'stable version required' >&2; exit 1; }
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo 'build on macOS arm64' >&2; exit 1; }
[[ -z "$(git status --porcelain)" ]] || { echo 'clean checkout required' >&2; exit 1; }
REVISION="$(git rev-parse HEAD)"
NAME="desktop-world-plugin-${VERSION}-darwin-arm64"
OUT="$PWD/artifacts/release/$VERSION"
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT" >&2; exit 1; }
mkdir -p "$OUT/staging" "$OUT/$NAME/bin"
export GOWORK=off CGO_ENABLED=1 GOOS=darwin GOARCH=arm64
export GOCACHE="${GOCACHE:-${TMPDIR:-/tmp}/desktop-world-plugin-go-cache}"
export CGO_CFLAGS='-mmacosx-version-min=14.0'
export CGO_LDFLAGS='-mmacosx-version-min=14.0'
go build -trimpath -buildvcs=true -ldflags "-X main.releaseVersion=$VERSION" -o "$OUT/staging/dtw" ./cmd/dtw
(cd clients/mcp && npm ci && npm run build)
NODE_VERSION=v24.21.0
NODE_ARCHIVE="node-$NODE_VERSION-darwin-arm64.tar.gz"
curl -fsSLo "$OUT/staging/$NODE_ARCHIVE" "https://nodejs.org/dist/$NODE_VERSION/$NODE_ARCHIVE"
curl -fsSLo "$OUT/staging/SHASUMS256.txt" "https://nodejs.org/dist/$NODE_VERSION/SHASUMS256.txt"
EXPECTED="$(awk -v name="$NODE_ARCHIVE" '$2 == name { print $1 }' "$OUT/staging/SHASUMS256.txt")"
ACTUAL="$(shasum -a 256 "$OUT/staging/$NODE_ARCHIVE" | awk '{print $1}')"
[[ -n "$EXPECTED" && "$EXPECTED" == "$ACTUAL" ]] || { echo 'official Node archive checksum mismatch' >&2; exit 1; }
tar -xzf "$OUT/staging/$NODE_ARCHIVE" -C "$OUT/staging"
NODE_ROOT="$OUT/staging/node-$NODE_VERSION-darwin-arm64"
node scripts/build-plugin-package.mjs "$VERSION" darwin-arm64 "$OUT/staging/dtw" "$NODE_ROOT/bin/node" "$NODE_ROOT/LICENSE" "$ACTUAL" "$OUT/$NAME"
node clients/mcp/verify-package.mjs "$OUT/$NAME"
tar -czf "$OUT/$NAME.tar.gz" -C "$OUT" "$NAME"
git archive --format=tar.gz --prefix="desktop-world-$VERSION/" -o "$OUT/desktop-world-$VERSION-source.tar.gz" HEAD
(cd "$OUT" && shasum -a 256 "$NAME.tar.gz" "desktop-world-$VERSION-source.tar.gz" > SHA256SUMS)
rm -rf "$OUT/staging"
echo "revision=$REVISION package=$OUT/$NAME.tar.gz"
