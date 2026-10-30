#!/usr/bin/env python3
"""Stage an existing DTW binary and Skill in a new local Claude Code plugin directory."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil


def stage(binary, output, repo):
    binary = binary.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("--binary must be an existing executable file")
    output = output.absolute()
    if not output.parent.is_dir():
        raise ValueError("--output parent directory must already exist")
    version = (repo / "version.txt").read_text().strip()
    manifest = json.loads((repo / "plugins/claude/.claude-plugin/plugin.json").read_text())
    manifest["version"] = version
    # Reserve a new directory. Never modify an existing plugin or user config.
    output.mkdir()
    try:
        (output / "bin").mkdir()
        shutil.copy2(binary, output / "bin/dtw")
        shutil.copytree(repo / "plugins/claude/.claude-plugin", output / ".claude-plugin")
        (output / ".claude-plugin/plugin.json").write_text(json.dumps(manifest, indent=2) + "\n")
        shutil.copy2(repo / "plugins/claude/.mcp.json", output / ".mcp.json")
        shutil.copy2(repo / "plugins/claude/README.md", output / "README.md")
        shutil.copytree(repo / "skills/desktop-world", output / "skills/desktop-world")
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
            shutil.copy2(repo / name, output / name)
        (output / "docs").mkdir()
        for name in ("native-source-provenance.md", "native-source-provenance-cross-platform.md",
                     "background-input-port.md"):
            shutil.copy2(repo / "docs" / name, output / "docs" / name)
        shutil.copytree(repo / "packaging/third-party", output / "packaging/third-party")
        with (output / "bin/dtw").open("rb") as executable:
            binary_sha256 = hashlib.file_digest(executable, "sha256").hexdigest()
        metadata = {
            "layout": "claude-code-plugin",
            "version": version,
            "binary_sha256": binary_sha256,
            "distribution": "local-development-stage",
        }
        (output / "build-info.json").write_text(json.dumps(metadata, indent=2) + "\n")
    except Exception:
        # This invocation created the directory; keep partial output inspectable.
        raise RuntimeError(f"Staging failed; partial output remains at {output}")
    return output, metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path, help="Already-built local DTW executable")
    parser.add_argument("--output", required=True, type=Path, help="New directory; existing paths are refused")
    args = parser.parse_args()
    try:
        output, metadata = stage(args.binary, args.output, Path(__file__).resolve().parent.parent)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps({"output": str(output), **metadata}, indent=2))


if __name__ == "__main__":
    main()
