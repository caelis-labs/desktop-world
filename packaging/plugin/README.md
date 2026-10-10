# Desktop World Agent Plugin

This directory supplies the native macOS Agent Plugin manifests and Skill.
The local package builder copies them beside `bin/dtw` and its private
`libexec/dtw-helper`. `mcp.json` starts `bin/dtw` directly. The server exposes one
MCP tool, `exec`, and uses embedded JavaScript without Node or npm.

Install the complete package directory in an Agent Plugin client, or register
`bin/dtw` as a persistent stdio MCP server and install the packaged Skill
separately. Each MCP connection has its own JavaScript state and receipt
history. Keep a connection open while using its object addresses.

The package applies native OS permission checks and refuses actions targeting
terminal applications. The embedding application is responsible for its own
user authorization. The macOS arm64 package is locally buildable; Windows
validation for this runtime is pending.
