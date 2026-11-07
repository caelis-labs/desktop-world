# Install DTW in DeepSeek Harness

First [download and verify DTW](../install.md#select-and-extract-a-package), import
it into the managed store. This guide covers the DSH **0.2.1-rc.5** Desktop
configuration format as a tested compatibility baseline, not a required current
host version. Use the direct file-URL row below when that format is supported.
Other versions should use their installed MCP client
and Skill provider schemas. The bundled Cordis adapter mounts the existing MCP
bridge and filesystem Skill provider together as one plugin.

## Windows host execution and storage

Use the host's supported executable-launch path and permissions for installation
as well as the later MCP process. Download and extract the verified complete
package to a host-approved temporary directory **outside the session workspace**
(and outside the source checkout) before invoking its `bin/dtw.exe package import`.
The common Windows recipe uses a unique child under the user's Local Application
Data `dtw-stage` directory. A host-approved absolute `DTW_STAGING_DIR` can
override it. Confirm permission for the resolved path before running it.

[Issue #112](https://github.com/caelis-labs/desktop-world/issues/112) records
DSH Desktop 0.2.2-rc.1 redirecting TEMP under workspace-write. The same verified
CLI could print its version from that directory but failed to open the managed
store. The approved external launch of the same bytes could open the store.

[Issue #104](https://github.com/caelis-labs/desktop-world/issues/104) reports an A/B
check on Windows 11 with DSH Desktop 0.2.1-rc.5 and DTW 0.3.3: identical executable
bytes inside the session workspace failed to access the default external store
with `os error 5`, even with the host's `danger-full-access` setting. The same
executable launched from an external temporary directory succeeded while its
working directory remained inside the workspace. This is a reported host/path
compatibility condition, not a claim about every DSH version or proof that every
access-denied error has that cause.

Before installation, establish that the host permits both execution from the
external staging path and access to `%LOCALAPPDATA%\DesktopWorld` (or an explicitly
authorized dedicated store). If the host denies either operation, stop and use
its normal authorization process or an administrator-documented supported route.
Do not change sandbox policy, disable protection, elevate opportunistically,
or change filesystem ACLs. Moving an executable is not permission to bypass a
host restriction; external staging applies only when that route is supported
and authorized.

After import, use its returned immutable `executable`, `package_path` and
`cordis_url` for management and registration; do not register the temporary copy.
The default managed package is outside the workspace, but the host must still
permit its MCP launch and required file/desktop access. Keep the MCP executable
and Skill on the same verified release.

If `package import` or `package status` reports access denied, retain the exact
error and check the resolved executable path, staging path and store against the
host's supported execution policy. Current package CLI help and argument
validation run before opening the requested store; older releases, including the
one in #104, could fail during store initialization even for `package --help`.
A help failure does not by itself show that the archive is corrupt. Use the
selected package's own CLI contract; updated documentation does not change an
older executable. Do not diagnose this by repeatedly changing `--store`, granting
broader permissions or editing ACLs. Follow the normal host approval route and
retry only once the required access is authorized.

## Bundled Cordis plugin

### DSH Desktop: one file-URL row

On Windows use the common [recipe](../install.md#windows-x64-download-and-verify-once)
once; on macOS use the package import from the common guide.
The `cordis_url` output is the encoded absolute URL of the verified package's
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
exists, update its `name`; do not insert
a second row. Remove only old standalone DTW MCP/Skill registrations when
migrating, preserving other providers and Skill roots. This tested route does
not require a marketplace/npm lookup, a local-directory package install, or
the manual MCP fallback below. Retain the first loader error if it fails.

Editing the file is not a live reload. In the reported macOS Desktop 0.2.1-rc.5
installation, the running profile did not reload this external patch; the old
MCP and Skill remained active. The Plugins refresh button and a fresh conversation
or verifier do not prove that the parent plugin was reloaded.

Use an already documented host operation to replace/reload this same DTW plugin
if available. Otherwise save the patch once, retain the pending pin and old live
package, report **prepared; waiting for DSH restart**, and stop. Ask only for one
normal quit/reopen after the user saves work. Do not reverse engineer private RPCs,
scan host binaries, toggle unrelated plugins, kill a shared bridge, or repeatedly
spawn verifiers to discover a reload workaround.

After the host reload/restart, perform one live check. A fresh verifier is useful
only when that host already runs the new package and this conversation still
has old tools or Skill. A directly launched new MCP handshake is a package check,
not proof that DSH switched its existing connection.

A required restart is not a plugin-load failure. Do not add a manual MCP/Skill
registration or switch installation routes merely to avoid that restart.

The adapter asks the host loader for its installed
`@deepseek-ai/dsh-mcp-client` and `@deepseek-ai/dsh-skill-filesystem` plugins. It
mounts one `desktop-world` MCP connection and a Skill provider named
`desktop-world`, scanning only the bundled `skills/` directory. It does not
install a second DSH runtime or rescan all default Skill roots. Its child plugins
are disposed with the parent by Cordis.

### Other local-package routes

For a host version that supports local package installation, the plugin manager
can install the complete imported package using its returned absolute directory and
enable `@caelis-labs/desktop-world`. The package name is not a claim of npm
publication. A direct file URL also works as above (`file:///absolute/path/...`
on macOS), with spaces and non-ASCII characters escaped. The adapter derives
binary and Skill paths from its own location; it takes no path configuration.

After host reload/restart, run [the live checks](#verify-the-running-installation).
Do not toggle plugins as part of an ordinary install. If the host lacks these
loader/provider interfaces, retain the manual route below and report the exact
DSH version; no DSH core change is required by DTW.

The Windows file-URL route supports package paths containing spaces and
non-ASCII characters. Do not infer a Desktop reload capability from a different
profile or platform's earlier observation.

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
        command: '/absolute/managed/package/bin/dtw'
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

Run these checks after the host has loaded the changed registration. If the old
runtime is still active, follow the single restart boundary above rather than
repeating the same checks. Keep the task pending until the host connection and
Skill match the intended package.

On macOS, these checks do not change your desktop:

```sh
/absolute/managed/package/bin/dtw --version
pgrep -fl '/absolute/managed/package/bin/dtw'
```

For a Windows process check, inspect the executable path and arguments in Task
Manager. A `dtw mcp` process shows that something launched it, not that the correct
DSH session has working tools. DSH may also start a temporary negotiation probe;
process count alone does not diagnose duplicate configuration.

In a DSH session:

1. Discover the DTW tools (normally names such as
   `mcp__desktop-world__apps`, `mcp__desktop-world__exec`).
2. Verify the actual host connection uses the intended package/version and its
   bundled Skill. Call `apps({state:"running"})`, choose one of its app IDs, and call `exec` with that `app` and
   `source: "await app.look();"`.
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
Repeat the read-only `apps`, `app.look()` and screenshot checks before resuming
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

## Resume after restart and finish cleanup

Before waiting for restart, retain a short note in this installation task:
expected version/package, host/profile pin, changed registration, and the exact
previous standalone path if one exists. Report only the pending state and one
required host action; do not print checksums, file inventories or internal pins
as a success report. Continue this task after restart; if the host cannot resume
it automatically, the same install/update prompt must resume the existing
pending pin instead of downloading or reconfiguring the package again.

Verify the intended live host connection/Skill once, then activate the pin. Only
then report the update complete. Activation/GC covers the managed store and keeps
rollback/live references; it cannot delete an older standalone directory such as
`~/Applications/DesktopWorld`. The installer retires that exact previous owned
directory separately, only after verifying its package ownership/inventory,
zero live consumers and zero remaining configuration references. Never claim
store GC will remove an external installation or a retained rollback.

For upstream details, see DSH's [MCP client](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/mcp/mcp-client).

Use a distinct [managed host/profile pin](package-management.md) (for example
`dsh-desktop` or `agy`) before configuration changes. Use the recipe's immutable
path or file URL; there is no shared mutable `current` alias. Activate the pin
only after old contexts/connections are drained and the new connection and
matching Skill have been checked. The previous package remains the rollback
reference; unrelated hosts keep their own versions. For agy copies in its fixed
plugin slot, stop the host before replacing the entire slot and let agy own its
removal. Package GC only manages the distribution store.
