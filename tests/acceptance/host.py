"""Host process lookup for isolated macOS acceptance applications."""

import re
import subprocess


def wps_pids():
    found = subprocess.run(
        ["pgrep", "-fl", "/Applications/wpsoffice.app/Contents/MacOS/wpsoffice"],
        capture_output=True, text=True,
    )
    if found.returncode == 1:
        return set()
    found.check_returncode()
    return {
        int(line.split(None, 1)[0]) for line in found.stdout.splitlines()
        if "/Applications/wpsoffice.app/Contents/MacOS/wpsoffice" in line
    }


def obsidian_pid(profile):
    lines = subprocess.check_output(
        ["pgrep", "-fl", "Obsidian.app/Contents/MacOS/Obsidian"], text=True,
    ).splitlines()
    matches = []
    for line in lines:
        if str(profile) in line:
            match = re.match(r"^\s*(\d+)\s", line)
            if match:
                matches.append(int(match.group(1)))
    if len(matches) != 1:
        raise RuntimeError(f"expected one temporary Obsidian PID, got {len(matches)}")
    return matches[0]
