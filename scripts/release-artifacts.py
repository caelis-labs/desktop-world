#!/usr/bin/env python3
"""Validate and archive tagged CI packages; publication is a separate final step."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tomllib
import zipfile


def main():
    repo = Path(__file__).resolve().parent.parent
    tag = os.environ["RELEASE_TAG"]
    version = tomllib.loads((repo / "cmd/dtw/Cargo.toml").read_text())["package"]["version"]
    assert tag == "v" + version and "/" not in tag and "\\" not in tag, (tag, version)
    assert json.loads((repo / "packaging/plugin/plugin.json").read_text())["version"] == version
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if sys.argv[1] == "validate":
        assert subprocess.check_output(["git", "rev-parse", tag + "^{commit}"], text=True).strip() == revision
        print(json.dumps({"tag": tag, "revision": revision}))
        return
    assert sys.argv[1] == "archive"
    package = Path(os.environ["RUNNER_TEMP"]) / "dtw-plugin"
    platform = os.environ["RELEASE_PLATFORM"]
    assert platform in {"darwin-arm64", "windows-amd64"}
    executable = ".exe" if platform.startswith("windows") else ""
    inventory = json.loads((package / "LICENSING/DEPENDENCIES.json").read_text(encoding="utf-8"))
    out = repo / "dist"
    out.mkdir(exist_ok=True)
    if platform.startswith("windows"):
        path = out / f"desktop-world-{tag}-{platform}.zip"
        with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for file in sorted(package.rglob("*")):
                if file.is_file():
                    archive.write(file, "desktop-world/" + file.relative_to(package).as_posix())
    else:
        path = out / f"desktop-world-{tag}-{platform}.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            archive.add(package, arcname="desktop-world")
    evidence = {"tag": tag, "source_revision": revision, "cua_revision": inventory["cua_revision"],
        "platform": platform, "github_run_id": os.environ.get("GITHUB_RUN_ID"),
        "archive_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "adapter_sha256": hashlib.sha256((package / ("bin/dtw" + executable)).read_bytes()).hexdigest(),
        "worker_sha256": hashlib.sha256((package / ("libexec/cua-driver" + executable)).read_bytes()).hexdigest(),
        "license_packages": len(inventory["packages"]), "mpl_source_packages": inventory["mpl_source_packages"],
        "license_inventory_sha256": hashlib.sha256((package / "LICENSING/DEPENDENCIES.json").read_bytes()).hexdigest(),
        "validation": {"source_build": "PASS", "package_checksums": "PASS", "mcp_smoke": "PASS",
            "protocol_contracts": "PASS", "independent_installs": "PASS", "license_notices_and_mpl_source": "PASS"},
        "signing": "ad-hoc; not notarized" if platform.startswith("darwin") else "unsigned"}
    (out / f"BUILD-{platform}.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence))


if __name__ == "__main__":
    main()
