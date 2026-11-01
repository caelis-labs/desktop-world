#!/usr/bin/env python3
"""Stage one existing DTW binary and Skill as a local multi-Agent plugin."""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil

_plugin_spec = importlib.util.spec_from_file_location("dtw_plugin_package", Path(__file__).with_name("plugin_package.py"))
plugin_package = importlib.util.module_from_spec(_plugin_spec)
_plugin_spec.loader.exec_module(plugin_package)


def stage(binary, output, repo, target=None):
    binary = binary.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("--binary must be an existing executable file")
    output = output.absolute()
    if not output.parent.is_dir():
        raise ValueError("--output parent directory must already exist")
    target = target or plugin_package.host_target(platform.system())
    executable = plugin_package.EXECUTABLES[target]
    version = (repo / "version.txt").read_text(encoding="utf-8").strip()
    # Reserve a new directory. Never modify an existing plugin or user config.
    output.mkdir()
    try:
        (output / "bin").mkdir()
        shutil.copy2(binary, output / "bin" / executable)
        entries = plugin_package.write_plugin_entries(repo, output, version, target)
        shutil.copy2(repo / "plugins/claude/README.md", output / "README.md")
        shutil.copytree(repo / "skills/desktop-world", output / "skills/desktop-world")
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
            shutil.copy2(repo / name, output / name)
        (output / "docs").mkdir()
        for name in ("native-source-provenance.md", "native-source-provenance-cross-platform.md",
                     "background-input-port.md", "install.md", "install-caelis.md", "install-dsh.md", "install-agy.md"):
            shutil.copy2(repo / "docs" / name, output / "docs" / name)
        shutil.copy2(repo / "install.md", output / "install.md")
        shutil.copytree(repo / "packaging/third-party", output / "packaging/third-party")
        with (output / "bin" / executable).open("rb") as handle:
            binary_sha256 = hashlib.file_digest(handle, "sha256").hexdigest()
        metadata = {
            "layout": "multi-agent-plugin",
            "version": version,
            "binary_sha256": binary_sha256,
            "distribution": "local-development-stage",
            **entries,
        }
        (output / "build-info.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    except Exception:
        # This invocation created the directory; keep partial output inspectable.
        raise RuntimeError(f"Staging failed; partial output remains at {output}")
    return output, metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path, help="Already-built local DTW executable")
    parser.add_argument("--output", required=True, type=Path, help="New directory; existing paths are refused")
    parser.add_argument("--target", choices=plugin_package.EXECUTABLES,
                        help="Package target; defaults to the current supported host")
    args = parser.parse_args()
    try:
        output, metadata = stage(args.binary, args.output, Path(__file__).resolve().parent.parent, args.target)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps({"output": str(output), **metadata}, indent=2))


if __name__ == "__main__":
    main()
