# Install DTW in Antigravity (agy)

First [download and verify the latest stable DTW package](../install.md#select-and-extract-a-package)
for your OS. Keep the complete extracted directory, including `bin/`,
`skills/`, `.antigravity-plugin/plugin.json` and `mcp_config.json`.
Install this directory as **one plugin**; a separate `agy mcp add` registration
or a copied standalone Skill is not needed.

## Antigravity App

For a local package, place the complete extracted package in one discovery root:

- One workspace: `<workspace>/.agents/plugins/desktop-world/`.
- All workspaces: `~/.gemini/config/plugins/desktop-world/` (on Windows,
  `%USERPROFILE%\.gemini\config\plugins\desktop-world\`).

The manifest must be directly inside that plugin directory's
`.antigravity-plugin/`, not inside another nested release folder. Copy the whole
package rather than only its manifests: its MCP server launches the bundled
`bin/dtw.exe` on Windows or `bin/dtw` on macOS.

Open **Customizations → Installed**, locate **Desktop World**, and check that
its Skill and MCP components are present and enabled. The Marketplace view is
for browsing published plugins; a local DTW release package need not be listed
there. Do not add a second DTW server under Installed MCP Servers.

Start a **new conversation** in the intended workspace. An already running
conversation may retain its previous tool/Skill snapshot. A new conversation
does not necessarily restart an existing plugin MCP process. During upgrades,
check the actual executable path of the MCP process launched by Antigravity's
Language Server: it must be the installed plugin's `bin/dtw.exe` on Windows,
not the previous external DTW installation. A green plugin/MCP status or a
refreshed manifest can still describe a cached connection to the old executable.

If the plugin remains absent, or the MCP still uses the old path after checking
the package and enabled state, save active work, fully quit and reopen the App,
then use a new conversation and repeat the live checks. A restart is recovery,
not a substitute for a valid package or a successful tool call.

**Verified App scope:** Windows, Antigravity App **2.28.2**. The bundled relative
command (`./bin/dtw.exe`) with `cwd: "${PLUGIN_ROOT}"` launched the executable
inside the global plugin directory after a normal full App close/reopen. The
upgrade check also observed an old MCP process surviving manifest refresh while
the UI remained green. This App verification does not certify the CLI surface.

## Antigravity CLI

If the `agy` CLI is installed, it can validate and install that same directory:

```sh
agy plugin validate --strict "/absolute/path/to/extracted-package"
agy plugin install "/absolute/path/to/extracted-package"
agy plugin list --json
```

Use a quoted absolute Windows path when applicable. Inspect the installed path,
version, enabled state, Skill and MCP entries, then start a new session. The CLI
and App share global plugin storage; do not install another copy just because
you use both surfaces.

## Verify the actual plugin connection

1. Load the bundled `desktop-world` Skill (or invoke `/desktop-world`).
2. Discover the plugin's `apps`, `api`, `exec` and `result` tools. agy may qualify
   their names with the plugin and server names; use the names it actually exposes.
3. Call `apps`, then `exec` with a returned app ID and
   `source: "return await app.windows();"`.
4. Capture a disposable test window through DTW and confirm the image reaches
   the model before trying input. See [the common installation check](../install.md#check-the-installation).

A plugin card, a running `dtw mcp` process, and a successful terminal command are
different checks. None establishes by itself that this conversation has tools,
the MCP-launched native worker can see your desktop, or screenshots reach the
model. Retain the exact native diagnostic if enumeration or capture fails.
An isolated terminal desktop does not prove the plugin MCP uses that desktop.
DTW cannot grant itself access denied by the host's sandbox or Windows desktop
ACLs. Use the host's supported MCP execution route; do not disable sandbox or
security settings to work around a missing tool.

The agy manifest lives in `.antigravity-plugin/` so it does not conflict with
the standard Agent Plugins root manifest or Claude metadata. Its MCP config
uses the plugin working directory and a relative bundled executable; it does
not depend on `${CLAUDE_PLUGIN_ROOT}` or a hardcoded external DTW installation.

## Migrate, upgrade or remove

Finish active operations before changing the installation. Preserve unrelated
plugins and settings. Remove only the previous standalone DTW MCP registration
and copied `desktop-world` Skill, if present, before enabling the plugin route.
Do not leave workspace and global copies of the same plugin active together.

For an upgrade, verify the new complete package, stop old DTW connections, replace
only its plugin directory, and start a new conversation. Verify the running MCP
executable identity as well as its reported version; when a cached old connection
remains, fully close/reopen the App before accepting the upgrade. Keep a stopped
old package until the new installation passes the checks above. To remove DTW, use
the App's plugin removal controls or `agy plugin uninstall desktop-world`, and
remove any old standalone DTW registration separately if one remains.

Host references: [Plugins](https://antigravity.google/docs/plugins),
[plugin layout and path variables](https://antigravity.google/docs/build-plugins),
[MCP configuration](https://antigravity.google/docs/mcp).
