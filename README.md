# Desktop World

**Give an AI Agent the ability to inspect and operate desktop apps.** Desktop World (DTW) exposes one MCP `exec` tool for window inspection, screenshots, clicks, typing, and short native scripts. Its Skill teaches the Agent to observe, act, and verify. An Agent still needs your normal desktop permissions before it can control an app.

## Get started

Download **one `dtw` executable for your platform**. It contains the Skill, plugin manifests, notices, and its private CUA worker. `dtw install` expands these files and connects the Agent; users do not need Rust, Python, Node, a DMG, or a separate worker download.

Download the `v0.1.2` release from [Releases](https://github.com/caelis-labs/desktop-world/releases/tag/v0.1.2). Use the versioned links below so the commands resolve to this tested build.

### macOS Apple Silicon

```sh
curl -fL https://github.com/caelis-labs/desktop-world/releases/download/v0.1.2/dtw-darwin-arm64 -o dtw
chmod +x dtw
```

### Windows x64 (PowerShell)

```powershell
Invoke-WebRequest https://github.com/caelis-labs/desktop-world/releases/download/v0.1.2/dtw-windows-amd64.exe -OutFile dtw.exe
```

### Choose your Agent

Run the command for your Agent after downloading DTW. On Windows, replace `./dtw` with `.\dtw.exe`.

| Agent / host | Installation entry | Scope and integration |
| --- | --- | --- |
| **Caelis** | [Export and register the Plugin](#use-with-caelis) | Local host registration; standard Plugin with Skill + MCP |
| **Caelis Bot** | Built-in Computer Use Plugin | Bot manages its own pinned DTW version; [embedding contract](docs/embedding.md) |
| **Antigravity** | `./dtw install agy` | User or `--project`; native Plugin, requires `agy` CLI |
| **Claude Code** | `./dtw install claude` | User only; local marketplace Plugin, requires `claude` CLI |
| **Codex** | `./dtw install codex` | User or `--project`; MCP configuration + Skill |
| **Gemini CLI** | `./dtw install gemini` | User only; extension, requires `gemini` CLI |
| **Cursor** | `./dtw install cursor` | User or `--project`; MCP configuration + Skill |
| **Other Agent Plugin 1.0 hosts** | `./dtw install plugin --to "/absolute/new/plugin-directory"` | Export a complete package, then register through the host |

These are implemented installation paths, not a claim of native desktop acceptance for every Agent/platform combination. DTW publishes macOS Apple Silicon (macOS 14+) and Windows x64 packages. Agent-specific trust and permission prompts still apply. [Installation, upgrade, and removal →](docs/install.md)

### Use with Caelis

With the [Caelis CLI](https://github.com/caelis-labs/caelis) installed, expand DTW into a **new, permanent directory outside your workspace**, then register it:

```sh
./dtw install plugin --to "$HOME/.local/share/desktop-world-plugin"
caelis plugin install "$HOME/.local/share/desktop-world-plugin" --json
caelis plugin status desktop-world --json
```

PowerShell:

```powershell
.\dtw.exe install plugin --to "$env:LOCALAPPDATA\desktop-world-plugin"
caelis plugin install "$env:LOCALAPPDATA\desktop-world-plugin" --json
caelis plugin status desktop-world --json
```

Caelis registers the directory in place: keep its files available. You can also enter `/plugin install /absolute/path/to/plugin` in the Caelis TUI. Start a new Session after installation, check that the Skill and `desktop_world__exec` are ready, then try the task below. Registration status alone does not establish MCP readiness. For updates, finish active work and update the package at the same registered path. [Caelis installation details →](docs/install.md#caelis)

### First task

Start a new Agent session and try a read-only task:

> Use the Desktop World Skill to inspect my open browser window and describe its controls. Do not click or type.

Then open a temporary document and ask it to type a short string and verify the visible result. On macOS, grant Accessibility and Screen Recording to the app that launches DTW, then restart that app. On Windows, the system requires the x64 Visual C++ runtime. [Permissions and limits →](docs/install.md#permissions-and-first-use)

Windows upstream DPI, modal-provider and uncertain-focus limitations are tracked in [#36](https://github.com/caelis-labs/desktop-world/issues/36) and do not block this release. These flows remain limited support.

## Embed DTW in an Agent host

For smaller embedded packages, download the platform `desktop-world-plugin-*` archive from the same Release; it removes the redundant self-extracting payload.

The same single executable can expand a complete [Agent Plugin 1.0](https://agent-plugins.org/) directory, with `plugin.json`, `mcp.json`, `skills/`, `bin/dtw`, and `libexec/cua-driver`:

```sh
./dtw install plugin --to "/absolute/new/plugin-directory"
```

The destination must be new. A host such as Caelis Bot can package that directory for its own platform and load its one stdio MCP server plus Skill through the standard plugin interface. Host lifecycle, authorization, and enabling remain the host's responsibility. Publishing DTW does not update Bot's separately pinned copy. See the [embedding contract](docs/embedding.md).

Runtime-owned CUA sessions and macOS application observation with screenshot-bound clicks are included. See [architecture](docs/architecture.md) and [release readiness](docs/release-readiness.md).

## Source and distribution

The next release targets the usable rewrite only, with no old-runtime compatibility. The [product matrix](docs/product-matrix.md) records verified slices and remaining gaps; [implementation checkpoints](docs/implementation-checkpoints.md) follow the [architecture baseline](docs/next-architecture-baseline.md). macOS discovery, SCK capture/click, AX observe/press, app-scoped MCP and bounded scripts have checkpoint evidence. Real-app inventory validation, Windows, default-entry cleanup and rewritten packaging remain unfinished. The installation instructions above describe the existing released product.

The [developer build guide](docs/building.md) separates fast source/Swift controls from targeted local builds and GUI acceptance. Development CI no longer compiles upstream CUA or validates old platform packages. Stable core behavior will gain targeted CI coverage as the product develops.

The old automatic release/candidate workflows have been retired. New candidates will use the rewritten package contract once its scope and acceptance are ready. [Release direction and checkpoints →](docs/releasing.md)

DTW is [MIT licensed](LICENSE). Derived code and third-party dependencies retain their own terms; see [third-party notices](THIRD_PARTY_NOTICES.md). Historical released Plugins also include their bundled worker's notices and `LICENSING` inventory.
