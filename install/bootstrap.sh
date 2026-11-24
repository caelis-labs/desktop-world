#!/bin/sh
# Repository bootstrap for macOS and Linux. The runtime owns all setup changes.
set -eu
export LC_ALL=C
umask 077

fail() {
    printf 'bootstrap: %s\n' "$*" >&2
    exit 1
}

case $# in
    2|3) ;;
    *) fail 'Usage: bootstrap.sh HOST PROFILE [INSTALLED_DTW]' ;;
esac
host=$1
profile=$2
installed=${3-}
for identifier in "$host" "$profile"; do
    case $identifier in
        ''|.*|*[!a-z0-9_.-]*) fail 'Invalid HOST or PROFILE identifier.' ;;
    esac
    [ "${#identifier}" -le 96 ] || fail 'Invalid HOST or PROFILE identifier.'
done
if [ "$#" -eq 3 ]; then
    case $installed in
        /*) ;;
        *) fail 'INSTALLED_DTW must be the absolute path of your trusted, registered runtime.' ;;
    esac
    [ -f "$installed" ] && [ -x "$installed" ] ||
        fail 'INSTALLED_DTW is not accessible as an executable. Check host authorization for that path.'
fi

os=$(uname -s)
case $os in
    Darwin) ;;
    Linux) command -v python3 >/dev/null || fail 'Linux bootstrap requires Python 3 to parse GitHub and setup JSON.' ;;
    *) fail "Unsupported operating system: $os." ;;
esac

# Both readers return a typed scalar or the length of an array.
json_read() {
    case $os in
        Darwin)
            if [ "$3" = present ]; then
                plutil -type "$2" -- "$1" >/dev/null 2>&1
            else
                plutil -extract "$2" raw -expect "$3" -o - -- "$1" 2>/dev/null
            fi
            ;;
        Linux)
            # Linux has no standard shell JSON reader. Use the standard library
            # already shipped on supported Ubuntu Desktop installations.
            python3 -I - "$@" 2>/dev/null <<'PY'
import json
import sys

filename, path, expected, *match = sys.argv[1:]
with open(filename, encoding="utf-8") as stream:
    value = json.load(stream)
for component in path.split("."):
    value = value[int(component) if isinstance(value, list) else component]
if expected == "present":
    sys.exit(0)
if expected == "index":
    field, wanted = match
    if type(value) is not list:
        sys.exit(1)
    indices = []
    for index, entry in enumerate(value):
        name = entry[field]
        if type(name) is not str:
            sys.exit(1)
        if name == wanted:
            indices.append(index)
    if len(indices) != 1:
        sys.exit(1)
    print(indices[0])
else:
    if type(value) is not {"bool": bool, "integer": int, "string": str}[expected]:
        sys.exit(1)
    print(json.dumps(value) if expected == "bool" else value)
PY
            ;;
    esac
}

# Ignore TMPDIR, which the host can redirect into the workspace.
stage_root=${DTW_STAGING_DIR:-/tmp}
case $stage_root in
    /*) ;;
    *) fail 'DTW_STAGING_DIR must be an approved absolute path outside the workspace.' ;;
esac
work=$(mktemp -d "${stage_root%/}/dtw-bootstrap.XXXXXXXX") ||
    fail "Cannot create staging directory in $stage_root. Authorize that path or set DTW_STAGING_DIR to an approved absolute path outside the workspace."
trap 'rm -rf "$work"' 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
receipt=$work/setup.json

probe_setup() {
    # A successful legacy --help has no setup usage entry.
    "$1" --help > "$work/help" ||
        fail "Cannot read runtime help at $1. Check host authorization and runtime availability."
    if ! grep -Eq '^[[:space:]]*dtw setup([[:space:]]|$)' "$work/help"; then
        grep -Eq '^[[:space:]]+dtw --help$' "$work/help" ||
            fail "Unexpected runtime help at $1."
        return 1
    fi
    "$1" setup --help > "$receipt" || fail "Cannot read setup capability at $1."
    [ "$(json_read "$receipt" setup_schema integer)" = 1 ] ||
        fail "Unsupported setup capability at $1."
}

run_setup() {
    setup_exe=$1
    setup_command=$2
    shift 2
    if "$setup_exe" setup "$setup_command" "$host" "$profile" "$@" > "$receipt"; then
        state=$(json_read "$receipt" state string) || fail "Invalid setup $setup_command response."
    else
        status=$?
        printf 'bootstrap: Setup %s failed at %s. If access was denied, authorize the runtime and its required paths through the host.\n' \
            "$setup_command" "$setup_exe" >&2
        exit "$status"
    fi
    [ "$(json_read "$receipt" host string)" = "$host" ] &&
        [ "$(json_read "$receipt" profile string)" = "$profile" ] ||
        fail 'Setup response does not match the requested host and profile.'
    if [ "$setup_command" = prepare ]; then
        [ "$(json_read "$receipt" package_id string)" = "$1" ] ||
            fail 'Setup response does not match the requested package ID.'
    fi
}

finish_setup() {
    case $state in
        verify|restart_required|host_action_required) cat "$receipt" ;;
        *) fail "Unexpected setup $setup_command state: $state." ;;
    esac
    exit 0
}

setup_installed=
if [ -n "$installed" ] && probe_setup "$installed"; then
    setup_installed=$installed
    run_setup "$setup_installed" resume
    case $state in
        no_pending) ;;
        *) finish_setup ;;
    esac
fi

case "$os:$(uname -m)" in
    Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin; suffix=.zip ;;
    Linux:x86_64) target=x86_64-unknown-linux-gnu; suffix=.tar.gz ;;
    Linux:aarch64|Linux:arm64) target=aarch64-unknown-linux-gnu; suffix=.tar.gz ;;
    *) fail 'Unsupported platform. See https://github.com/caelis-labs/desktop-world/blob/main/install.md.' ;;
esac

fetch() {
    url=$1
    output=$2
    shift 2
    curl --disable --fail --silent --show-error --proto '=https' --proto-redir '=https' \
        --output "$output" "$@" "$url" || fail "Cannot download $url."
}

valid_sha256() {
    [ "${#1}" -eq 64 ] || return 1
    case $1 in *[!0-9a-f]*) return 1 ;; esac
}

check_sha256() {
    case $os in
        Darwin) sum=$(shasum -a 256 "$1") ;;
        Linux) sum=$(sha256sum "$1") ;;
    esac || fail "Cannot hash ${1##*/}."
    [ "${sum%% *}" = "$2" ] || fail "SHA-256 mismatch for ${1##*/}."
}

