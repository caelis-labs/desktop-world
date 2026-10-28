#!/usr/bin/env python3
"""Compile pure macOS inventory controls without reading or operating the desktop."""
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    if sys.platform != "darwin":
        print("Native inventory controls skipped: macOS Swift frameworks required")
        return
    root = Path(__file__).resolve().parents[2]
    native = root / "cmd/dtw/src/backends/macos/Native.swift"
    admission = root / "cmd/dtw/src/backends/macos/Input.swift"
    scene = root / "cmd/dtw/src/backends/macos/Scene.swift"
    controls = root / "tests/fixtures/native-inventory-tests.swift"
    with tempfile.TemporaryDirectory(prefix="dtw-native-inventory-") as scratch:
        directory = Path(scratch)
        source = directory / "main.swift"
        source.write_text("\n".join(path.read_text() for path in [
            admission, native, scene, controls,
            native.with_name("surface_contract_tests.swift"),
            native.with_name("scene_contract_tests.swift"),
        ]))
        binary = directory / "native-inventory-tests"
        subprocess.run(
            ["swiftc", "-warnings-as-errors", "-module-cache-path", str(directory / "module-cache"),
             str(source), "-o", str(binary)],
            check=True, timeout=120,
        )
        subprocess.run([str(binary)], check=True, timeout=20)


if __name__ == "__main__":
    main()
