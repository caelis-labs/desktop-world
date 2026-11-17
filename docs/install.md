# Install Desktop World

Start with the [common installation guide](../install.md) and
[managed package lifecycle](package-management.md).

Host setup: [Caelis](install-caelis.md), [DSH](install-dsh.md), [agy](install-agy.md),
[OpenCode](install-opencode.md), [Pi](install-pi.md), [Grok Build](install-grok-build.md).
Pi requires a package with the native adapter; the public v0.5.0 package predates
that entry. See [installation acceptance](https://github.com/caelis-labs/desktop-world/blob/main/docs/agent-install-acceptance-2026-11-17.md)
for the new hosts' tested versions and macOS scope.

The [standard MCP configuration](examples/mcp.json) starts `dtw mcp`, exposing
`apps` and `exec` as before. To select flat native app-operation tools, set that
server's arguments to `["mcp", "--atomic"]`, then restart/reconnect it. Retain the
complete installed package and bundled Skill. Discover/list the actual tools:
default mode has `exec`; atomic mode has the fourteen camelCase app operations
and no JavaScript tool. See [MCP toolsets](mcp-toolsets.md) for names, shared
argument objects, execution boundaries and compatibility.