# Look only at top-level entries of the expected array and require a unique match.
find_index() (
    if [ "$os" = Linux ]; then
        json_read "$1" "$2" index "$3" "$4" || fail "Expected one $4 in $2 of ${1##*/}."
        exit 0
    fi
    count=$(json_read "$1" "$2" array) || fail "Invalid $2 in ${1##*/}."
    index=0
    match=
    while [ "$index" -lt "$count" ]; do
        value=$(json_read "$1" "$2.$index.$3" string) || fail "Invalid $2 entry in ${1##*/}."
        if [ "$value" = "$4" ]; then
            [ -z "$match" ] || fail "Duplicate $4 in ${1##*/}."
            match=$index
        fi
        index=$((index + 1))
    done
    [ -n "$match" ] || fail "Missing $4 in ${1##*/}."
    printf '%s\n' "$match"
)

asset_key() (
    index=$(find_index "$release" assets name "$1") || exit 1
    key=assets.$index
    [ "$(json_read "$release" "$key.browser_download_url" string)" = "$download/$1" ] ||
        fail "Unexpected download URL for $1."
    printf '%s\n' "$key"
)

asset_digest() (
    key=$(asset_key "$1") || exit 1
    digest=$(json_read "$release" "$key.digest" string) || fail "Missing GitHub asset digest for $1."
    case $digest in
        sha256:*) digest=${digest#sha256:} ;;
        *) fail "Unsupported GitHub asset digest for $1." ;;
    esac
    valid_sha256 "$digest" || fail "Invalid GitHub asset digest for $1."
    printf '%s\n' "$digest"
)

repo=caelis-labs/desktop-world
release=$work/latest.json
fetch "https://api.github.com/repos/$repo/releases/latest" "$release" \
    --header 'Accept: application/vnd.github+json'
[ "$(json_read "$release" draft bool)" = false ] &&
    [ "$(json_read "$release" prerelease bool)" = false ] || fail 'GitHub did not return a stable public release.'
