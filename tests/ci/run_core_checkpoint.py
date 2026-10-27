#!/usr/bin/env python3
"""Compile and exercise DTW's core without CUA, Cargo dependencies, or a desktop."""
from pathlib import Path
import os
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix="dtw-core-checkpoint-") as scratch:
        binary = Path(scratch) / ("core-tests.exe" if os.name == "nt" else "core-tests")
        subprocess.run(
            ["rustc", "+1.98.0", "--edition=2021", "--test",
             str(root / "cmd/dtw/src/lib.rs"), "-D", "warnings", "-o", str(binary)],
            check=True,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
