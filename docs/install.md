# Connect Desktop World to an Agent

The current local product is the rewrite-only `dtw` executable, its standard stdio MCP interface, and an optional [Agent Skill](../skills/desktop-world/SKILL.md). No custom Host adapter, grant file or DTW approval UI is required. The Agent/Harness applies its own tool policy; each `exec` exposes the stable app ID as ordinary tool input.

## Fixed candidates

A rewrite candidate is a ZIP produced by the [candidate builder](releasing.md).
It contains one executable, Skill, standard configuration examples, dependency
notices and checksums. Extract the complete package, verify its outer `.sha256`
and internal `SHA256SUMS`, then register the absolute `bin/dtw[.exe]` path below.
It requires no Rust, Xcode, CUA or Node installation to run DTW. The same directory
also supports the optional Claude plugin route without a second binary.

Candidate construction, signature verification, notarization and actual installed
GUI acceptance are separate results; consult its `BUILD.json`, `CANDIDATE.json` and
[current acceptance record](rewrite-candidate-acceptance.md). No new public release
is implied by the 0.2.0 source version. Windows still requires independent acceptance.

## Build and register MCP

Build from this checkout using [the build instructions](building.md). Use an absolute path to the resulting executable. For clients that accept the usual `mcpServers` JSON configuration, copy and customize [mcp.json](examples/mcp.json):

```json
{
  "mcpServers": {
    "desktop-world": {
      "command": "/absolute/path/to/dtw",
      "args": ["mcp"]
    }
  }
}
```

There is no universal Agent plugin manifest. Register the stdio server using the target client's documented configuration. Do not run the removed `dtw install` command.

For Codex, merge [the TOML example](examples/codex.toml) into its normal MCP configuration, or run:

```sh
codex mcp add desktop-world -- /absolute/path/to/dtw mcp
```

Codex documents both CLI registration and the `mcp_servers` table in its [official MCP guide](https://developers.openai.com/codex/mcp/). These are instructions for the operator; no repository helper edits your Agent configuration.

For agy, register the same executable through its standard MCP command:

```sh
agy mcp add desktop-world /absolute/path/to/dtw mcp
```

agy also discovers workspace Skills under `.agents/skills/`; copy the canonical
`skills/desktop-world` directory there. Its native plugin configuration uses
`mcp_config.json`. The Claude template below is client-specific: agy's importer
does not translate `${CLAUDE_PLUGIN_ROOT}` into an executable path. Use the
ordinary MCP registration above when loading the Skill separately. agy applies
its normal tool policy; DTW requires no special approval integration.

## Add the Skill

Copy the `skills/desktop-world` directory to the chosen Agent's documented Skill discovery directory, or include it through that Agent's plugin packaging. The self-contained `SKILL.md` follows the [Agent Skills specification](https://agentskills.io/specification). It contains the observation/action/readback workflow and interruption rules. MCP itself advertises the method index through `apps` and detailed schemas through `api`, so the Skill is useful guidance rather than a hidden API dependency.

## Local Claude Code plugin

The repository has a [Claude Code plugin template](../plugins/claude/.claude-plugin/plugin.json) using that client's official layout. Stage an already-built local binary into a new directory:

```sh
python3 scripts/stage-agent-plugin.py \
  --binary "$PWD/cmd/dtw/target/debug/dtw" \
  --output /tmp/desktop-world-claude-plugin
claude plugin validate /tmp/desktop-world-claude-plugin
claude --plugin-dir /tmp/desktop-world-claude-plugin
```

The output contains `.claude-plugin/plugin.json`, `.mcp.json`, `bin/dtw`, the canonical Skill and notices. `.mcp.json` resolves the binary with `${CLAUDE_PLUGIN_ROOT}`. The helper refuses an existing output path and records the copied binary's SHA-256. It neither installs the plugin nor publishes, signs or builds a release. See Claude Code's [plugin reference](https://code.claude.com/docs/en/plugins-reference) and [local plugin testing](https://code.claude.com/docs/en/plugins#test-your-plugins-locally).

Do not install both a manually configured server and the plugin's copy into the same Agent session unless two separate server instances are intended.

## First execution

Start the configured MCP connection, call `apps`, choose a returned app, then call `exec` with only its returned `app` ID string and a small `source` script such as `return await app.windows();`. `exec` returns when the script settles. `result` reads an original receipt when needed; it is not a normal polling step. Detailed tool arguments and cancellation are in the [MCP contract](app-protocol-v1.md).

Native execution currently targets macOS. Accessibility and Screen Recording access are OS prerequisites for the relevant operations. Grant those OS permissions to the actual process identity used in your environment when macOS requires them; changing an unsigned build's path or bytes may require rechecking that identity. No capability approval token is supplied to DTW.

Windows has a native provider in this source tree; linked product builds, independent Windows GUI acceptance and signed/notarized public packages remain separate work. Linux X11 is experimental and has no desktop support commitment. A successful local stage or manifest validation proves package structure, not native permissions, GUI behavior or a signed release. Old public `0.1.3`/candidate artifacts use the retired CUA runtime and must not be substituted for this build.
