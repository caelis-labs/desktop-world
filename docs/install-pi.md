# Install DTW in Pi

Start with the [common setup entry](../install.md). Use this guide only for its host-specific registration or recovery step. For a prepared package, use the returned `install.package_path`; do not import or bind it again. Finish with `setup confirm` and the exact receipt ID after live verification. The low-level `package activate` examples below are for older managers.

For the older manual route, [download, verify and import the DTW package](../install.md#select-and-extract-a-package),
subject to the Pi package feature gate below. Use import's immutable
`package_path` for the package command, not the extraction directory. Bind a
distinct host/profile pin (for example `pi`) before changing registration;
[activate it](package-management.md) only after checking the new live MCP
connection, matching bundled Skill and GUI observation. All
`/absolute/managed/package` paths below are placeholders for that returned path.

## Package feature gate

Prefer Pi's native Package installation so that one package supplies both its
MCP extension and Skill. The tested host is **Pi 1.5.2**. A compatible complete
DTW package must include these declarations in its `package.json`:

```json
{
  "keywords": ["pi-package"],
  "pi": {
    "extensions": ["./pi/index.mjs"],
    "skills": ["./skills/dtw"]
  }
}
```

Its `pi/index.mjs` must default-export an extension that uses Pi's built-in
`registerMcpServer` to register `dtw`, resolving `bin/dtw` from that same package
with argument `mcp`. The tested adapter uses no third-party dependencies; Pi
owns the MCP server lifecycle. This fragment describes packaged metadata for
inspection, not changes to make after download.

**Publication boundary:** the public v0.5.0 release lacks this Pi metadata and
entry point. Native installation was verified with a newly built development
candidate containing the adapter and declarations above, alongside the official
v0.5.0 runtime and Skill bytes. It is not evidence that the published v0.5.0
package already supports native Pi installation. A guide merged before release
does not add those files to an older artifact. Select a complete package whose
published inventory includes the adapter, or report the unsupported package
boundary and leave activation pending. Do not edit a release or immutable
managed package to add metadata. Loading only the Skill is not a successful DTW
installation.

## Install and load the native Package

With a package that passes the feature gate:

```sh
pi install "/absolute/managed/package"
pi list
```

Pi's package manager records the package in its settings. A user-scope
`~/.pi/agent/settings.json` entry may be stored as a relative path: resolve it
against the directory containing that settings file when checking that it
points to the imported immutable payload. Preserve existing packages and all
model, provider and authentication settings. Do not add a parallel manual MCP
entry, copy/link the Skill into a discovery directory, or install an MCP bridge.
Separate MCP/Skill configuration is unnecessary for the native Package route.

Use `/reload` in Pi to load extensions and Skills, then verify in a fresh
session when the package requires a fresh host context. Inspect session MCP
state through `/mcp` and check that the extension-provided server exposes live
`apps` and `exec` tools. Confirm that the loaded `dtw` Skill and the running
executable both resolve to the same managed package.

`pi mcp list` does not load extensions. An empty result from that command does
not disprove the Package integration, and a static MCP listing cannot prove
that the extension loaded. Likewise, `pi list` proves package registration,
not a live MCP connection or GUI readiness.

Complete the [first-execution check](../install.md#check-the-installation),
including read-only observation and a screenshot of an authorized test window.
Follow the common macOS Accessibility and Screen Recording permission guidance.
Only after live tools, the matching Skill and GUI observation pass:

```sh
"/absolute/managed/package/bin/dtw" package activate pi
```

**Evidence scope:** Pi 1.5.2 installed the development candidate as a native
Package. A fresh native Pi session loaded its extension and Skill and completed
observation, screenshots, Unicode typing and native button operations through
live DTW tools. The supervisor independently verified the committed note and
counter effect; see the [acceptance record](https://github.com/caelis-labs/desktop-world/blob/main/docs/agent-install-acceptance-2026-11-17.md).
The public v0.5.0 package still lacks this adapter, and this macOS evidence does
not establish Windows or other Pi-version compatibility.

## Upgrade or remove

Finish active DTW operations and reuse this host/profile pin. Check the new
package's Pi feature gate before replacing the old DTW package entry through
Pi's package manager. Retain unrelated packages. Load the replacement with the
supported reload boundary, verify its actual live executable, bundled Skill
and GUI, then activate. Do not leave both DTW package versions registered or
modify either immutable payload.

For removal, use Pi's package manager to remove only the DTW package reference,
reload to stop its extension-owned connection, and then run
`dtw package unbind pi` with the managed executable. Follow the
[package lifecycle](package-management.md) for rollback and owned store cleanup;
other hosts' pins and live consumers remain protected.
