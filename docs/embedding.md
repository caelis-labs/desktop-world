# Embed the Desktop World Agent Plugin

The per-platform `dtw` release executable is also a Plugin exporter. At application packaging or installation time, run `dtw install plugin --to NEW_DIRECTORY`. This needs no DTW build toolchain or network access. The output is a complete, relocatable Plugin for that platform:

```text
plugin.json                   Agent Plugin 1.0 identity
mcp.json                      one stdio MCP server: desktop-world
skills/desktop-world/         Skill and authoring references
bin/dtw                       MCP executable (dtw.exe on Windows)
libexec/cua-driver            private native worker (cua-driver.exe on Windows)
LICENSING/                    dependency inventory, notices, and source materials
.claude-plugin/plugin.json    Claude-compatible identity
.mcp.json                     Claude-compatible MCP declaration
```

The standard `mcp.json` launches `./bin/dtw` from the Plugin root. The CLI finds `libexec/cua-driver` beside itself at runtime. Keep the directory together; copying only `bin/dtw` out of it loses the private worker. On Windows the exported `mcp.json` uses `./bin/dtw.exe`. The host should treat the package as a single versioned unit and keep an active Session on its original version until that Session ends.

## Caelis Bot integration boundary

Caelis Bot can stage the platform-specific Plugin during its own build or installation, verify the release executable's published SHA-256, and load the exported directory through the standard Agent Plugin interface in Caelis Core. Core already recognizes root `plugin.json`, root `mcp.json`, and `skills/`. The Bot remains responsible for enabling the Plugin, obtaining user authorization, providing its own lifecycle and update policy, and starting new Sessions after an update. DTW exposes only the single MCP `exec` tool; the Bot should not add a second desktop-control bridge for this package.

The current Bot release still uses its existing Desktop World integration. This document defines the new package handoff; it is not a claim that a Bot build has already switched to this Plugin. A Bot acceptance run must verify Plugin registration, Skill discovery, MCP initialization, `exec` availability, and native worker startup from its actual application bundle on each platform.

## Validation from one downloaded file

Copy the downloaded executable to a clean location without any sibling files, then export the Plugin and run the MCP protocol smoke test against the exported `bin/dtw`. The release pipeline does this with isolated homes and paths containing spaces. It also verifies that the embedded worker extracted from a signed macOS CLI matches the worker submitted to Apple for notarization. See [release readiness](release-readiness.md).
