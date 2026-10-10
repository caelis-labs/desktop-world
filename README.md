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

## Build and install

On macOS arm64 with Go 1.26 and Xcode Command Line Tools:

```sh
./scripts/build-native-plugin.sh /absolute/output/dtw-plugin
/absolute/output/dtw-plugin/bin/dtw version
/absolute/output/dtw-plugin/bin/dtw install codex
```

`dtw install AGENT` supports `codex`, `claude`, `antigravity`, `gemini`, and
`cursor`. It installs the MCP server and the same Skill using each client's
current package or configuration format. Default scope is `user`; add
`--scope project --project /path/to/project` for a supported project install.
Claude Code installation calls its `claude plugin install` CLI. Gemini CLI
extensions currently support user scope in DTW. `dtw install plugin --to DIR`
copies the portable Agent Plugin 1.0 package for any compatible client.
Run `dtw install` for the complete usage line. Restart or reload the target
Agent after installation.

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
