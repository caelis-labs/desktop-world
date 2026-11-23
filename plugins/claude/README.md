# Desktop World

Inspect and operate running desktop applications through standard MCP tools using app-scoped JavaScript or flat native operations.

This is a local development plugin staged from a source build. It includes Agent Plugins 1.0, Claude Code, Antigravity, DSH Cordis and native Pi Package entries around one executable and the Desktop World Skill. Pi loads its declared extension and Skill; the root npm main/exports remain the Cordis entry. Grok Build loads the compatible plugin, and OpenCode uses its native MCP/Skill configuration.

Choose one route in each host. Follow [the installation guide](https://github.com/caelis-labs/desktop-world/blob/main/install.md) and [host setup routes](https://github.com/caelis-labs/desktop-world/blob/main/docs/install.md) for Caelis, DSH, agy, OpenCode, Pi and Grok Build. Packaged copies include these guides at their root and under `docs/`. Do not register both plugin and manual MCP/Skill copies. Claude Code can load this directory with `claude --plugin-dir /absolute/path/to/plugin`.

`dtw mcp` advertises `apps` and `exec`. Search with `apps({query})`, then pass the
identity to `exec({app,source})`. The bundled Skill and API describe app methods
and parameters. Use `look({})` or `capture({})` for inspection. Scripts can group
related actions and select output with `return` or `print`. Verify effects from
observations before further input.

The Harness owns tool approval. macOS 14.2+ arm64 operations need the corresponding Accessibility or Screen Recording permissions. Windows x64 uses native UIA and input delivery, with foreground access managed by DTW when needed. This development stage still needs acceptance on its exact bytes; prior release acceptance does not certify a rebuilt binary. Browser DOM and locked-session input remain outside the runtime contract. Host execution/file restrictions also apply; use a supported, authorized launch path and the host's normal approval process if access is denied. Never change sandbox policy or ACLs to make installation succeed. Switching apps retains each app’s committed state and still-valid references. References can expire or be consumed by actions; rediscover them when needed.

This local stage is not a signed or notarized release package. Build identity and retained source/license notices are included alongside the executable; public distribution requires separate release validation.

Current staging records `package_store_schema: 2` in `build-info.json`: MCP
server `dtw`, Skill
`dtw` at `skills/dtw`, and Caelis Skill identity `desktop-world:dtw`. The
plugin/install ID remains `desktop-world`. Published schema 1 packages retain
their original `desktop-world` server and `skills/desktop-world` layout for
verification and rollback; do not rewrite them or rebind existing installations
to change names.
