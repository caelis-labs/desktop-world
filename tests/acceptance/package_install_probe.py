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
import shutil

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

    isolated_home = root / "home with spaces"
    isolated_home.mkdir()
    def run(arguments, expected=0):
        environment = runtime_env()
        environment["HOME"] = str(isolated_home)
        environment["USERPROFILE"] = str(isolated_home)
        if os.name == "nt":
            environment["LOCALAPPDATA"] = str(isolated_home / "AppData/Local")
        process = subprocess.run([str(arg) for arg in arguments], cwd=root, env=environment,
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
        standard = json.loads((generic / "plugin.json").read_text(encoding="utf-8"))
        mcp = json.loads((generic / "mcp.json").read_text(encoding="utf-8"))
        assert standard["$schema"] == "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
        assert standard["name"] == "desktop-world"
        assert mcp["$schema"] == "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json"
        assert list(mcp["mcpServers"]) == ["desktop-world"]
        server = mcp["mcpServers"]["desktop-world"]
        assert server == {"type": "stdio", "command": "./bin/" + executable}
        assert (generic / "skills/desktop-world/SKILL.md").is_file()
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
        assert (installed / ".dtw-owner.json").is_file()
        for name, digest in source.items():
            if name in {"mcp.json", "plugin.json"}:
                continue
            assert hashlib.sha256((installed / name).read_bytes()).hexdigest() == digest, name
        run([binary, "install", "antigravity", "--project", project])
        repeated_command = json.loads((installed / "mcp_config.json").read_text(encoding="utf-8"))["mcpServers"]["dtw"]["command"]
        assert Path(repeated_command).samefile(installed / "bin" / executable)
        run([sys.executable, repo / "tests/acceptance/mcp_smoke.py", command])
        run([sys.executable, repo / "tests/acceptance/licenses_probe.py", installed])
        standalone = root / ("standalone dtw.exe" if os.name == "nt" else "standalone dtw")
        shutil.copy2(binary, standalone)
        standalone_project = root / "standalone project with spaces"
        standalone_project.mkdir()
        run([standalone, "install", "agy", "--project", standalone_project])
        if os.name == "nt":
            expanded = isolated_home / "AppData/Local/desktop-world/runtime"
        elif sys.platform == "darwin":
            expanded = isolated_home / "Library/Application Support/desktop-world/runtime"
        else:
            expanded = isolated_home / ".local/share/desktop-world/runtime"
        assert any((p / "libexec" / ("cua-driver.exe" if os.name == "nt" else "cua-driver")).is_file()
                   for p in expanded.iterdir())
        standalone_plugin = standalone_project / ".agents/plugins/desktop-world"
        assert (standalone_plugin / "skills/desktop-world/SKILL.md").is_file()
        worker_name = "cua-driver.exe" if os.name == "nt" else "cua-driver"
        embedded_worker = standalone_plugin / "libexec" / worker_name
        assert hashlib.sha256(embedded_worker.read_bytes()).hexdigest() == source["libexec/" + worker_name]
        run([sys.executable, repo / "tests/acceptance/mcp_smoke.py", standalone_plugin / "bin" / executable])
        run([standalone, "uninstall", "agy", "--project", standalone_project])
        assert not standalone_plugin.exists()
        run([sys.executable, repo / "tests/acceptance/mcp_smoke.py", standalone])
        for agent in ("codex", "cursor"):
            target_project = root / (agent + " project with spaces")
            target_project.mkdir()
            config = target_project / (".codex/config.toml" if agent == "codex" else ".cursor/mcp.json")
            config.parent.mkdir()
            original = '[mcp_servers.other]\ncommand = "other"\n' if agent == "codex" else '{"mcpServers":{"other":{"command":"other"}},"theme":"dark"}\n'
            config.write_text(original, encoding="utf-8")
            for _ in range(2):
                run([binary, "install", agent, "--project", target_project])
            assert "other" in config.read_text(encoding="utf-8")
            skill = target_project / (".agents" if agent == "codex" else ".cursor") / "skills/desktop-world/SKILL.md"
            assert skill.is_file()
            agent_package = target_project / ".agents/plugins" / agent / "desktop-world"
            assert (agent_package / "libexec" / ("cua-driver.exe" if os.name == "nt" else "cua-driver")).is_file()
            run([binary, "uninstall", agent, "--project", target_project])
            assert "other" in config.read_text(encoding="utf-8")
            assert "desktop-world" not in config.read_text(encoding="utf-8")
            assert not skill.exists() and not agent_package.exists()
        report.update(status="PASS", checksum_files=len(source),
                      antigravity_host_validation="Not run; isolated manifest and configured MCP command exercised")
    finally:
        (root / "summary.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "checksum_files": len(source),
                      "record": str(root / "summary.json")}))


if __name__ == "__main__":
    main()
