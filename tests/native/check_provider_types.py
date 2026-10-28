#!/usr/bin/env python3
"""Check native providers without cross-compiling QuickJS or linking a product.

This is intentionally opt-in, not a new PR matrix. Rust target standard libraries
must already be installed. It checks production source + tests against its target
APIs, but neither runs those tests nor claims native GUI/build acceptance.
"""
import argparse
import json
import shutil
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", choices=["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"], required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    cargo = (root / "cmd/dtw/Cargo.toml").read_text()
    targets = cargo[cargo.index("[target."):cargo.index("[profile.release]")]
    with tempfile.TemporaryDirectory(prefix="dtw-provider-types-") as directory:
        path = Path(directory)
        (path / "src").mkdir()
        (path / "Cargo.toml").write_text('[package]\nname="dtw-provider-types"\nversion="0.0.0"\nedition="2021"\n[dependencies]\nserde_json="1"\nbase64="0.22.1"\nuuid={version="1",features=["v4"]}\n' + targets)
        shutil.copyfile(root / "cmd/dtw/Cargo.lock", path / "Cargo.lock")
        (path / "src/lib.rs").write_text(
            '#[path=' + json.dumps(str(root / "cmd/dtw/src/computer/mod.rs"), ensure_ascii=False) + '] pub mod computer;\n' +
            '#[path=' + json.dumps(str(root / "cmd/dtw/src/backends/mod.rs"), ensure_ascii=False) + '] pub mod backends;\n')
        subprocess.run(["cargo", "check", "--offline", "--tests", "--target", args.target,
                        "--manifest-path", str(path / "Cargo.toml")], check=True)
    print("Provider source/test type check passed; no linking, runtime or GUI claim.")


if __name__ == "__main__":
    main()
