#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="${1:?usage: package-prerelease.sh vX.Y.Z-prerelease}"
[[ "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-[a-zA-Z0-9.-]+$ ]] || { echo 'prerelease version required' >&2; exit 1; }
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo 'build on macOS arm64' >&2; exit 1; }
[[ -z "$(git status --porcelain)" ]] || { echo 'clean checkout required' >&2; exit 1; }
REVISION="$(git rev-parse HEAD)"
if git rev-parse --verify "refs/tags/$VERSION" >/dev/null 2>&1; then
  [[ "$(git rev-parse "$VERSION^{commit}")" == "$REVISION" ]] || { echo 'tag differs from HEAD' >&2; exit 1; }
fi
NAME="desktop-world-${VERSION}-darwin-arm64"
OUT="$PWD/artifacts/release/$VERSION"
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT" >&2; exit 1; }
mkdir -p "$OUT/$NAME/bin" "$OUT/$NAME/source"
export GOWORK=off CGO_ENABLED=1 GOOS=darwin GOARCH=arm64
export GOCACHE="${GOCACHE:-${TMPDIR:-/tmp}/desktop-world-go-cache}"
export CGO_CFLAGS='-mmacosx-version-min=14.0'
export CGO_LDFLAGS='-mmacosx-version-min=14.0'
go build -trimpath -buildvcs=true -ldflags "-X main.releaseVersion=$VERSION" -o "$OUT/$NAME/bin/dtw" ./cmd/dtw
codesign --force --sign - "$OUT/$NAME/bin/dtw"
codesign --verify --strict "$OUT/$NAME/bin/dtw"
"$OUT/$NAME/bin/dtw" version > "$OUT/$NAME/manifest.json"
python3 - "$OUT/$NAME/manifest.json" "$VERSION" "$REVISION" <<'PY'
import json, sys
p, version, revision = sys.argv[1:]
d = json.load(open(p))
assert d['version'] == version and d['vcs.revision'] == revision
assert d['vcs.modified'] == 'false' and d['os'] == 'darwin' and d['arch'] == 'arm64'
d.update(signing='ad-hoc', notarized=False, minimum_macos='14.0', license='MPL-2.0')
with open(p, 'w') as f: json.dump(d, f, indent=2); f.write('\n')
PY
git archive HEAD | tar -x -C "$OUT/$NAME/source"
cp README.md HANDOFF.md LICENSE NOTICE THIRD_PARTY_NOTICES.md "$OUT/$NAME/"
cp -R docs clients skills examples "$OUT/$NAME/"
tar -czf "$OUT/$NAME.tar.gz" -C "$OUT" "$NAME"
git archive --format=tar.gz --prefix="desktop-world-$VERSION/" -o "$OUT/desktop-world-$VERSION-source.tar.gz" HEAD
(cd "$OUT" && shasum -a 256 "$NAME.tar.gz" "desktop-world-$VERSION-source.tar.gz" > SHA256SUMS)
echo "$OUT"
