# Install DTW in agy

First [download and verify the DTW package](../install.md#select-and-extract-a-package).

```sh
agy mcp add desktop-world /absolute/path/to/extracted-package/bin/dtw mcp
```

On Windows, pass the absolute `bin/dtw.exe` path. Copy the complete bundled
`skills/desktop-world` to the workspace's `.agents/skills/desktop-world`, then
start a session in that workspace. agy has its own ordinary MCP and Skill extension
points; do not import the Claude manifest expecting `${CLAUDE_PLUGIN_ROOT}` to
be expanded by agy.

Complete [the first-execution check](../install.md#check-the-installation) in that
session. A registered MCP server is not proof that native permissions are ready.
Use agy's MCP management to update or remove the existing `desktop-world`
registration; do not add a duplicate server. Keep the executable and Skill from
the same package version.
