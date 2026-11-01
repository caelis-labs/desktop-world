# Install DTW in Caelis

First [download and verify the DTW package](../install.md#select-and-extract-a-package).

The existing package contains a compatible plugin manifest and shares one runtime
between its MCP and Skill entries. Caelis supports this layout through its normal
[plugin manager](https://github.com/caelis-labs/caelis/blob/main/docs/plugins.md):

```sh
caelis plugin install /absolute/path/to/extracted-package --json
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

Or enter `/plugin install /absolute/path/to/extracted-package` in the Caelis TUI.
Start a new Session to load the MCP/Skill snapshot. Registration alone does not
prove tools are ready; complete the [first-execution check](../install.md#check-the-installation). Do not separately
register a second copy of the MCP server.

**Caelis compatibility:** use Core main `d15874f759aae5736b5afecf1060186a23d700bf`
or a later release containing [Core #185](https://github.com/caelis-labs/caelis/pull/185)
and [#187](https://github.com/caelis-labs/caelis/pull/187). These changes support the
standard plugin identity, large results and fixed `tool_search` / `mcp_call` route.
Core requests approval on first use of each `exec.app` in a Session, then reuses
that app approval. A new app or Session needs its own approval; denial stops
before dispatch. Approval remains entirely in Core, not DTW. Older Core builds
may lack these integration fixes.

## Upgrade or remove

Finish active operations before changing the registration. Caelis binds a local
registration to its canonical directory: to change directories, run
`caelis plugin remove desktop-world --json`, then install the new verified package
and start a new Session. Preserve unrelated plugins. Remove the old package only
when its MCP connections have stopped.

For macOS permissions, follow [the common installation check](../install.md#check-the-installation).
