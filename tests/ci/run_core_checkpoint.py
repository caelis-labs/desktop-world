#!/usr/bin/env python3
"""Compile and exercise DTW's core without CUA, Cargo dependencies, or a desktop."""
from pathlib import Path
import os
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix="dtw-core-checkpoint-") as scratch:
        for name, source in [("core", "tests/ci/core_checkpoint.rs"),
                             ("ipc", "cmd/dtw/src/backends/process.rs"),
                             ("input-sequence", "cmd/dtw/src/backends/input_sequence.rs"),
                             ("linux-inventory", "cmd/dtw/src/backends/linux/inventory.rs")]:
            binary = Path(scratch) / (name + ("-tests.exe" if os.name == "nt" else "-tests"))
            subprocess.run(
                ["rustc", "+1.98.0", "--edition=2021", "--test",
                 str(root / source), "-D", "warnings", "-o", str(binary)],
                check=True,
            )
            subprocess.run([str(binary)], check=True)



if __name__ == "__main__":
    main()
