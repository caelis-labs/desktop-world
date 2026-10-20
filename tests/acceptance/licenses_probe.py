#!/usr/bin/env python3
"""Verify shipped attribution bytes and complete source for the pinned MPL crates."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import sys
import tarfile


def main():
    package = Path(sys.argv[1]).resolve()
    root = package / "LICENSING"
    inventory = json.loads((root / "DEPENDENCIES.json").read_text(encoding="utf-8"))
    assert inventory["cua_revision"] == "2d2f99228d4d2508dc6fcc84c176b296e146714a"
    assert set(inventory["targets"]) == {"aarch64-apple-darwin", "x86_64-pc-windows-msvc"}
    assert len(inventory["packages"]) >= 300
    count = 0
    for component in inventory["packages"]:
        assert component["license"] and component["notices"], component["name"]
        assert component["uses"] and all(use.startswith(("adapter:", "worker:")) for use in component["uses"])
        if component["source"].startswith("registry+"):
            assert len(component["registry_checksum"]) == 64, component["name"]
        for notice in component["notices"]:
            path = root / notice["file"]
            assert path.resolve().is_relative_to(root), path
            assert hashlib.sha256(path.read_bytes()).hexdigest() == notice["sha256"], path
            count += 1
    expected = {name + "-0.31.0" for name in (
        "uniffi", "uniffi_core", "uniffi_internal_macros", "uniffi_macros", "uniffi_meta", "uniffi_pipeline")}
    assert set(inventory["mpl_source_packages"]) == expected
    archive = root / "MPL-SOURCES.tar.gz"
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == inventory["mpl_source_sha256"]
    with tarfile.open(archive) as source:
        names = source.getnames()
        assert all(not PurePosixPath(name).is_absolute() and ".." not in PurePosixPath(name).parts for name in names)
        assert {name.split("/")[0] for name in names} == expected
        for crate in expected:
            assert any(name.startswith(crate + "/src/") and name.endswith(".rs") for name in names), crate
            assert any(name.startswith(crate + "/") and "/LICENSE" in name.upper() for name in names), crate
            assert crate + "/Cargo.toml.orig" in names, crate
    assert (root / "RUST-COPYRIGHT-library.html").is_file()
    assert (root / "rust-runtime-licenses/MIT.txt").is_file()
    assert (root / "rust-runtime-licenses/Apache-2.0.txt").is_file()
    embedded = {component["name"]: component for component in inventory["embedded_components"]}
    assert embedded["Inter font"]["license"] == "OFL-1.1"
    assert "Copyright 2020 The Inter Project Authors" in (root / embedded["Inter font"]["notice"]).read_text(encoding="utf-8")
    print(json.dumps({"packages": len(inventory["packages"]), "notice_files": count,
                      "mpl_source_packages": sorted(expected), "status": "PASS"}))


if __name__ == "__main__":
    main()
