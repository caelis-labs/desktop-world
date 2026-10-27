#!/bin/sh
# Shared source checks only. Final packaged/signed bytes have separate gates.
set -eu
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$repo_root"
cargo +1.98.0 fmt --manifest-path cmd/dtw/Cargo.toml --check
python3 tests/ci/run_core_checkpoint.py
python3 -m compileall -q scripts tests/acceptance tests/ci
python3 -m unittest discover -s tests/acceptance -p test_click_native_probe.py
python3 -m unittest discover -s tests/ci -p 'test_*.py'
python3 -m unittest discover -s tests -p test_app_protocol_probe.py
npm exec --yes --package typescript@5.9.3 -- tsc --strict --lib es2020 --noEmit tests/acceptance/click_types_contract.ts
owned_source=
trap 'if [ -n "$owned_source" ]; then rm -rf -- "$owned_source"; fi' 0
if [ -n "${CUA_SOURCE_ROOT:-}" ]; then
  source_root=$CUA_SOURCE_ROOT
else
  pin=$(python3 -c 'import tomllib; print(tomllib.load(open("cmd/dtw/Cargo.toml", "rb"))["dependencies"]["cua-driver-sdk"]["rev"])')
  owned_source=$(mktemp -d "${TMPDIR:-/tmp}/dtw-source-contract.XXXXXXXX")
  source_root=$owned_source
  git -C "$source_root" init -q
  git -C "$source_root" fetch -q --no-tags --depth 1 https://github.com/trycua/cua.git "$pin"
  git -C "$source_root" checkout -q --detach FETCH_HEAD
fi
node tests/acceptance/click_prelude_contract.mjs "$source_root"
