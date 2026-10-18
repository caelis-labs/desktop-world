#!/bin/sh
# Release-only. Sign the worker before embedding it in dtw, then notarize both
# final Mach-O files. The submitted ZIP is evidence, not the download asset.
set -eu
umask 077

package=${1:?usage: sign-notarize-macos.sh PACKAGE EVIDENCE_JSON}
evidence=${2:?usage: sign-notarize-macos.sh PACKAGE EVIDENCE_JSON}
: "${APPLE_DEVELOPER_ID_P12_BASE64:?missing Developer ID certificate}"
: "${APPLE_DEVELOPER_ID_P12_PASSWORD:?missing certificate password}"
: "${APPLE_NOTARY_APPLE_ID:?missing Notary Apple ID}"
: "${APPLE_NOTARY_PASSWORD:?missing Notary app-specific password}"
: "${APPLE_TEAM_ID:?missing Apple Team ID}"

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/dtw-notary.XXXXXXXX")
keychain="$work/signing.keychain-db"
cleanup() {
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -rf -- "$work"
}
trap cleanup EXIT HUP INT TERM

printf '%s' "$APPLE_DEVELOPER_ID_P12_BASE64" | base64 -D > "$work/identity.p12"
keychain_password=$(uuidgen)
security create-keychain -p "$keychain_password" "$keychain" >/dev/null
security unlock-keychain -p "$keychain_password" "$keychain" >/dev/null
security set-keychain-settings -lut 21600 "$keychain"
security import "$work/identity.p12" -k "$keychain" -P "$APPLE_DEVELOPER_ID_P12_PASSWORD" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null
# codesign still consults the user keychain search list when resolving a SHA-1
# identity, even with --keychain. The runner is ephemeral and this keychain is
# removed by the cleanup trap.
security list-keychains -d user -s "$keychain"
identity=$(security find-identity -v -p codesigning "$keychain" | grep 'Developer ID Application:' | grep -Eo '[A-F0-9]{40}' | head -1 || true)
if [ -z "$identity" ]; then
  echo 'Developer ID signing identity not found in uploaded certificate' >&2
  exit 2
fi

export DTW_SIGN_IDENTITY=$identity DTW_SIGN_KEYCHAIN=$keychain
"$repo_root/scripts/build-plugin.sh" "$package"

mkdir -p "$work/submission" "$work/standalone" "$(dirname "$evidence")" "$work/home"
for binary in "$package/bin/dtw" "$package/libexec/cua-driver" "${package}-slim/bin/dtw"; do
  xcrun vtool -show-build "$binary" | python3 -c 'import re,sys; values=re.findall(r"minos\s+(\S+)",sys.stdin.read()); assert values and all(v in ("14.0", "14.0.0") for v in values), values'
  codesign --verify --strict --verbose=2 "$binary"
  details=$(codesign -dv --verbose=4 "$binary" 2>&1)
  printf '%s\n' "$details" | grep -q 'flags=.*runtime'
  printf '%s\n' "$details" | grep -q '^Timestamp='
  team=$(printf '%s\n' "$details" | sed -n 's/^TeamIdentifier=//p')
  if [ "$team" != "$APPLE_TEAM_ID" ]; then
    echo "signed binary has wrong Team ID: $binary" >&2
    exit 2
  fi
done
cp "${package}-slim/bin/dtw" "$work/submission/dtw-slim"
cp "$package/bin/dtw" "$work/submission/dtw"
cp "$package/libexec/cua-driver" "$work/submission/cua-driver"
cp "$package/bin/dtw" "$work/standalone/dtw"
ditto -c -k --sequesterRsrc --keepParent "$work/submission" "$work/submission.zip"
xcrun notarytool submit "$work/submission.zip" --apple-id "$APPLE_NOTARY_APPLE_ID" --password "$APPLE_NOTARY_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait --output-format json > "$work/notary.json"
python3 - "$work/notary.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1],encoding='utf-8'))
if v.get('status') != 'Accepted' or not v.get('id'):
    raise SystemExit(f"notarization did not accept artifact: {v.get('status', 'missing status')}")
PY

# The standalone download expands the exact worker submitted for notarization.
HOME="$work/home" USERPROFILE="$work/home" "$work/standalone/dtw" install plugin --to "$work/expanded" > "$work/install.json"
cmp "$package/libexec/cua-driver" "$work/expanded/libexec/cua-driver"
cmp "$package/bin/dtw" "$work/expanded/bin/dtw"
codesign --verify --strict --verbose=2 "$work/expanded/bin/dtw"
codesign --verify --strict --verbose=2 "$work/expanded/libexec/cua-driver"
codesign -vvvv -R="notarized" --check-notarization "$work/standalone/dtw"
codesign -vvvv -R="notarized" --check-notarization "$work/expanded/libexec/cua-driver"
codesign -vvvv -R="notarized" --check-notarization "${package}-slim/bin/dtw"
python3 - "$work/notary.json" "$evidence" "$APPLE_TEAM_ID" <<'PY'
import json,sys
v=json.load(open(sys.argv[1],encoding='utf-8'))
e={'submission_id':v['id'],'status':v['status'],'team_id':sys.argv[3],
   'signed_binaries':['bin/dtw','libexec/cua-driver'],
   'deployment_target':'14.0','slim_runtime_notarized':True,'hardened_runtime':True,'embedded_worker_matches_signed_worker':True,
   'notarization_ticket_dtw':'PASS','notarization_ticket_extracted_worker':'PASS',
   'ticket_delivery':'online; standalone Mach-O tickets cannot be stapled'}
with open(sys.argv[2],'w',encoding='utf-8') as f:
    json.dump(e,f,indent=2)
    f.write('\n')
PY
