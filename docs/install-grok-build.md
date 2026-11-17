# Install DTW in Grok Build

First [download, verify and import the DTW package](../install.md#select-and-extract-a-package).
Use import's immutable `package_path` for the plugin commands below, not the
extraction directory. Bind a distinct host/profile pin (for example
`grok-build`) before changing registration; [activate it](package-management.md)
only after checking the new live MCP connection, matching bundled Skill and GUI
observation. All `/absolute/managed/package` paths below are placeholders for
that returned path.

## Install the native plugin

The verified **Grok Build 1.0.77** installation consumes the existing official
v0.5.0 package through its Claude/Grok-compatible plugin layout:
`.claude-plugin/plugin.json`, `.mcp.json` and bundled `skills`. The installed
plugin ID is `desktop-world`; BUILD schema 2 supplies MCP server `dtw` and Skill
`dtw`. This does not establish support for the separate Agent Plugins 1.0
manifest format that may also be present in the package.

```sh
grok plugin validate "/absolute/managed/package"
grok plugin install "/absolute/managed/package" --trust
grok plugin list --json
grok plugin details desktop-world
grok inspect --json
grok mcp doctor dtw --json
```

Use Grok's normal plugin trust mechanism when the user has authorized the DTW
installation. The tested install used `--trust` under that authorization; it
does not require changing sandbox, filesystem or approval policies. Preserve
unrelated plugins and existing model/provider settings.

Grok **copies** the plugin into `~/.grok/installed-plugins/<key>`; it does not
execute directly from the managed source path. Discover the actual key/path
from plugin details and inspection output. Verify the copied file inventory
and content hashes against the imported complete package. In the tested
installation, the copy matched the managed package without differences.
Do not edit either the immutable source or the installed copy.

The `.mcp.json` command uses `${CLAUDE_PLUGIN_ROOT}/bin/dtw`; Grok correctly
expands that variable to the installed plugin copy. `grok inspect --json`
should identify both the MCP server and Skill as coming from plugin
`desktop-world`, with their actual paths under that same copy. Check that the
copy's runtime version and bytes match the managed package pinned for this host;
report the actual copied runtime path rather than the managed source as the
live executable.

Plugin-provided MCP servers are absent from `grok mcp list`. Use plugin details,
`grok inspect --json` and `grok mcp doctor dtw --json` for this route. Do not add
a second manual MCP server or separately install the Skill because the static
MCP listing is empty.

## Load and verify

Start a new Grok session to load the installed plugin. Registration from an
external CLI does not update an existing session's loaded plugin snapshot.
Use the host's normal session lifecycle; do not kill leader/shared processes
to force a reload.

In the new session, confirm live `apps` and `exec` tools and the matching
plugin-provided `dtw` Skill. Check the actual server process path/version under
the installed copy, then complete the
[first-execution check](../install.md#check-the-installation), including
read-only observation and a screenshot of an authorized test window. On macOS,
follow the common Accessibility and Screen Recording permission guidance.
A successful doctor handshake alone does not establish the session's tools,
Skill loading or GUI readiness.

Only after those checks pass, activate the managed source package's pending
host/profile pin:

```sh
"/absolute/managed/package/bin/dtw" package activate grok-build
"/absolute/managed/package/bin/dtw" package status
```

**Evidence scope:** Grok Build 1.0.77 installed the verified official v0.5.0
macOS arm64 package through the native trusted plugin route. Validation,
plugin inventory, expanded runtime/Skill paths, copied-byte equality and MCP
doctor all passed; doctor completed a handshake and discovered two tools.
A fresh session also loaded the plugin Skill and completed screenshots, Unicode
typing and native button operations through live DTW tools. The supervisor
verified the note commit and counter effect independently. The
[acceptance record](https://github.com/caelis-labs/desktop-world/blob/main/docs/agent-install-acceptance-2026-11-17.md) retains a host file-read
recovery and an extra checkbox probe; this was a functional pass with those
onboarding notes, not a claim of an error-free run or Windows acceptance.

## Upgrade or remove

Finish this host's active DTW operations and reuse its `grok-build` pin. Import
and bind the new complete package, then use Grok's plugin manager to remove or
replace only this DTW plugin's old installed slot before installing the new
one. Preserve unrelated plugins, registrations and other hosts' pins. Do not
create another DTW slot, overlay individual files, or manually repoint only
its MCP or Skill path.

Start a new session, verify the replacement copy against its managed source,
check live tools, Skill and GUI, then activate the managed pin. Grok owns
cleanup of its installed plugin copies; managed-store activation/collection
does not delete those copies. Follow the
[package lifecycle](package-management.md) for managed rollback and cleanup.
To roll back, reinstall the complete package protected by the rollback pin and
repeat the same verification before activation.

For removal, remove this DTW plugin through Grok's plugin manager, let its
connections stop through the normal lifecycle, then run
`dtw package unbind grok-build` with the managed executable. Do not remove
shared managed packages or another plugin's copied directory.
