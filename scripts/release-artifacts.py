#!/usr/bin/env python3
"""Check rewrite version metadata. No artifact build or publication is provided."""
import argparse
import json
from pathlib import Path
import tomllib


def check_version(repo):
    package = tomllib.loads((repo / "cmd/dtw/Cargo.toml").read_text())["package"]
    version = package["version"]
    if (repo / "version.txt").read_text().strip() != version:
        raise SystemExit("version.txt does not match cmd/dtw/Cargo.toml")
    packages = tomllib.loads((repo / "cmd/dtw/Cargo.lock").read_text())["package"]
    locked = [entry["version"] for entry in packages if entry["name"] == package["name"]]
    if locked != [version]:
        raise SystemExit("cmd/dtw/Cargo.lock does not match cmd/dtw/Cargo.toml")
    manifest = repo / ".release-please-manifest.json"
    if manifest.exists() and json.loads(manifest.read_text()).get(".") != version:
        raise SystemExit("Release Please manifest does not match cmd/dtw/Cargo.toml")
    return version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check",))
    parser.parse_args()
    version = check_version(Path(__file__).resolve().parent.parent)
    print(json.dumps({"version": version, "version_files": "consistent"}))


if __name__ == "__main__":
    main()
