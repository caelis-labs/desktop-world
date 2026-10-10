# Desktop World

Desktop World gives an Agent a compact view of local desktop applications:
`App → Window → Element`. Its macOS package runs a native Go MCP server with
an embedded QuickJS interpreter and a native desktop helper. Runtime startup
does not require Node or npm.

The MCP server exposes exactly one tool, `exec`. Its scripts use `dtw`,
persistent `state`, and `print`:

```javascript
const app = await dtw.app('Obsidian');
const win = await app.window('exact window title');
print(await win.find({role:'button'}));
```

A printed address such as `W1/B2` can be used in the next call with
`dtw.at('W1/B2')`. The same `exec` tool queries `status` or full `result`, and
cancels an execution by its original ID. Normal results are concise text;
original native receipts remain available by ID. A delivered input event is
not proof of an application effect, so read back when that distinction matters.

DTW chooses a background route when the native action supports one, then a
short coordinated foreground route when required. There is no script-facing
input mode. The Agent cursor is drawn separately from the user's physical
pointer. DTW blocks actions directed to terminal applications. Operating
system desktop permissions still apply; the embedding application manages its
own user authorization.

## Local macOS package

On macOS arm64 with Go 1.26 and Xcode Command Line Tools:

```sh
./scripts/build-native-plugin.sh /absolute/output/dtw-plugin
/absolute/output/dtw-plugin/bin/dtw version
```

The output contains the `dtw` MCP binary, a private `libexec/dtw-helper`, one Skill,
`plugin.json`, and `mcp.json`. Run `bin/dtw` with no arguments as a persistent
stdio MCP server, or install the directory in a compatible Agent Plugin client.
The package does not bundle Node. `scripts/package-plugin.sh` creates a local
versioned candidate archive from a clean checkout; it does not publish it.

The new runtime has live macOS evidence for owned AppKit fixtures and selected
Obsidian, Chrome, and WPS tasks. Windows behavior for this runtime remains
unverified and is not packaged as the same candidate. The native engine is
shared internally; platform launch, input coordination, and desktop drivers
have separate Darwin and Windows implementation files.
