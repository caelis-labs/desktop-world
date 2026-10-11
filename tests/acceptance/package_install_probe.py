#!/usr/bin/env python3
"""Exercise independent installs, Unicode paths and runtime-only MCP startup."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid

from probe import runtime_env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    package = args.package.resolve()
    root = args.output.resolve() / ("independent install 中文 " + uuid.uuid4().hex[:8])
    root.mkdir(parents=True)
    repo = Path(__file__).resolve().parents[2]
    executable = "dtw.exe" if os.name == "nt" else "dtw"
    binary = package / "bin" / executable
    records = []

    def run(arguments, expected=0):
        process = subprocess.run([str(arg) for arg in arguments], cwd=root, env=runtime_env(),
            capture_output=True, text=True, encoding="utf-8", timeout=60)
        record = {"arguments": [str(arg) for arg in arguments], "exit_code": process.returncode,
                  "stdout": process.stdout, "stderr": process.stderr}
        records.append(record)
        assert process.returncode == expected, record

    def checksum_map(path):
        result = {}
        for line in (path / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
            digest, name = line.split("  ", 1)
            file = path / name
            assert file.resolve().is_relative_to(path)
            assert hashlib.sha256(file.read_bytes()).hexdigest() == digest, name
            result[name.removeprefix("./")] = digest
        assert set(result) == {file.relative_to(path).as_posix() for file in path.rglob("*")
                               if file.is_file() and file.name != "SHA256SUMS"}
        return result

    report = {"package": str(package), "runtime_path": runtime_env()["PATH"], "records": records}
    try:
        source = checksum_map(package)
        assert "LICENSING/MPL-SOURCES.tar.gz" in source
        generic = root / "generic plugin"
        run([binary, "install", "plugin", "--to", generic])
        assert checksum_map(generic) == source
        run([binary, "install", "plugin", "--to", generic], expected=2)
        assert checksum_map(generic) == source
        run([sys.executable, repo / "tests/acceptance/mcp_smoke.py", generic / "bin" / executable])
        run([sys.executable, repo / "tests/acceptance/licenses_probe.py", generic])
        project = root / "independent project 中文"
        project.mkdir()
        run([binary, "install", "antigravity", "--project", project])
        installed = project / ".agents/plugins/desktop-world"
        manifest = json.loads((installed / "plugin.json").read_text(encoding="utf-8"))
        config = json.loads((installed / "mcp_config.json").read_text(encoding="utf-8"))
        assert manifest["name"] == "desktop-world"
        assert list(config["mcpServers"]) == ["dtw"]
        command = config["mcpServers"]["dtw"]["command"]
        assert Path(command).samefile(installed / "bin" / executable)
        assert not (installed / "SHA256SUMS").exists()
        for name, digest in source.items():
            if name in {"mcp.json", "plugin.json"}:
                continue
            assert hashlib.sha256((installed / name).read_bytes()).hexdigest() == digest, name
        run([binary, "install", "antigravity", "--project", project], expected=2)
        run([sys.executable, repo / "tests/acceptance/mcp_smoke.py", command])
        run([sys.executable, repo / "tests/acceptance/licenses_probe.py", installed])
        report.update(status="PASS", checksum_files=len(source),
                      antigravity_host_validation="Not run; isolated manifest and configured MCP command exercised")
    finally:
        (root / "summary.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "checksum_files": len(source),
                      "record": str(root / "summary.json")}))


if __name__ == "__main__":
    main()