tag=$(json_read "$release" tag_name string) || fail 'GitHub release tag is missing.'
case $tag in *[!v0-9.]*) fail 'Invalid stable release tag.' ;; esac
printf '%s\n' "$tag" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || fail 'Invalid stable release tag.'
version=${tag#v}
download=https://github.com/$repo/releases/download/$tag
manifest=$work/RELEASE.json
manifest_digest=$(asset_digest RELEASE.json)
fetch "$download/RELEASE.json" "$manifest" --location --max-redirs 5
check_sha256 "$manifest" "$manifest_digest"
[ "$(json_read "$manifest" tag string)" = "$tag" ] &&
    [ "$(json_read "$manifest" version string)" = "$version" ] || fail 'Release manifest does not match the resolved tag.'
index=$(find_index "$manifest" platforms target "$target")
platform=platforms.$index
case $tag in
    v0.[0-5].*|v0.6.0)
        json_read "$manifest" "$platform.package_id" present ||
            fail "Release $tag is unsupported for managed setup (missing package_id). Follow the legacy installation instructions at https://github.com/$repo/blob/$tag/install.md."
        ;;
esac
package_id=$(json_read "$manifest" "$platform.package_id" string) && valid_sha256 "$package_id" ||
    fail 'Missing or invalid release package_id.'

if [ -n "$setup_installed" ]; then
    run_setup "$setup_installed" prepare "$package_id"
    case $state in
        needs_download) ;;
        *) finish_setup ;;
    esac
fi

package=desktop-world-$version-$target
archive=$package$suffix
[ "$(json_read "$manifest" "$platform.archive" string)" = "$archive" ] || fail 'Unexpected platform archive name.'
archive_digest=$(json_read "$manifest" "$platform.archive_sha256" string) || fail 'Missing platform archive digest.'
api_archive_digest=$(asset_digest "$archive")
[ "$archive_digest" = "$api_archive_digest" ] || fail 'Release and GitHub archive digests disagree.'
binary_digest=$(json_read "$manifest" "$platform.binary_sha256" string) || fail 'Missing platform binary digest.'
valid_sha256 "$binary_digest" || fail 'Invalid platform binary digest.'
sidecar=$archive.sha256
asset_key "$sidecar" >/dev/null
fetch "$download/$sidecar" "$work/$sidecar" --location --max-redirs 5
printf '%s  %s\n' "$archive_digest" "$archive" > "$work/expected.sha256"
cmp -s "$work/expected.sha256" "$work/$sidecar" || fail 'Archive sidecar disagrees with the release archive identity.'
fetch "$download/$archive" "$work/$archive" --location --max-redirs 5
check_sha256 "$work/$archive" "$archive_digest"

# GNU tar reads the Linux tarball; macOS BSD tar reads the ZIP. Check member
# names and types before writing anything. Release packages contain no links.
unset TAR_OPTIONS
tar -tf "$work/$archive" > "$work/members" &&
    tar -tvf "$work/$archive" > "$work/member-types" || fail 'Cannot inspect the verified release archive.'
awk -v root="$package" '
    {
        path = $0
        sub(/\/$/, "", path)
        if (path ~ /[[:cntrl:]\\]/ || path ~ /(^|\/)\.\.?($|\/)/ || path ~ /\/\// ||
            (path != root && index(path, root "/") != 1) || seen[path]++) exit 1
        if (path == root "/bin/dtw") executable = 1
    }
    END { if (!executable) exit 1 }
' "$work/members" &&
    awk 'NR == FNR { count++; next } !/^[-d]/ { exit 1 } END { if (FNR != count) exit 1 }' \
        "$work/members" "$work/member-types" || fail 'Unsafe release archive members.'
mkdir "$work/staging"
tar -xf "$work/$archive" -C "$work/staging" --no-same-owner --no-same-permissions ||
    fail 'Cannot extract the verified archive. Check host authorization for the staging path.'
source=$work/staging/$package
binary=$source/bin/dtw
[ -f "$binary" ] && [ -x "$binary" ] || fail 'Verified release archive lacks an executable bin/dtw.'
check_sha256 "$binary" "$binary_digest"
probe_setup "$binary" || fail "Release $tag does not advertise managed setup."
run_setup "$binary" prepare "$package_id" "$source"
finish_setup
