# Install DTW in DeepSeek Harness

First [download and verify DTW](../install.md#select-and-extract-a-package), extract
it to a permanent directory, and start DSH once. This guide checks the shipped
DSH **0.2.1-rc.5** configuration format. On Windows, use the direct file-URL row
below as the first route. Other versions should use their installed MCP client
and Skill provider schemas. The bundled Cordis adapter mounts the existing MCP
bridge and filesystem Skill provider together as one plugin.

## Bundled Cordis plugin

### Windows DSH 0.2.1-rc.5: one file-URL row

Use the common [Windows recipe](../install.md#windows-x64-download-and-verify-once)
once. Its `cordis_url` output is the encoded absolute URL of the verified package's
`cordis/index.mjs`. In the running App's active Desktop profile, back up
`cordis.patch.yml` (default: `~/.dsh/profiles/desktop/cordis.patch.yml`; a custom
DSH home changes the root) and merge this one row, replacing the example `name`
with that returned URL:

```yaml
- insert:
    - id: desktop-world-plugin
      name: 'file:///C:/Tools/desktop-world/cordis/index.mjs'
```

Keep the complete package at its returned permanent path. If the row already
exists, update its `name` after stopping its old DTW connection; do not insert
a second row. Remove only old standalone DTW MCP/Skill registrations when
migrating, preserving other providers and Skill roots. This tested route does
not require a marketplace/npm lookup, a local-directory package install, or
the manual MCP fallback below. Retain the first loader error if it fails.

With profile hot reload enabled, the one row starts the bundled executable and
Skill without restarting the App. Use a fresh session for the live checks below;
if only the installing session has an old tool snapshot, use a normal read-only
verifier subagent or report that a new session is needed.

The adapter asks the host loader for its installed
`@deepseek-ai/dsh-mcp-client` and `@deepseek-ai/dsh-skill-filesystem` plugins. It
mounts one `desktop-world` MCP connection and a Skill provider named
`desktop-world`, scanning only the bundled `skills/` directory. It does not
install a second DSH runtime or rescan all default Skill roots. Its child plugins
are disposed with the parent by Cordis.

### Other local-package routes

For a host version that supports local package installation, the plugin manager
can install the complete extracted package using its absolute directory and
enable `@caelis-labs/desktop-world`. The package name is not a claim of npm
publication. A direct file URL also works as above (`file:///absolute/path/...`
on macOS), with spaces and non-ASCII characters escaped. The adapter derives
binary and Skill paths from its own location; it takes no path configuration.

Start a new session and run [the live checks](#verify-the-running-installation).
Check disabling/re-enabling the parent plugin removes/restores its DTW tools
and Skill without duplicate namespace diagnostics. If the host lacks these
loader/provider interfaces, retain the manual route below and report the exact
DSH version; no DSH core change is required by DTW.

On Windows with DSH **0.2.1-rc.5**, the preferred file-URL route has been checked from a
package path containing spaces and non-ASCII characters. Profile hot reload
stopped the previous standalone DTW connection and launched the package's
bundled executable without restarting the App. Process identity verifies the
loaded package; complete the session tool and screenshot checks separately.

## Manual fallback: register the MCP server

In the running Desktop profile, add the following to `cordis.patch.yml` without
replacing its existing entries. The default path is
`~/.dsh/profiles/desktop/cordis.patch.yml`; a custom DSH home changes that root.
Use the profile used by the running application, not an unrelated CLI profile.
Back up the file before editing and keep unrelated settings intact.

```yaml
- insert:
    - id: mcp-desktop-world
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: desktop-world
        transport: stdio
        command: '/absolute/path/to/extracted-package/bin/dtw'
        args: ['mcp']
```

On Windows, use an absolute path such as
`command: 'C:\Tools\desktop-world\bin\dtw.exe'`. YAML single quotes keep the
backslashes literal. The command is the executable, not a shell command string;
`mcp` belongs in `args`.

**New rows require `insert`.** This is not a valid way to add a new server:

```yaml
# An override only: ignored with a diagnostic if the id does not already exist.
- id: mcp-desktop-world
  name: '@deepseek-ai/dsh-mcp-client'
  config: {serverName: desktop-world, transport: stdio, command: '/path/to/dtw', args: ['mcp']}
```

A bare `id` patch changes an existing row. For subsequent changes, edit the
original inserted row or override its existing ID. An override's `config`
replaces the whole config; it does not merge individual fields. Do not insert a
second row with the same server name. DSH can remain usable while this optional
MCP bridge fails, so a working chat does not prove DTW loaded.

## Make the Skill discoverable

Use DSH's existing `@deepseek-ai/dsh-skill-filesystem` provider. Choose one route:

- For one workspace, copy the entire package's `skills/desktop-world` directory
  into `<workspace>/.agents/skills/desktop-world` and start a session there.
- For all workspaces using this profile, add the package's absolute `skills`
  directory to the existing provider's `customSkillDirs`. Inspect the active
  provider's ID and preserve its other configuration and Skill roots. Do not
  guess an ID or replace its full config with only this field.

The directory to add to `customSkillDirs` is the parent `skills`, not
`skills/desktop-world`. The provider discovers immediate `<skill>/SKILL.md`
bundles. Confirm that the Agent can load the `desktop-world` Skill and its
references from the same DTW version as the executable.

## Verify the running installation

With configuration hot reload enabled, profile patch changes take effect without
restarting DSH. Allow a few seconds, then check the live connection. If hot reload
is disabled, restart after saving the configuration. Do not repeatedly restart
an otherwise working host to compensate for an invalid patch.

On macOS, these checks do not change your desktop:

```sh
/absolute/path/to/extracted-package/bin/dtw --version
pgrep -fl '/absolute/path/to/extracted-package/bin/dtw'
```

For a Windows process check, inspect the executable path and arguments in Task
Manager. A `dtw mcp` process shows that something launched it, not that the correct
DSH session has working tools. DSH may also start a temporary negotiation probe;
process count alone does not diagnose duplicate configuration.

In a DSH session:

1. Discover the DTW tools (normally names such as
   `mcp__desktop-world__apps`, `mcp__desktop-world__exec`).
2. Call `apps`, choose one of its app IDs, and call `exec` with that `app` and
   `source: "return await app.windows();"`.
3. Load the Skill, locate a test window and capture it through the documented
   screenshot API. Confirm the returned image reaches the model.

The tested model route is `deepseek-flash` (DeepSeek-V4.1-Flash); image delivery
also depends on the selected model and DSH attachment support. Start a new
session when checking changed session/preset configuration.

**Desktop is not a CLI profile to boot.** In this DSH version,
`dsh --profile desktop --dump-config` is rejected with
`profile "desktop" is managed exclusively by the Electron application`.
Use the running application's plugin/configuration view, diagnostics and actual
tool calls to check Desktop. Do not start a parallel CLI Desktop host as a
workaround. Avoid posting full profile dumps, which may contain credentials.

## macOS permissions

The first relevant GUI operations need **Accessibility** (UI inspection/input)
and **Screen Recording** (screenshots). In the reported Desktop installation,
macOS attributed access to **DeepSeek Harness**, not a separate DTW entry.
Inspect the system prompt and **System Settings → Privacy & Security** for the
host that actually launched this connection. A shell-launched DTW may instead
be associated with its terminal host. Host identification is a useful hint;
DTW cannot prove which application macOS records as TCC's responsible process.

After granting Screen Recording, fully quit and reopen **DeepSeek Harness** when
macOS requests it or capture remains denied. Save active work first. Merely
reconnecting `dtw mcp` may leave the host's old permission state in effect.
Repeat the read-only `apps`, window listing and screenshot checks before resuming
input; do not replay an earlier write whose outcome was uncertain.

Empty window titles alone do **not** prove an Accessibility denial. Check the
permission diagnostic, app/window availability and whether the desktop is
unlocked. A successful window list also does not establish screenshot access:
Accessibility and Screen Recording must be checked separately. If authorized
screenshots still fail, retain the error and inspect desktop visibility and the
specific window instead of repeatedly toggling permissions.

## Multiple Agents, upgrades and troubleshooting

All Agents can reference one installed DTW package and its matching Skill.
Separate stdio MCP connections start separate runtimes with their own state;
DSH decides whether Agents share a bridge or open separate connections. Multiple
Agents still operate on the same real applications, so sharing installation
files does not isolate their desktop effects. Do not duplicate the installation
per model or use process count as a session count.

If DTW tools are missing, check the correct profile, `insert` syntax, unique
server name, executable path, and DSH bridge diagnostics before restarting.
If tools work but input or capture fails, inspect native permission and window
errors rather than reinstalling the bridge. If the Skill is missing, check its
root directory and active filesystem provider.

For upgrades, finish active operations, stop old connections and point the
existing server entry and Skill root to the new verified package. Check the
version of that exact executable and repeat live tool checks. Preserve other
MCP servers, model settings and Skill roots. To remove DTW, remove only its
inserted row/overrides and Skill registration after connections have stopped.

For upstream details, see DSH's [MCP client](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/mcp/mcp-client).
