# Install DTW in DeepSeek Harness

Start with the [common setup entry](../install.md). Use this guide only for
host-specific registration, restart recovery, or manual setup steps. For a
prepared package, use the receipt's `install.package_path` and
`install.cordis_url`; do not import or bind it again. Finish with
`setup confirm dsh-desktop dsh-desktop <package_id>` using the exact receipt ID
after live verification.

This guide covers DSH Desktop's `cordis.patch.yml` file-URL plugin format, where
the bundled Cordis adapter mounts both the `dtw` stdio MCP client and the
`desktop-world` Skill provider from one verified package. Other DSH versions
should use their supported MCP client and Skill provider schemas.

## Windows host execution and storage

Use the host's supported executable-launch path and permissions for both
installation and the later MCP process. On Windows, extract and launch bootstrap
from an approved external directory **outside the session workspace** (and
outside any source checkout)—the common guide uses `%LOCALAPPDATA%\dtw-stage`,
or a host-approved `DTW_STAGING_DIR`.

Before installation, verify that the host permits both execution from that
staging directory and access to `%LOCALAPPDATA%\DesktopWorld` (or an explicitly
authorized `--store` directory). If the host denies either path, stop and use
its normal authorization flow; never alter sandbox policies, disable
protections, elevate opportunistically, or modify filesystem ACLs to force
access. Always register the immutable store payload (`install.executable`,
`install.package_path`, and `install.cordis_url`), never a temporary staging
copy. Keep the MCP executable and bundled Skill on the same verified package.

If a store or package command reports access denied, retain the exact error (note
that `package --help` and CLI argument validation run before opening the store,
so argument errors do not indicate corrupt archives) and obtain normal host
authorization before retrying.

## Bundled Cordis plugin

### DSH Desktop: one file-URL row

In the running App's active Desktop profile, back up `cordis.patch.yml`
(default: `~/.dsh/profiles/desktop/cordis.patch.yml`; a custom DSH home changes
that root) and keep one `desktop-world-plugin` row whose `name` equals the
receipt's `install.cordis_url` (or `dtw setup` edits that row automatically on
macOS/Linux when the file uses the standard YAML block):

```yaml
- insert:
    - id: desktop-world-plugin
      name: 'file:///C:/Tools/desktop-world/cordis/index.mjs'
```

Keep the complete package at its immutable store path. If the row already
exists, update its `name` rather than inserting a duplicate row, and remove any
stale standalone DTW MCP/Skill entries while preserving unrelated providers and
Skill roots. Retain the first loader diagnostic if loading fails.

Editing `cordis.patch.yml` is **not** a live reload in DSH Desktop: the running
application keeps its earlier plugin instance active, and neither the Plugins
refresh action nor a new conversation reloads an external patch. Save the patch
once, retain the pending pin (`setup` reports `restart_required`), report
**prepared; waiting for DSH restart**, and ask the user for one normal
quit/reopen after saving work. Never probe private RPCs, scan host binaries,
toggle unrelated plugins, kill shared bridges, or spawn repeated verifiers to
bypass that restart.

After reopening DSH, perform one live check. A fresh verifier is only useful
once the restarted host runs the new package; a standalone MCP handshake checks
the binary, not DSH's own connection.

The Cordis adapter delegates to DSH's `@deepseek-ai/dsh-mcp-client` and
`@deepseek-ai/dsh-skill-filesystem` plugins, mounting one `dtw` MCP connection
and a Skill provider named `desktop-world` scoped only to the package's `skills/`
directory. Child plugins are disposed cleanly by Cordis when the parent stops.

### Other local-package routes

