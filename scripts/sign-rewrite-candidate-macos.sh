#!/bin/bash
# Explicit CI candidate construction. Never creates a release or retries notary.
# Disable inherited tracing before inspecting any secret-bearing environment.
set +x
set -euo pipefail
umask 077

if [[ $# != 1 ]]; then
  echo 'usage: sign-rewrite-candidate-macos.sh NEW_ABSOLUTE_OUTPUT' >&2
  exit 2
fi

dtw_missing=''
for dtw_name in APPLE_DEVELOPER_ID_P12_BASE64 APPLE_DEVELOPER_ID_P12_PASSWORD APPLE_NOTARY_APPLE_ID APPLE_NOTARY_PASSWORD APPLE_TEAM_ID; do
  if [[ -z ${!dtw_name:-} ]]; then dtw_missing="$dtw_missing $dtw_name"; fi
done
dtw_certificate=${APPLE_DEVELOPER_ID_P12_BASE64:-}
dtw_certificate_password=${APPLE_DEVELOPER_ID_P12_PASSWORD:-}
dtw_notary_user=${APPLE_NOTARY_APPLE_ID:-}
dtw_notary_password=${APPLE_NOTARY_PASSWORD:-}
dtw_team=${APPLE_TEAM_ID:-}
unset APPLE_DEVELOPER_ID_P12_BASE64 APPLE_DEVELOPER_ID_P12_PASSWORD APPLE_NOTARY_APPLE_ID APPLE_NOTARY_PASSWORD APPLE_TEAM_ID
export -n dtw_certificate dtw_certificate_password dtw_notary_user dtw_notary_password dtw_team

dtw_output=$1
if [[ $dtw_output != /* || ! -d $(dirname -- "$dtw_output") || -e $dtw_output || -L $dtw_output || -e ${dtw_output}.signing.json ]]; then
  echo 'Candidate output must be a new absolute path with an existing parent; prior evidence is never overwritten' >&2
  exit 2
fi
dtw_repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
dtw_work=''
dtw_keychain=''
dtw_identity=''
dtw_search_changed=0
dtw_builder_invocations=0
dtw_stage=preflight

dtw_cleanup() {
  local dtw_status=$? dtw_cleanup_failed=0 dtw_keychain_cleanup=not_created dtw_search_restore=not_changed dtw_private_cleanup=not_created
  trap - EXIT HUP INT TERM
  set +e
  if [[ -n $dtw_keychain && -e $dtw_keychain ]]; then
    if /usr/bin/security delete-keychain "$dtw_keychain" >/dev/null 2>&1; then
      dtw_keychain_cleanup=passed
    else
      dtw_keychain_cleanup=failed; dtw_cleanup_failed=1
    fi
  fi
  if [[ $dtw_search_changed == 1 ]]; then
    if python3 - "$dtw_work/keychains.json" <<'PY'
import json, shlex, subprocess, sys
paths = json.load(open(sys.argv[1], encoding='utf-8'))
subprocess.run(['/usr/bin/security','list-keychains','-d','user','-s',*paths],
               check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
actual = subprocess.check_output(['/usr/bin/security','list-keychains','-d','user'], stderr=subprocess.DEVNULL)
if shlex.split(actual.decode('utf-8')) != paths:
    raise SystemExit('Original keychain search list did not read back unchanged')
PY
    then dtw_search_restore=passed
    else dtw_search_restore=failed; dtw_cleanup_failed=1
    fi
  fi
  if [[ -n $dtw_work ]]; then
    rm -rf -- "$dtw_work"
    if [[ ! -e $dtw_work ]]; then dtw_private_cleanup=passed
    else dtw_private_cleanup=failed; dtw_cleanup_failed=1
    fi
  fi
  if [[ $dtw_cleanup_failed != 0 ]]; then
    echo 'Candidate signing cleanup failed; inspect the signing sidecar' >&2
    if [[ $dtw_status == 0 ]]; then dtw_status=1; fi
  fi
  python3 - "${dtw_output}.signing.json" "$dtw_status" "$dtw_stage" "$dtw_team" "$dtw_identity" \
    "$dtw_keychain_cleanup" "$dtw_search_restore" "$dtw_private_cleanup" "$dtw_builder_invocations" <<'PY'
import json, pathlib, sys
path, status, stage, team, identity, keychain, search, private, builds = sys.argv[1:]
record = {'kind':'unpublished-candidate-ci-signing', 'exit_code':int(status), 'last_stage':stage,
          'team_id':team or None, 'signing_identity_sha1':identity or None, 'builder_invocations':int(builds),
          'keychain_cleanup':keychain, 'original_search_list_restore':search,
          'private_keychain_and_p12_directory_cleanup':private, 'raw_credentials_exported_to_builder':False}
with pathlib.Path(path).open('x', encoding='utf-8') as stream:
    json.dump(record, stream, indent=2); stream.write('\n')
PY
  if [[ $? != 0 && $dtw_status == 0 ]]; then dtw_status=1; fi
  exit "$dtw_status"
}
trap dtw_cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

if [[ -n $dtw_missing ]]; then
  printf 'Missing candidate signing configuration:%s\n' "$dtw_missing" >&2
  exit 2
fi
if [[ ! $dtw_team =~ ^[A-Z0-9]{10}$ ]]; then
  echo 'APPLE_TEAM_ID must contain ten uppercase letters or digits' >&2
  exit 2
fi
if [[ $(uname -s) != Darwin || $(uname -m) != arm64 ]]; then
  echo 'Candidate signing requires a native Apple Silicon macOS host' >&2
  exit 2
fi

dtw_work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/dtw-candidate-sign.XXXXXXXX")
dtw_keychain="$dtw_work/signing.keychain-db"
dtw_stage=save_original_search_list
/usr/bin/security list-keychains -d user > "$dtw_work/keychains.txt" 2> "$dtw_work/auth.log"
python3 - "$dtw_work/keychains.txt" "$dtw_work/keychains.json" <<'PY'
import json, pathlib, shlex, sys
paths = shlex.split(pathlib.Path(sys.argv[1]).read_text())
pathlib.Path(sys.argv[2]).write_text(json.dumps(paths))
PY

dtw_stage=import_identity
printf '%s' "$dtw_certificate" | /usr/bin/base64 -D > "$dtw_work/identity.p12"
unset dtw_certificate
dtw_keychain_password=$(uuidgen)
export -n dtw_keychain_password
# create-keychain may itself change the search list; restore even on failure.
dtw_search_changed=1
if ! /usr/bin/security create-keychain -p "$dtw_keychain_password" "$dtw_keychain" > "$dtw_work/auth.log" 2>&1 ||
   ! /usr/bin/security unlock-keychain -p "$dtw_keychain_password" "$dtw_keychain" >> "$dtw_work/auth.log" 2>&1 ||
   ! /usr/bin/security set-keychain-settings -lut 7200 "$dtw_keychain" >> "$dtw_work/auth.log" 2>&1 ||
   ! /usr/bin/security import "$dtw_work/identity.p12" -k "$dtw_keychain" -P "$dtw_certificate_password" -T /usr/bin/codesign >> "$dtw_work/auth.log" 2>&1 ||
   ! /usr/bin/security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$dtw_keychain_password" "$dtw_keychain" >> "$dtw_work/auth.log" 2>&1; then
  echo 'Could not import/unlock the CI Developer ID identity; no build was started' >&2
  exit 2
fi
unset dtw_certificate_password dtw_keychain_password
rm -f -- "$dtw_work/identity.p12"
/usr/bin/security list-keychains -d user -s "$dtw_keychain" >> "$dtw_work/auth.log" 2>&1
/usr/bin/security find-identity -v -p codesigning "$dtw_keychain" > "$dtw_work/identities.txt" 2>> "$dtw_work/auth.log"
dtw_stage=validate_identity
dtw_identity=$(python3 - "$dtw_repo/scripts/build-rewrite-candidate.py" "$dtw_work/identities.txt" "$dtw_team" <<'PY'
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location('candidate', sys.argv[1])
module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
print(module.macos_signing_identity(pathlib.Path(sys.argv[2]).read_text(), sys.argv[3]))
PY
)

dtw_stage=store_notary_profile
dtw_profile=dtw-rewrite-candidate
if ! xcrun notarytool store-credentials "$dtw_profile" --keychain "$dtw_keychain" \
    --apple-id "$dtw_notary_user" --password "$dtw_notary_password" --team-id "$dtw_team" > "$dtw_work/auth.log" 2>&1; then
  echo 'Could not validate/store CI notarization credentials; no build was started' >&2
  exit 2
fi
unset dtw_notary_user dtw_notary_password
dtw_stage=build_sign_notarize_once
dtw_builder_invocations=1
python3 "$dtw_repo/scripts/build-rewrite-candidate.py" --target aarch64-apple-darwin --output "$dtw_output" \
  --signing-identity "$dtw_identity" --notary-profile "$dtw_profile" --notary-keychain "$dtw_keychain"

dtw_stage=verify_final_signature_team
python3 - "$dtw_output" "$dtw_team" <<'PY'
import json, pathlib, re, sys
root = pathlib.Path(sys.argv[1]); summary = json.loads((root/'CANDIDATE.json').read_text())
package = root/summary['archive'].removesuffix('.zip')
build = json.loads((package/'BUILD.json').read_text())
details = build['signing']['details']
if not re.search(r'^TeamIdentifier='+re.escape(sys.argv[2])+r'$', details, re.MULTILINE):
    raise SystemExit('Final candidate signature has an unexpected TeamIdentifier')
if not re.search(r'^Timestamp=.+$', details, re.MULTILINE):
    raise SystemExit('Final candidate signature lacks a secure timestamp')
if summary['notarization']['status'] != 'Accepted':
    raise SystemExit('Original candidate submission was not Accepted')
PY
dtw_stage=complete
