# Install Desktop World

Download the `v0.1.0` release's one executable matching your platform from [Releases](https://github.com/caelis-labs/desktop-world/releases/tag/v0.1.0): `dtw-darwin-arm64` for macOS Apple Silicon or `dtw-windows-amd64.exe` for Windows x64. The [README](../README.md#get-started) has copyable download and first-use commands. Keep the downloaded `dtw` file if you want to rerun the installer for upgrades or removal.

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

PowerShell: `.\dtw.exe install claude`. Claude installation supports user scope only; `--project` is rejected before changing files. DTW creates a local `dtw-local` marketplace containing its Plugin and asks the Claude CLI to register `desktop-world@dtw-local`. The Plugin contains `.claude-plugin/plugin.json`, `.mcp.json`, and the same Skill and native runtime. A missing Claude CLI produces an actionable error. Restart the Claude session after installation or update.

## Codex and Cursor

```sh
./dtw install codex
./dtw install cursor
```

PowerShell uses `.\dtw.exe install codex` or `.\dtw.exe install cursor`.
Both support user scope (default) or `--project "/absolute/existing/project"`.
DTW installs a complete runtime and projects its Skill and MCP configuration into
the selected host's configuration. Existing unrelated entries are preserved;
ownership conflicts are rejected. A project may still require host trust before
its MCP configuration is loaded. Start a fresh Agent session after installation.

## Gemini CLI

```sh
./dtw install gemini
```

PowerShell: `.\dtw.exe install gemini`. Install the `gemini` CLI first. DTW
registers the complete package through `gemini extensions install`. Only user
scope is supported; `--project` is rejected. Restart the Agent session to load
the extension's Skill and MCP server.

## Standard Agent Plugin export

To stage a complete [Agent Plugin 1.0](https://agent-plugins.org/) package for an embedding host:

```sh
./dtw install plugin --to "/absolute/new/plugin-directory"
```

PowerShell: `.\dtw.exe install plugin --to "C:\new\plugin-directory"`. The destination must not already exist. The result includes its own `bin/dtw` and `libexec/cua-driver`; keep those together. Its `mcp.json` declares one stdio server and `skills/desktop-world/SKILL.md` supplies Agent guidance. [Host integration contract →](embedding.md)

## Caelis

With the Caelis CLI installed, export the Plugin as above to a new permanent
absolute directory outside the workspace, then run:

```sh
caelis plugin install /absolute/path/to/plugin-directory --json
caelis plugin status desktop-world --json
```

PowerShell accepts an absolute path such as `"C:\Users\me\AppData\Local\desktop-world-plugin"`.
The equivalent TUI command is `/plugin install /absolute/path/to/plugin-directory`.
Installation enables the Plugin; `/plugin manage` controls future Sessions.
Start a new Session and verify Skill discovery and `desktop_world__exec` readiness.
`status` reports registration, not live MCP or worker readiness.

Caelis registers files in place. Keep the full package at the registered path.
For an update, finish active DTW work, export the new release into a separate
staging directory, then replace the contents of the existing registered root
with the complete staged package. Do not export directly over an existing
directory: `dtw install plugin --to` intentionally rejects that.
Repeat `caelis plugin install` at the same canonical root to refresh registration;
Caelis preserves the enabled/disabled choice. Start a new Session afterwards.
To switch to a different root, first remove the old registration explicitly.

```sh
caelis plugin remove desktop-world --json
```

Removal unregisters the Plugin and leaves its externally owned files in place.
See [Caelis's Plugin guide](https://github.com/caelis-labs/caelis/blob/main/docs/plugins.md)
for local Host/Store selection and ownership rules.

## Caelis Bot and embedding hosts

Caelis Bot includes its own pinned Computer Use Plugin. Update it through Bot's
distribution flow; installing a separate DTW release does not replace that copy.
For custom host integration, follow the [embedding contract](embedding.md).
Embedding hosts can download `desktop-world-plugin-<platform>.tar.gz` (macOS) or `.zip` (Windows), containing a runtime without the redundant self-extracting payload. Keep `bin`, `libexec`, manifests, Skill and licensing files together.

## Upgrade and remove

Download the newer platform `dtw` and repeat the install command with the **same scope or project**. The installer replaces only directories marked as its own and preserves unrelated host configuration. Stop active DTW sessions and start a fresh Agent session after updating.

```sh
./dtw uninstall agy
./dtw uninstall claude
./dtw uninstall codex
./dtw uninstall gemini
./dtw uninstall cursor
./dtw uninstall agy --project "/absolute/path/to/my project"
```

PowerShell uses `.\dtw.exe uninstall ...`. Claude removal unregisters only `desktop-world@dtw-local` and its DTW-owned local marketplace. User-scope Antigravity removal uses `agy plugin uninstall desktop-world` after checking DTW ownership; project-scope removal deletes only its installer-owned Plugin directory. For a generic `install plugin --to` export, the embedding host owns its lifecycle; remove it through that host.

## Permissions and first use

On macOS, grant Accessibility and Screen Recording to the app that launches the Agent CLI (for example, Terminal.app), then fully quit and reopen that app. The `agy` and `codex` command-line executables may not appear as separate apps in Privacy & Security. DTW's embedded worker checks the launching host's permissions but does not raise its own authorization prompt. If the host is absent from the list, use the **+** button in each permission pane to add it. A network proxy's authorization is separate from macOS desktop permissions. The signed CLI and worker should be checked on a fresh Mac before public release. On Windows, the x64 Visual C++ runtime is required; an elevated target app still follows Windows integrity restrictions.

Start with a read-only task: “Use the Desktop World Skill to inspect my open browser window and describe its controls. Do not click or type.” Next, use a temporary document and ask the Agent to type a short string and verify it appears. The [Skill](../packaging/plugin/skills/desktop-world/SKILL.md) explains native scripts, receipts, and readback; users can describe tasks in natural language.

## Protocol references

[Agent Plugin 1.0](https://agent-plugins.org/) · [Claude plugin components](https://code.claude.com/docs/en/plugins/components) · [Antigravity plugins](https://www.antigravity.google/docs/plugins)
