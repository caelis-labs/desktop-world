"""Generate the shared plugin entries for a DTW package, without installing it.

Keep executable paths, version and component locations identical in development
stages and release candidates. Host-specific manifests intentionally coexist;
their similarly named plugin.json files do not share a schema.
"""

import json
import shutil


EXECUTABLES = {
    "aarch64-apple-darwin": "dtw",
    "x86_64-pc-windows-msvc": "dtw.exe",
}


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def write_plugin_entries(source, package, version, target):
    """Write host manifests around the package's one binary and canonical Skill."""
    executable = EXECUTABLES[target]
    common = json.loads((source / "plugins/claude/.claude-plugin/plugin.json").read_text(encoding="utf-8"))
    common["version"] = version
    write_json(package / ".claude-plugin/plugin.json", common)
    write_json(package / ".mcp.json", {"mcpServers": {"desktop-world": {
        "command": "${CLAUDE_PLUGIN_ROOT}/bin/" + executable, "args": ["mcp"]}}})

    standard = {key: common[key] for key in
                ("name", "version", "description", "license", "author", "homepage", "repository", "keywords")
                if key in common}
    standard["$schema"] = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
    write_json(package / "plugin.json", standard)
    write_json(package / "mcp.json", {
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {"desktop-world": {"type": "stdio", "command": "./bin/" + executable, "args": ["mcp"]}},
    })

    # Antigravity checks its specific manifest before the standard root manifest.
    # Its documented built-in path expansion covers cwd, args and env, not
    # command. Resolve this relative executable from the explicit plugin cwd.
    agy = {**standard, "$schema": "https://antigravity.google/schemas/v1/plugin.json",
           "displayName": "Desktop World"}
    write_json(package / ".antigravity-plugin/plugin.json", agy)
    write_json(package / "mcp_config.json", {"mcpServers": {"desktop-world": {
        "command": "./bin/" + executable, "args": ["mcp"], "cwd": "${PLUGIN_ROOT}"}}})

    # A dependency-free Cordis adapter asks the host loader for its own MCP and
    # Skill implementations. It does not vendor or install another DSH runtime.
    write_json(package / "package.json", {
        "name": "@caelis-labs/desktop-world", "version": version, "type": "module", "private": True,
        "description": common.get("description", "Desktop World MCP and Skill"),
        "license": common.get("license", "MIT"), "main": "./cordis/index.mjs",
        "exports": "./cordis/index.mjs",
    })
    (package / "cordis").mkdir()
    shutil.copy2(source / "plugins/cordis/index.mjs", package / "cordis/index.mjs")
    return {"target": target, "binary": "bin/" + executable,
            "formats": ["agent-plugins-1.0", "claude-code", "antigravity", "dsh-cordis"]}


def host_target(system):
    """The staging helper supports the same native platforms as candidates."""
    if system == "Windows":
        return "x86_64-pc-windows-msvc"
    if system == "Darwin":
        return "aarch64-apple-darwin"
    raise ValueError("Use --target for a supported DTW package target")
