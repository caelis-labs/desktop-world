# Desktop World

**Give an AI Agent the ability to inspect and operate desktop apps.** Desktop World (DTW) exposes one MCP `exec` tool for window inspection, screenshots, clicks, typing, and short native scripts. Its Skill teaches the Agent to observe, act, and verify. An Agent still needs your normal desktop permissions before it can control an app.

## Get started

Download **one `dtw` executable for your platform**. It contains the Skill, plugin manifests, notices, and its private CUA worker. `dtw install` expands these files and connects the Agent; users do not need Rust, Python, Node, a DMG, or a separate worker download.

Download the `v0.1.0-rc.8` prerelease from [Releases](https://github.com/caelis-labs/desktop-world/releases/tag/v0.1.0-rc.8). Use the versioned links below so the commands resolve to this tested build.

### macOS Apple Silicon

```sh
curl -fL https://github.com/caelis-labs/desktop-world/releases/download/v0.1.0-rc.8/dtw-darwin-arm64 -o dtw
chmod +x dtw
./dtw install agy
```

### Windows x64 (PowerShell)

```powershell
Invoke-WebRequest https://github.com/caelis-labs/desktop-world/releases/download/v0.1.0-rc.8/dtw-windows-amd64.exe -OutFile dtw.exe
.\dtw.exe install agy
```

`agy` uses the installed Antigravity CLI to register the complete Plugin at user scope. For Claude Code, run `./dtw install claude` on macOS or `.\dtw.exe install claude` on Windows instead. The corresponding Agent CLI must already be installed. Both installs can coexist. Use `--project "/absolute/path/to/project"` for a project-local Antigravity Plugin or Claude registration. [Installation, upgrade, and removal →](docs/install.md)

Start a new Agent session and try a read-only task:

> Use the Desktop World Skill to inspect my open browser window and describe its controls. Do not click or type.

Then open a temporary document and ask it to type a short string and verify the visible result. On macOS, grant Accessibility and Screen Recording to the app that launches DTW, then restart that app. On Windows, the system may need the x64 Visual C++ runtime. [Permissions and limits →](docs/install.md#permissions-and-first-use)

## Embed DTW in an Agent host

The same single executable can expand a complete [Agent Plugin 1.0](https://agent-plugins.org/) directory, with `plugin.json`, `mcp.json`, `skills/`, `bin/dtw`, and `libexec/cua-driver`:

```sh
./dtw install plugin --to "/absolute/new/plugin-directory"
```

The destination must be new. A host such as Caelis Bot can package that directory for its own platform and load its one stdio MCP server plus Skill through the standard plugin interface. Host lifecycle, authorization, and enabling remain the host's responsibility. See the [embedding contract](docs/embedding.md). Caelis Core currently installs this local Plugin through its TUI; a direct `dtw install caelis` entry awaits [a supported headless Caelis installation API](https://github.com/caelis-labs/caelis/issues/129).

The current development branch makes CUA sessions implicit in the Runtime and adds macOS application observation with screenshot-bound clicks. See [architecture](docs/architecture.md) and [RC.9 integration validation](docs/rc9-acceptance.md); these changes are not part of the download linked above.

## Source and distribution

The [developer build guide](docs/building.md) covers Rust, Python and the macOS Swift toolchain for contributors. The signed macOS release gate signs the worker **before** embedding it, signs the final CLI, notarizes both executables, and verifies the extracted worker. Windows candidate CI validates package and MCP behavior; live Windows GUI acceptance is separate. See [release readiness](docs/release-readiness.md).

DTW is [MIT licensed](LICENSE). Its bundled CUA worker and dependencies retain their own terms; the expanded Plugin includes the complete [third-party notices](THIRD_PARTY_NOTICES.md) and `LICENSING` inventory.
