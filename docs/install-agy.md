# Install DTW in Antigravity (agy)

Start with the [common setup entry](../install.md). Use this guide only for
host-specific registration, reload, or recovery steps. Use the receipt's
`install.package_path` for registration.

Antigravity App (`agy-app`) and CLI (`agy-cli`) share one global plugin slot
with the default managed store profile `agy`. Use the App section inside
Antigravity App. Keep the complete package (`bin/`, `skills/dtw/`,
`.antigravity-plugin/plugin.json`, `mcp_config.json`) as **one plugin**. Do not
add a separate `agy mcp add` entry or a copied standalone Skill.

## Plugin slots

`dtw setup` can manage the default global plugin slot.

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

## Antigravity App

For a **first install** in the global slot, `dtw setup` copies the verified
package and pauses at `host_action_required`. Guide the user to open
[Desktop World](customization://desktop-world) in **Customizations → Installed**
and enable Desktop World with its bundled `dtw` Skill and `dtw` MCP server. The
Marketplace view only lists published catalog entries and need not list a local
DTW release package. Do not add a second DTW server under Installed MCP Servers.

After a [successful automatic live swap](#upgrade-an-occupied-slot), enable
Desktop World if needed. If it was already enabled, open
[Desktop World](customization://desktop-world) and restart **only `dtw`**
(**MCP → More actions (⋮) → Restart**). If the installer Agent has an
independently authorized way to operate that UI, use it. Otherwise ask the
user for this one click; the old `dtw` connection cannot be relied on to
restart itself. Do not reinstall or restart the App. If the per-server action
is unavailable, toggle Desktop World off and back on once.

After enabling or restarting the MCP server,
[resume using the setup receipt](../install.md#use-the-setup-receipt). Check
that the Language Server launched the expected executable from the slot after
installation or replacement, then use the fresh verifier below. Confirm the
receipt after live verification.

Only if an old Antigravity `dtw mcp` process still remains active after the MCP
restart or plugin toggle, ask the user to save active work and fully quit and
reopen Antigravity App, then start a new conversation and repeat the live check.

**Host enablement state (read-only):** Antigravity stores the global enabled flag
in `~/.gemini/config/config.json` (`%USERPROFILE%\.gemini\config\config.json`):

```json
{"plugins":{"desktop-world":{"enabled":true}}}
```

Inspect `config.json` read-only if needed. Use the supported UI for reload:
`agy` 1.5.15 has no `mcp restart` command. Do not hand-edit `config.json`,
kill the server or call private Language Server RPCs to simulate a restart.

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

In the default global profile, `dtw setup` validates the package with `agy plugin
validate` and checks registration with `agy plugin list --json`. For manual
registration, validate the verified `install.package_path` and inspect the
registration in the intended profile.

```sh
agy plugin validate "/absolute/managed/package" --json
agy plugin list --json
```

Proceed when validation accepts both the Skill and MCP server. Check the
intended slot on disk as well as in the listing. If it is empty and no other
Desktop World copy is discovered, copy the complete verified package into
place using the [copy rules](#plugin-slots). For an occupied slot, follow the
upgrade instructions below. Never run `agy plugin install` against an occupied
slot.

For the installed copy, check the path, version, enabled state, Skill, and MCP
entry in `agy plugin list --json`. If setup requested a host action, complete it
and [resume using the receipt](../install.md#use-the-setup-receipt). Start a
fresh CLI session for the live checks, then confirm the receipt.

## Upgrade an occupied slot

On macOS/Linux, `setup prepare` replaces the complete plugin in one atomic
operation while the host is running. This works **from the published v0.6.1**
when the slot exactly matches the previous managed pin and the verified
**destination** declares `"agy_live_swap": 2` in `BUILD.json`. The new binary
rejects worker launches from the old MCP. Finish any active DTW operation
before updating; after the exchange, restart only `dtw` MCP in the App to load
the new binary. Do not quit the App or move any files for this upgrade.

Use the offline procedure below only if setup cannot safely replace the slot
(for example, a locally modified or unrecognized slot, an occupied Windows
slot, or a downgrade to v0.6.1, which lacks the destination guard).

A successful swap reports the displaced tree in `registration.backup`. Keep it
outside discovery through live verification and confirmation. Afterward, you
can remove it if an inspection finds no local data you need to keep.

If setup reports an uncertain exchange, keep the displaced tree at
`registration.backup` outside discovery and leave the transaction pending.
Inspect both trees after both surfaces stop. Do not attempt a second swap.

### Replace offline

1. Save the receipt and store root. Ask the user to save work and fully stop
   **both Antigravity App and all `agy` CLI sessions**. Wait for their DTW
   connections to exit before changing the slot.
2. Confirm the occupied directory is the intended Desktop World slot. Stage a
   complete copy of the verified `install.package_path` outside plugin discovery
   and check it matches the managed package. Move the old tree to a backup
   outside discovery and put the staged copy into the empty slot. Preserve
   unrelated files and stop if the slot changes unexpectedly.
3. Reopen the intended host, enable Desktop World if needed, and
   [resume using the setup receipt](../install.md#use-the-setup-receipt). Verify
   the live MCP connection and matching Skill in a fresh context, then confirm
   the receipt. Keep the old tree outside discovery until verification and
   confirmation succeed.

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
manually copied Skill before enabling the plugin route.

To remove DTW, uninstall or disable it through Antigravity App Customizations or
use `agy plugin uninstall desktop-world`. Let its connections exit cleanly,
then [unbind the receipt's profile in its original store](package-management.md#remove-a-host-installation).

Host references: [Plugins](https://antigravity.google/docs/plugins),
[plugin layout and path variables](https://antigravity.google/docs/build-plugins),
[normal subagents](https://antigravity.google/docs/subagents/),
[MCP configuration](https://antigravity.google/docs/mcp).
