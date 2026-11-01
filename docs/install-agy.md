# Install DTW in Antigravity (agy)

First [download and verify the latest stable DTW package](../install.md#select-and-extract-a-package)
for your OS. Keep the complete extracted directory, including `bin/`,
`skills/`, `.antigravity-plugin/plugin.json` and `mcp_config.json`.
Install this directory as **one plugin**; a separate `agy mcp add` registration
or a copied standalone Skill is not needed.

On Windows, run the common [download/verification recipe](../install.md#windows-x64-download-and-verify-once)
once and use its returned `package_path` below. Choose the App section when
installing for Antigravity App; an `agy` CLI lookup is not part of that route.

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

For the tested Windows App **2.28.2**, the global enabled state is the following
subtree in `%USERPROFILE%\.gemini\config\config.json`:

```json
{"plugins":{"desktop-world":{"enabled":true}}}
```

Use the App's normal enable control or merge only that setting with the existing
file, preserving other plugins and `userSettings`. This fragment is not a whole
replacement config. The installed package already supplies its MCP config and
Skill; keep their bundled relative paths.

If this Windows installation conversation still has its original tool snapshot,
complete the read-only check with [one fresh verifier](#fresh-verifier-for-the-installing-conversation).

For subsequent desktop work, start a **new conversation** in the intended
workspace. An already running conversation may retain its previous tool/Skill
snapshot. A new conversation
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
inside the global plugin directory. A clean installation enabled the plugin and
verified `apps` plus three `app.windows()` calls through a fresh verifier without
an App restart. An earlier upgrade check needed a normal full App close/reopen
because an old MCP process survived manifest refresh while the UI remained green.
This App verification does not certify the CLI surface.

### Fresh verifier for the installing conversation

Antigravity App exposes the normal host tool `invoke_subagent`. In the tested
Windows App, a fresh `self` context saw the newly enabled plugin even while the
installing conversation retained its original tools. Invoke it once with the
complete read-only task, using the host's exposed argument schema:

```json
{
  "TypeName": "self",
  "Role": "Desktop Verifier",
  "Model": "inherit",
  "Workspace": "inherit",
  "Prompt": "Verify the installed desktop-world plugin using its MCP tools. Call apps, choose a returned canonical app ID unchanged, then call exec using that app with source set to 'return await app.windows();'. Report the actual tool result or error. Do not use shell commands, private host RPCs, binary scans, UI input, or file writes."
}
```

Use its normal completion reply. In this App the exposed MCP server name is
`desktop-world_desktop-world`, with `apps`, `api`, `exec` and `result` tools.
The fresh verifier tests the existing installation; it does not install another
copy or register another MCP server. Reading subagent transcript files or
probing Language Server endpoints is not part of this check.

If a fresh verifier is unavailable or still lacks DTW tools, report the verified
package path, enabled state and exact missing-tool condition, then give the
single next step: start a new conversation. Use the normal App restart recovery
above only when the enabled plugin is absent or its MCP still runs the old path.

## Antigravity CLI

Use this section when installing for the **agy CLI**. It can validate and install
that same directory:

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
[normal subagents](https://antigravity.google/docs/subagents/),
[MCP configuration](https://antigravity.google/docs/mcp).
