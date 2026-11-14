# Install DTW in Caelis

First [download, verify and import the DTW package](../install.md#select-and-extract-a-package).
Use import's immutable `package_path` for the plugin commands below, not the
extraction directory. Bind a distinct host/profile pin (for example `caelis`)
before changing registration; [activate it](package-management.md) only after
checking the new live MCP connection and matching Skill.

The existing package contains a compatible plugin manifest and shares one runtime
between its MCP and Skill entries. Caelis supports this layout through its normal
[plugin manager](https://github.com/caelis-labs/caelis/blob/main/docs/plugins.md):

```sh
caelis plugin install /absolute/managed/package --json
caelis plugin status <installed-id> --json
```

Current packages include Agent Plugins 1.0 metadata with the stable installed
ID `desktop-world`, independent of the extracted directory name. Check them with
`caelis plugin status desktop-world --json`. An earlier Claude-format Windows
package may have registered its directory name instead; remove that exact old
ID before installing this standard format, preserving unrelated registrations.
Earlier macOS packages use the Claude-compatible format; current Core resolves
their manifest name to the same `desktop-world` ID. Check the installed status
rather than registering a second MCP/Skill copy.

For BUILD `package_store_schema: 2`, this plugin exposes MCP server `dtw` and
Skill `dtw` with full identity `desktop-world:dtw`. The installed ID above stays
`desktop-world`. Schema 1 packages retain their original server/Skill names for
verification and rollback; do not modify or automatically rebind them.

Or enter `/plugin install /absolute/managed/package` in the Caelis TUI.
Start a new Session to load the MCP/Skill snapshot. Registration alone does not
prove tools are ready; complete the [first-execution check](../install.md#check-the-installation). Do not separately
register a second copy of the MCP server.

**Caelis compatibility:** use a Core build containing [Core #185](https://github.com/caelis-labs/caelis/pull/185)
and [#187](https://github.com/caelis-labs/caelis/pull/187). These changes support the
standard plugin identity, large results and fixed `tool_search` / `mcp_call` route.
They are not sufficient for schema 2: the audited older Core pairing and
namespace logic still requires the `desktop-world` Skill. Schema 2 also requires
a Core version that supports the `dtw` MCP namespace paired with Skill
`desktop-world:dtw` through `pairedSkill`. Until Core supports that pairing,
retain the matching complete schema 1 package, including its original MCP and
Skill registration. Do not add duplicate manual registrations to bypass this
compatibility boundary.

Core requests approval on first use of each `exec.app` in a Session, then reuses
that app approval. A new app or Session needs its own approval; denial stops
before dispatch. Approval remains entirely in Core, not DTW. Older Core builds
may lack these integration fixes.

## Upgrade or remove

Finish active operations before changing the registration. Caelis binds a local
registration to its canonical directory: to change directories, run
`caelis plugin remove desktop-world --json`, then install the new verified package
and start a new Session. Preserve unrelated plugins. Verify the actual new
connection and bundled Skill, then activate the existing host/profile pin.
Managed cleanup follows [the package lifecycle](package-management.md); remove
an old standalone package only after ownership, stopped consumers and absence
of configuration references are established. For removal, remove the host
registration first, then `dtw package unbind <host-profile>`.

For macOS permissions, follow [the common installation check](../install.md#check-the-installation).
