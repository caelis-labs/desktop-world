#!/bin/sh
# Fast rewrite checks: no CUA checkout, Cargo dependency build, package, or GUI.
set -eu
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$repo_root"
python3 scripts/release-artifacts.py check
cargo +1.98.0 fmt --manifest-path cmd/dtw/Cargo.toml --check
python3 tests/ci/run_core_checkpoint.py
python3 -m compileall -q scripts tests/acceptance tests/ci
python3 -m unittest discover -s tests -p test_app_protocol_probe.py
python3 -m unittest discover -s tests -p test_window_inventory_probe.py
python3 -m unittest discover -s tests -p test_input_cleanup_probe.py
python3 -m unittest discover -s tests -p test_rewrite_candidate.py
python3 -m unittest discover -s tests/acceptance -p test_browser_workbench_probe.py
python3 -m unittest discover -s tests/acceptance -p test_native_input_probe.py
python3 -m unittest discover -s tests/acceptance -p test_background_input_probe.py
python3 -m unittest discover -s tests/acceptance -p test_application_surface_probe.py
python3 -m unittest discover -s tests/acceptance -p test_cursor_overlay_probe.py
python3 -m unittest discover -s tests/acceptance -p test_anchored_scroll_probe.py
python3 -m unittest discover -s tests/acceptance -p test_real_app_tasks.py

python3 -m unittest discover -s tests -p test_ci_plan.py
python3 -m unittest discover -s tests -p test_release_verification.py
