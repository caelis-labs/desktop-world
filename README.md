# Desktop World

DTW gives an Agent a compact desktop model: App → Window → Element. The native
Go MCP server exposes one tool, `exec`; scripts use `dtw`, persistent `state`,
and `print`. The process needs no Node or npm. Input routes to the background
when possible and borrows the foreground when necessary. Terminal application
actions are blocked. Operating system desktop permissions still apply; an
embedding application owns its user authorization.

```javascript
const app = await dtw.app('Obsidian');
print(await app.windows());
```

An address printed by one script, such as `W1/B2`, works in the same MCP
connection with `dtw.at('W1/B2')` until that UI object expires. A script's
`execution_id` appears in the result. The same `exec` tool accepts `status`,
`result`, and `cancel` for that ID. Query the original result after an unknown
outcome; do not replay the action. The companion Skill teaches discovery,
actions, screenshots, and receipt handling.

## Quick install

From the repository root on macOS arm64 (Go 1.26 and Xcode Command Line
Tools required), build the native package once:

```sh
./scripts/build-native-plugin.sh "$PWD/artifacts/dtw-plugin"
DTW="$PWD/artifacts/dtw-plugin/bin/dtw"
"$DTW" version
```

**CLI install:** choose your Agent. Each command installs the native MCP
server and the same companion Skill. The default scope is your user account.

| Agent | Command |
| --- | --- |
| Codex | `"$DTW" install codex` |
| Claude Code | `"$DTW" install claude` |
| Antigravity | `"$DTW" install antigravity` |
| Gemini CLI | `"$DTW" install gemini` |
| Cursor | `"$DTW" install cursor` |

For a supported project install, use
`"$DTW" install codex --scope project --project /path/to/project` (replace
`codex` with the target Agent). Gemini
CLI extensions currently support user scope only. Claude Code requires its
`claude` CLI; DTW registers a local marketplace and installs the plugin through
that CLI. Running `dtw` without arguments starts its stdio MCP server for
hosts configured manually.

**Plugin package:** copy an [Agent Plugin 1.0](https://agent-plugins.org/)
package to a chosen directory, then load that directory in a compatible
Agent's plugin manager:

```sh
"$DTW" install plugin --to "$PWD/artifacts/dtw-agent-plugin"
```

Run `"$DTW" install` for all options. Restart or reload the Agent after
installation. The build destination must be empty; use a fresh directory for
a new build.

The package contains `plugin.json`, `mcp.json`, the Claude Code plugin
adapter, `bin/dtw`, a private `libexec/dtw-helper`, and
`skills/desktop-world/SKILL.md`. Cursor can consume the portable Agent Plugin
format; its direct installer also writes Cursor's MCP and Skill locations.
Antigravity uses its own closed plugin manifest, so its installer creates a
separate adapter package. Gemini CLI receives its extension manifest. None of
these adapters changes the DTW tool or JavaScript API.

The macOS runtime has live tests for owned AppKit fixtures and selected
Obsidian, Chrome, and WPS interactions. Windows has a separate driver entry
and cross-platform build checks, but this runtime has not passed real Windows
desktop validation.

## Source layout

```text
cmd/dtw/                    MCP server, JavaScript runtime, installer
cmd/dtw-helper/             private native helper
internal/world/             desktop object and receipt types
internal/engine/            observation and action execution
internal/backend/           shared driver contract
internal/platform/          platform opener and Darwin/Windows drivers
internal/ipc/               private host/helper protocol
internal/auditlog/          internal logging
internal/testutil/          deterministic desktop fixture
tests/contract/             behavioral contract tests
testdata/                   protocol and AppKit fixtures
packaging/plugin/           portable manifests and one Agent Skill
scripts/                    build, checks, fixture builders
```
