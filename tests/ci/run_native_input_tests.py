#!/usr/bin/env python3
"""Exercise native input coordination without posting events or reading the desktop."""
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    if sys.platform != "darwin":
        print("Native input controls skipped: macOS Swift runtime required")
        return
    root = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix="dtw-native-input-") as scratch:
        directory = Path(scratch)
        source = directory / "main.swift"
        source.write_text((root / "cmd/dtw/src/backends/macos/Input.swift").read_text()
                          + "\n" + (root / "tests/fixtures/native-input-tests.swift").read_text())
        binary = directory / "native-input-tests"
        subprocess.run(["swiftc", "-warnings-as-errors", "-module-cache-path",
                        str(directory / "module-cache"), str(source), "-o", str(binary)],
                       check=True, timeout=120)
        subprocess.run([str(binary)], check=True, timeout=20)


if __name__ == "__main__":
    main()
