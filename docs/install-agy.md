# Install DTW in Antigravity (agy)

Start with the [common setup entry](../install.md). Use this guide only for
host-specific registration, reload, or recovery steps. Use the receipt's
`install.package_path`; do not import or bind the package manually. Finish with
`setup confirm agy-app agy <package_id>` (or `agy-cli agy <package_id>`) using
the exact receipt ID after live verification.

Antigravity App (`agy-app`) and CLI (`agy-cli`) share one global plugin slot and
the same managed store profile `agy`. Choose the App section when running inside
Antigravity App; an `agy` CLI lookup is not part of that route. Keep the
complete package (`bin/`, `skills/dtw/`, `.antigravity-plugin/plugin.json`,
`mcp_config.json`) as **one plugin**; do not add a separate `agy mcp add` entry
or a copied standalone Skill.

## Antigravity App

`dtw setup` manages the global plugin slot automatically:

- Global (all workspaces): `~/.gemini/config/plugins/desktop-world/` (macOS / Linux)
  or `%USERPROFILE%\.gemini\config\plugins\desktop-world\` (Windows).
- One workspace (manual): `<workspace>/.agents/plugins/desktop-world/`.

Do not leave workspace and global copies of `desktop-world` active together.
When copying the package manually into a slot, follow two rules:

1. Copy **all** hidden files and directories (especially `.antigravity-plugin/plugin.json`
   and `.mcp.json`); the manifest must sit directly at `.antigravity-plugin/plugin.json`
   inside `desktop-world/`, never inside a nested directory.
2. Keep any staging or backup directory **outside** `plugins/` (for example
   under `/tmp`); Antigravity scans every subdirectory under `plugins/` and
   silently drops plugins with duplicate names.

### First install, upgrade, and reload inside Antigravity App

For a **first install** in the global slot, `dtw setup` copies the verified
package and pauses at `host_action_required`. Guide the user to open
[Desktop World](customization://desktop-world) in **Customizations → Installed**
and enable Desktop World with its bundled `dtw` Skill and `dtw` MCP server. The
Marketplace view only lists published catalog entries and need not list a local
DTW release package. Do not add a second DTW server under Installed MCP Servers.
After enabling, run `setup resume agy-app agy`, complete the live check with a
fresh verifier below, and finish with `setup confirm agy-app agy <package_id>`.

For a **live upgrade** of an existing global DTW slot:

- **macOS / Linux (single reload action for pinned slots):** When the existing
  slot matches the previously managed `agy` pin in the setup store, `dtw setup`
  atomically swaps the replacement copy into `desktop-world/` in place and
  re-verifies the displaced copy before cleanup (aborting cleanly or preserving
  any concurrently changed directory outside `plugins/`). No prior disable or
  manual copy is needed. Unpinned, custom, or modified slots are left untouched
  for manual review. When the plugin is already enabled, `setup` pauses once at
  `host_action_required`:
  1. Ask the user to open [Desktop World](customization://desktop-world) once and
     restart its `dtw` MCP server (**MCP → More actions (⋮) → Restart**, or
     toggle Desktop World off and back on once).
  2. Run `setup resume agy-app agy` (returns `verify`), confirm Antigravity's
     Language Server launched the new slot binary (`bin/dtw`, not an old
     process whose binary was replaced on disk), run the fresh `self` verifier
     below, and finish with `setup confirm agy-app agy <package_id>`.
- **Windows (non-destructive occupied slot):** `dtw setup` populates an empty
  slot automatically; for any already occupied slot, it leaves the existing
  directory intact and pauses at `host_action_required`. Ask the user to disable
  [Desktop World](customization://desktop-world), replace the entire slot from
  `install.package_path` with staging outside `plugins/` while preserving
  unrelated files, re-enable Desktop World, then run `setup resume agy-app agy`
  and complete live verification and `setup confirm`.

Only if an old Antigravity `dtw mcp` process still remains active after the MCP
restart or plugin toggle, ask the user to save active work and fully quit and
reopen Antigravity App, then start a new conversation and repeat the live check.

**Host enablement state (read-only):** Antigravity stores the global enabled flag
in `~/.gemini/config/config.json` (`%USERPROFILE%\.gemini\config\config.json`):

```json
{"plugins":{"desktop-world":{"enabled":true}}}
```

Inspect `config.json` read-only if needed, but always let the user toggle or
restart via `[Desktop World](customization://desktop-world)`: hand-editing
`config.json` or calling private Language Server mutation RPCs bypasses live
reload and risks clobbering concurrent `userSettings` or permission grants.

For subsequent desktop work, start a **new conversation** in the intended
workspace because an already running conversation keeps its earlier tool/Skill
snapshot. During upgrades, always confirm both the running MCP executable path
and refreshed tool/Skill identity, not only a green plugin card.

### Fresh verifier for the installing conversation

Antigravity App exposes `invoke_subagent`. A fresh `self` context loads the
newly enabled or restarted plugin's Skill and MCP tools even while the
installing conversation retains its earlier snapshot. Invoke it once with this
read-only task:

```json
{
  "TypeName": "self",
  "Role": "Desktop Verifier",
  "Model": "inherit",
  "Workspace": "inherit",
  "Prompt": "Verify the installed desktop-world plugin using the live host connection. Check that the loaded dtw Skill matches the installed package version, call apps with state='running', choose one unchanged canonical app ID for a neutral non-document window (never personal user documents), and call exec once with source='await app.look(); await app.capture();'. Report the actual view status, whether the screenshot image arrived clearly, and any errors verbatim. Do not run shell commands, private host RPCs, binary scans, UI input, or file writes."
}
```

Use its normal completion reply. The plugin ID is `desktop-world` and its MCP
server key is `dtw` (Antigravity may expose tools under `desktop-world_dtw`
such as `apps` and `exec`); discover and call the actual host-exposed names. The
fresh verifier tests the existing installation; do not install another copy,
register another MCP server, read subagent transcript files, or probe Language
Server endpoints.

If a fresh verifier is unavailable or still lacks the updated tools/Skill,
report the verified package path, enabled state, and exact missing-tool
condition, then give the single next step: start a new conversation (or use the
normal App restart recovery above if an old MCP process persists).

## Antigravity CLI

When installing or upgrading for **agy CLI** in its default global profile,
`dtw setup` validates the package with `agy plugin validate`, atomically installs
or replaces the global slot, and confirms the result with `agy plugin list --json`.
If manual validation or registration is needed:

```sh
agy plugin validate --strict "/absolute/managed/package"
agy plugin install "/absolute/managed/package"
agy plugin list --json
```

Use a quoted absolute path on Windows when applicable. Inspect the installed
path, version, enabled state, Skill, and MCP entries, then start a new session.
The CLI and App share global plugin storage; do not install a second copy when
using both surfaces.

## Verify the actual plugin connection

1. Load the bundled `dtw` Skill (or invoke `/dtw`).
2. Discover the plugin's `apps` and `exec` tools using the names exposed by
   Antigravity.
3. Call `apps({state:"running"})`, then `exec` with a returned app ID for a safe
   non-document window and `source: "await app.look(); await app.capture();"`.
4. Confirm both the `look` view and the screenshot image reach the model before
   trying input. See [the common installation check](../install.md#check-the-installation).

A plugin card, a running `dtw mcp` process, and a terminal command are separate
checks. None establishes by itself that the active conversation has tools, the
MCP-launched native worker can access the desktop, or screenshots reach the
model. Retain any native diagnostic verbatim if enumeration or capture fails;
DTW cannot grant itself permissions denied by macOS TCC, the host sandbox, or
Windows desktop ACLs.

The agy manifest lives in `.antigravity-plugin/` with `mcp_config.json` using
`${PLUGIN_ROOT}` and a relative bundled `./bin/dtw` executable.

## Upgrade or remove

Finish active operations before changing the installation and preserve unrelated
plugins and settings. Remove any duplicate standalone DTW MCP registration or
manually copied Skill before enabling the plugin route. Confirm the upgrade
with `setup confirm` only after the new live MCP connection and matching Skill
are verified; keep the previous package in the store rollback pin until then.

To remove DTW, uninstall or disable it through Antigravity App Customizations or
`agy plugin uninstall desktop-world`, let its connections exit cleanly, and run
`dtw package unbind agy` with the managed executable. Package GC only manages
the distribution store; see [managed package lifecycle](package-management.md).

Host references: [Plugins](https://antigravity.google/docs/plugins),
[plugin layout and path variables](https://antigravity.google/docs/build-plugins),
[normal subagents](https://antigravity.google/docs/subagents/),
[MCP configuration](https://antigravity.google/docs/mcp).
