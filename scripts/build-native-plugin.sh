#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
destination="${1:?usage: build-native-plugin.sh DESTINATION}"
version="${2:-v0.1.0}"
[[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[1-9][0-9]*)?$ ]] || { echo 'invalid version' >&2; exit 1; }
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo 'native plugin build requires macOS arm64' >&2; exit 1; }
if [[ -d "$destination" && -n "$(find "$destination" -mindepth 1 -print -quit)" ]]; then
  echo "output directory must be empty: $destination" >&2
  exit 1
fi
mkdir -p "$destination/bin" "$destination/libexec" "$destination/skills/desktop-world"
destination="$(cd "$destination" && pwd)"
export GOWORK=off CGO_ENABLED=1
(cd runtime/native-go && go build -trimpath -ldflags "-X main.releaseVersion=$version" -o "$destination/bin/dtw" .)
go build -trimpath -tags 'dtw_background_poc dtw_virtual_input_poc dtw_core_noauth' -o "$destination/libexec/dtw-helper" ./cmd/dtw
cp packaging/plugin/plugin.json packaging/plugin/mcp.json "$destination/"
cp LICENSE "$destination/"
sed -i '' "s/\"version\": \"0.1.0\"/\"version\": \"${version#v}\"/" "$destination/plugin.json"
cp packaging/plugin/skills/desktop-world/SKILL.md "$destination/skills/desktop-world/"
"$destination/bin/dtw" version
