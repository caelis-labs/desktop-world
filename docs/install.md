# Install Desktop World

The first public release is pending. When published, download the one executable matching your platform from [Releases](https://github.com/caelis-labs/desktop-world/releases): `dtw-darwin-arm64` for macOS Apple Silicon or `dtw-windows-amd64.exe` for Windows x64. The [README](../README.md#get-started) has copyable download and first-use commands. Keep the downloaded `dtw` file if you want to rerun the installer for upgrades or removal.

The release executable embeds its own CUA worker, Skill, manifests, and licensing files. On first use it expands a versioned runtime under your user profile. Moving or deleting the original download after installation does not break an installed Plugin; that Plugin has its own `bin/dtw` and `libexec/cua-driver`.

## Antigravity

User scope (the default):

```sh
./dtw install agy
```

PowerShell: `.\dtw.exe install agy`. This requires the `agy` CLI and uses its native `plugin install` command to register `desktop-world` and its Skill and MCP server. If the CLI is missing, install Antigravity first and rerun the command. To install only for one project, use an **existing** project path:

```sh
./dtw install agy --project "/absolute/path/to/my project"
```

PowerShell: `.\dtw.exe install agy --project "C:\path\to\my project"`. Project scope writes the Plugin to that project's `.agents/plugins/desktop-world` directory for Antigravity's project plugin discovery; it does not register a user import. `antigravity` is also accepted as a command alias. The installer owns only its `desktop-world` directory and refuses to overwrite another owner's files. Repeat the same command to update DTW. Start a fresh Antigravity session to load the new Skill and MCP server.

The plugin directory contains Antigravity's `plugin.json`, `mcp_config.json`, the Skill, and the complete native runtime. The MCP command resolves to the executable inside that installed directory; it does not depend on where you left the downloaded file.

## Claude Code

Install Claude Code first. Then run:

```sh
./dtw install claude
```

PowerShell: `.\dtw.exe install claude`. For an existing project only, append `--project "/absolute/path/to/project"` (PowerShell: `--project "C:\path\to\project"`). DTW creates a local `dtw-local` marketplace containing its Plugin and asks the Claude CLI to register `desktop-world@dtw-local`. The Plugin contains `.claude-plugin/plugin.json`, `.mcp.json`, and the same Skill and native runtime. A missing Claude CLI produces an actionable error. Restart the Claude session after installation or update.

## Standard Agent Plugin and Caelis hosts

To stage a complete [Agent Plugin 1.0](https://agent-plugins.org/) package for an embedding host:

```sh
./dtw install plugin --to "/absolute/new/plugin-directory"
```

PowerShell: `.\dtw.exe install plugin --to "C:\new\plugin-directory"`. The destination must not already exist. The result includes its own `bin/dtw` and `libexec/cua-driver`; keep those together. Its `mcp.json` declares one stdio server and `skills/desktop-world/SKILL.md` supplies Agent guidance. [Host integration contract →](embedding.md)

Caelis Core currently accepts this package through its TUI `/plugin install <absolute directory>`. Caelis Bot can use the same standard package as a versioned embedded dependency once its host integration installs and enables it. A direct `dtw install caelis` requires [a supported headless Caelis Plugin API](https://github.com/caelis-labs/caelis/issues/129) and is outside the first release gate; DTW does not edit Caelis's private store.

## Upgrade and remove

Download the newer platform `dtw` and repeat the install command with the **same scope or project**. The installer replaces only directories marked as its own and preserves unrelated host configuration. Stop active DTW sessions and start a fresh Agent session after updating.

```sh
./dtw uninstall agy
./dtw uninstall claude
./dtw uninstall agy --project "/absolute/path/to/my project"
```

PowerShell uses `.\dtw.exe uninstall ...`. Claude removal unregisters only `desktop-world@dtw-local` and its DTW-owned local marketplace. User-scope Antigravity removal uses `agy plugin uninstall desktop-world` after checking DTW ownership; project-scope removal deletes only its installer-owned Plugin directory. For a generic `install plugin --to` export, the embedding host owns its lifecycle; remove it through that host.

## Permissions and first use

On macOS, grant Accessibility and Screen Recording to the Agent or terminal app that launches DTW, then fully quit and reopen it. The signed CLI and worker should be checked on a fresh Mac before public release. On Windows, the x64 Visual C++ runtime may be required; an elevated target app still follows Windows integrity restrictions.

Start with a read-only task: “Use the Desktop World Skill to inspect my open browser window and describe its controls. Do not click or type.” Next, use a temporary document and ask the Agent to type a short string and verify it appears. The [Skill](../packaging/plugin/skills/desktop-world/SKILL.md) explains native scripts, receipts, and readback; users can describe tasks in natural language.

## Protocol references

[Agent Plugin 1.0](https://agent-plugins.org/) · [Claude plugin components](https://code.claude.com/docs/en/plugins/components) · [Antigravity plugins](https://www.antigravity.google/docs/plugins)
