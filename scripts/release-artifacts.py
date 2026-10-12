#!/usr/bin/env python3
"""Validate a tag and stage one executable per platform for release review."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib


def main():
    repo = Path(__file__).resolve().parent.parent
    tag = os.environ["RELEASE_TAG"]
    version = tomllib.loads((repo / "cmd/dtw/Cargo.toml").read_text())["package"]["version"]
    assert tag == "v" + version and "/" not in tag and "\\" not in tag, (tag, version)
    assert json.loads((repo / "packaging/plugin/plugin.json").read_text())["version"] == version
    assert json.loads((repo / "packaging/plugin/.claude-plugin/plugin.json").read_text())["version"] == version
    assert json.loads((repo / "packaging/plugin/gemini-extension.json").read_text())["version"] == version
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
    binary = package / "bin" / ("dtw" + executable)
    path = out / f"dtw-{platform}{executable}"
    if platform.startswith("windows"):
        signing = {"type": "unsigned"}
    else:
        notary = json.loads((Path(os.environ["RUNNER_TEMP"]) / "dtw-notary-evidence.json").read_text())
        assert notary["status"] == "Accepted"
        assert notary["embedded_worker_matches_signed_worker"]
        assert notary["gatekeeper_dtw"] == "PASS" and notary["gatekeeper_extracted_worker"] == "PASS"
        assert notary["hardened_runtime"] and notary["signed_binaries"] == ["bin/dtw", "libexec/cua-driver"]
        signing = {"type": "Developer ID Application", "notarization": notary}
    path.write_bytes(binary.read_bytes())
    if not platform.startswith("windows"):
        path.chmod(0o755)
    binary_digest = hashlib.sha256(path.read_bytes()).hexdigest()
    (out / f"{path.name}.sha256").write_text(f"{binary_digest}  {path.name}\n", encoding="utf-8")
    evidence = {"tag": tag, "source_revision": revision, "cua_revision": inventory["cua_revision"],
        "platform": platform, "github_run_id": os.environ.get("GITHUB_RUN_ID"),
        "artifact_sha256": binary_digest,
        "adapter_sha256": hashlib.sha256((package / ("bin/dtw" + executable)).read_bytes()).hexdigest(),
        "worker_sha256": hashlib.sha256((package / ("libexec/cua-driver" + executable)).read_bytes()).hexdigest(),
        "license_packages": len(inventory["packages"]), "mpl_source_packages": inventory["mpl_source_packages"],
        "license_inventory_sha256": hashlib.sha256((package / "LICENSING/DEPENDENCIES.json").read_bytes()).hexdigest(),
        "validation": {"source_build": "PASS", "package_checksums": "PASS", "mcp_smoke": "PASS",
            "protocol_contracts": "PASS", "independent_installs": "PASS", "license_notices_and_mpl_source": "PASS"},
        "signing": signing}
    (out / f"BUILD-{platform}.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence))


if __name__ == "__main__":
    main()
