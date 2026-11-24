# Install DTW in Grok Build

Start with the [common setup entry](../install.md). Use this guide only for
host-specific registration, inspection, or recovery steps. Use the receipt's
`install.package_path` for registration.

## Install the native plugin

Grok Build (**1.0.77**) installs DTW through its plugin layout
(`.claude-plugin/plugin.json`, `.mcp.json`, and bundled `skills/dtw`), exposing
plugin ID `desktop-world`, MCP server `dtw`, and Skill `dtw`:

```sh
grok plugin validate "/absolute/managed/package"
grok plugin install "/absolute/managed/package" --trust
grok plugin list --json
grok plugin details desktop-world
grok inspect --json
grok mcp doctor dtw --json
```

Use `--trust` only under user authorization; do not alter sandbox, filesystem,
or approval policies, and preserve unrelated plugins and provider settings.

Grok **copies** the plugin into `~/.grok/installed-plugins/<key>`. Discover the
actual `<key>` and copied path from `grok plugin details desktop-world` and
`grok inspect --json`, confirming `.mcp.json`'s `${CLAUDE_PLUGIN_ROOT}/bin/dtw`
and `skills/dtw` both resolve under that copied directory and match the managed
package bytes. Do not edit either the immutable managed source or Grok's
installed copy.

Plugin-provided MCP servers are not listed by `grok mcp list`; inspect with
`grok plugin details desktop-world`, `grok inspect --json`, and
`grok mcp doctor dtw --json` instead. Do not add a second manual MCP server or
standalone Skill copy.

## Load and verify

Start a new Grok session to load the installed plugin; external CLI plugin
commands do not refresh an existing session's snapshot, and shared host
processes must not be killed to force reload.

In the new session, confirm live `apps` and `exec` tools, matching plugin Skill
`dtw`, and running executable path under `~/.grok/installed-plugins/<key>`.
Complete the [first-execution check](../install.md#check-the-installation),
including read-only observation and a screenshot of an authorized test window,
and follow macOS Accessibility and Screen Recording guidance.

After those live checks pass,
[confirm the setup receipt](../install.md#use-the-setup-receipt).

## Upgrade or remove

Finish active DTW operations, replace or remove only Grok's old `desktop-world`
plugin slot, and install the new verified package from `install.package_path`
with user trust approval. Start a new session, verify the installed copy, live
tools, Skill, and GUI check, then confirm the setup receipt. Grok owns cleanup
of its `~/.grok/installed-plugins/` copies; see
[the package lifecycle](package-management.md) for managed rollback and store
cleanup.

For removal, uninstall the plugin through Grok's plugin manager. After the
connections stop,
[unbind the receipt's profile in its original store](package-management.md#remove-a-host-installation).
