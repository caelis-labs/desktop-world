# MCP source and installed payload

The canonical MCP server, worker and build sources are in
[`clients/mcp`](../../../clients/mcp/). The release build places their bundled
output here. A source checkout does not contain the native helper or private
Node runtime: install the matching official Full or Lite release archive for
this `plugin.json` version before running the MCP server. The repository files
are for inspection and marketplace metadata, not an implicit build step.
