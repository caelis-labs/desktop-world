#!/usr/bin/env python3
"""Copy a packaged POC into one isolated Antigravity project plugin directory."""

import json
from pathlib import Path
import shutil
import sys


def main():
    source = Path(sys.argv[1]).resolve()
    destination = Path(sys.argv[2]).resolve()
    if not (source / "bin/dtw").is_file() or not (source / "skills/desktop-world/SKILL.md").is_file():
        raise SystemExit("source is not a DTW thin package")
    if destination.exists():
        raise SystemExit(f"destination already exists: {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(source, destination)
    (destination / "plugin.json").write_text(json.dumps({
        "$schema": "https://antigravity.google/schemas/v1/plugin.json",
        "name": "desktop-world",
        "description": "DTW thin CUA kernel POC",
    }, ensure_ascii=False, indent=2) + "\n")
    (destination / "mcp_config.json").write_text(json.dumps({
        "mcpServers": {"dtw": {"command": str(destination / "bin/dtw")}},
    }, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"plugin": str(destination), "mcp_command": str(destination / "bin/dtw")}))


if __name__ == "__main__":
    main()
