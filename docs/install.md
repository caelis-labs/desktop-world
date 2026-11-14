# Install Desktop World

Start with the [common installation guide](../install.md) and
[managed package lifecycle](package-management.md).

Host setup: [Caelis](install-caelis.md), [DSH](install-dsh.md), [agy](install-agy.md).

The [standard MCP configuration](examples/mcp.json) starts `dtw mcp`, exposing
`apps` and `exec` as before. To select flat native app-operation tools, set that
server's arguments to `["mcp", "--atomic"]`, then restart/reconnect it. Retain the
complete installed package and bundled Skill. Discover/list the actual tools:
default mode has `exec`; atomic mode has the fourteen camelCase app operations
and no JavaScript tool. See [MCP toolsets](mcp-toolsets.md) for names, shared
argument objects, execution boundaries and compatibility.
