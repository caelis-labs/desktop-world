# Install DTW in Pi

Start with the [common setup entry](../install.md). Use this guide only for
host-specific registration, reload, or recovery steps. Use the receipt's
`install.package_path` for registration.

## Native Pi package contract

The Pi (**1.5.2**) integration uses Pi's native Package mechanism so one package
supplies both the MCP extension and the `dtw` Skill. Each verified package
declares:

```json
{
  "keywords": ["pi-package"],
  "pi": {
    "extensions": ["./pi/index.mjs"],
    "skills": ["./skills/dtw"]
  }
}
```

Its `pi/index.mjs` uses Pi's built-in `registerMcpServer` to register `dtw` from
that package's `bin/dtw mcp` with no external runtime dependencies; Pi owns the
MCP server lifecycle. Do not modify an immutable managed package.

## Install and load the native Package

```sh
pi install "/absolute/managed/package"
pi list
```

Pi stores packages in `~/.pi/agent/settings.json` (often as a path relative to
that directory). Preserve all existing packages, model, provider, and
authentication settings. Do not add a separate manual MCP entry, copy/link the
Skill into another discovery directory, or install an extra MCP bridge.

Use `/reload` in Pi to load updated extensions and Skills, then start a fresh
session when needed. Inspect session state via `/mcp` and verify live `apps`
and `exec` tools, matching `dtw` Skill, and running executable path.

Note: `pi mcp list` does not load extensions; an empty listing there does not
mean the Package integration failed, and `pi list` alone only shows
registration, not a live MCP connection or GUI readiness.

Complete the [first-execution check](../install.md#check-the-installation),
including read-only observation and a screenshot of an authorized test window,
and grant macOS Accessibility and Screen Recording permissions if prompted.
Then [confirm the setup receipt](../install.md#use-the-setup-receipt).

## Upgrade or remove

Finish active DTW operations, verify the replacement package in `pi list`, remove
the previous DTW entry (`pi remove "<previous_package_path>"`) and install the
new one (`pi install "/absolute/managed/new-package"`), retaining unrelated
packages. Use `/reload`, verify live tools, matching Skill, and GUI check in a
fresh session, then confirm the setup receipt.

For removal, run `pi remove "<package_path>"` and `/reload` to stop the
extension-owned MCP process. Then
[unbind the receipt's profile in its original store](package-management.md#remove-a-host-installation).