For host versions supporting direct local package installation, install the
complete immutable directory from `install.package_path` and enable
`@caelis-labs/desktop-world` (which is a local package identifier, not an npm
package). Direct `file:///...` URLs with spaces and non-ASCII characters
properly escaped also work on macOS, Linux, and Windows; the adapter resolves
`bin/dtw` and `skills/dtw` relative to its own location. After host
reload/restart, run [the live checks](#verify-the-running-installation).

## Manual fallback: register the MCP server

Only if your DSH build lacks the Cordis plugin interface, add this row to
`cordis.patch.yml` in the running Desktop profile without replacing existing
entries (back up first):

```yaml
- insert:
    - id: mcp-desktop-world
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: dtw
        transport: stdio
        command: '/absolute/managed/package/bin/dtw'
        args: ['mcp']
```

On Windows, use single quotes such as
`command: 'C:\Tools\desktop-world\bin\dtw.exe'` so backslashes stay literal. Put
`mcp` in `args`, never in `command`. **New rows require `insert`**; a bare
`id:` entry only overrides an already-existing row and is ignored otherwise. Keep
server names unique. Because DSH keeps running if this optional bridge fails,
verify tools directly rather than assuming chat availability implies DTW
loaded.

## Make the Skill discoverable

When using the manual fallback above, expose the matching `skills/` directory
through `@deepseek-ai/dsh-skill-filesystem`:

- For one workspace, copy the package's `skills/dtw` directory into
  `<workspace>/.agents/skills/dtw`.
- For all workspaces in that profile, add the package's parent `skills`
  directory (not `skills/dtw`) to `customSkillDirs` on the existing provider
  while preserving all other configuration and Skill roots.

## Verify the running installation

Run these checks after DSH has restarted and loaded the updated registration.
Keep the transaction pending until the live connection and `dtw` Skill match the
intended package.

On macOS / Linux (non-intrusive):

```sh
/absolute/managed/package/bin/dtw --version
pgrep -fl '/absolute/managed/package/bin/dtw'
```

On Windows, inspect the running `dtw.exe` path and arguments in Task Manager.
Process presence alone does not confirm the active DSH session exposes tools or
that DTW isn't just running a temporary probe.

In a DSH session:

1. Discover DTW tools (typically `mcp__dtw__apps`, `mcp__dtw__exec`).
2. Call `apps({state:"running"})`, pick an authorized non-document app ID, and
   run `exec` with `source: "await app.look();"`.
3. Load the `dtw` Skill, capture an authorized test window, and confirm the
   screenshot image reaches the model (image delivery depends on DSH attachment
   support and the selected model; start a new session after configuration or
   preset changes).

**Desktop is managed by the Electron app:** do not run
`dsh --profile desktop --dump-config` (DSH rejects it for the Electron-managed
`desktop` profile) and never boot a parallel CLI host or paste profile dumps
containing credentials.

## macOS permissions

GUI observation and input require **Accessibility** and **Screen Recording**
grants under **System Settings → Privacy & Security** for **DeepSeek Harness**
(the launching host application; shell-launched runs attribute to the terminal).

After granting Screen Recording, save active work and fully quit and reopen
DeepSeek Harness if macOS requests a restart or capture remains denied. Repeat
read-only `apps`, `app.look()`, and `app.capture()` before attempting input,
and check both permissions separately if either fails instead of toggling
settings repeatedly.

## Multiple Agents, upgrades and troubleshooting

All DSH Agents can reference the same immutable DTW package and Skill. Separate
stdio connections run independent execution state while still acting on the same
desktop applications; DSH decides connection sharing.

- Missing DTW tools: check the active profile, `insert` syntax, unique server
  name, executable path, and DSH bridge diagnostics before restarting.
- Tool errors on capture/input: inspect TCC/native diagnostics rather than
  reinstalling.
- Upgrades: finish active operations, point the existing registration to the new
  package, restart DSH once, verify live tools and Skill, and confirm with
  `setup confirm`. Keep unrelated servers, settings, and Skill roots intact.
- Removal: remove DTW's inserted entry (and any manual Skill registration)
  after connections exit, then run `dtw package unbind dsh-desktop` with the
  managed executable.

## Resume after restart and finish cleanup

Before waiting for restart, keep the setup receipt (`package_id`,
`install.executable`, host `dsh-desktop`, profile `dsh-desktop`) and report only
the pending restart step—do not dump checksums or internal inventories as a
success report. After restart, run `install.executable setup resume dsh-desktop
dsh-desktop`, verify the live connection and `dtw` Skill once, and finish with
`setup confirm dsh-desktop dsh-desktop <package_id>`.

For upstream details, see DSH's
[MCP client](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/mcp/mcp-client)
and [managed package lifecycle](package-management.md).
