# Desktop World

Desktop World (DTW) lets an Agent inspect and operate desktop applications through standard MCP tools and an app-scoped JavaScript API. Every execution names a stable platform app ID, such as `com.apple.TextEdit`. DTW verifies the running instance, window and observation references, executes the operation, and reports its outcome. Tool approval and task policy belong to the Agent/Harness.

The source tree contains the rewrite only, with no CUA SDK, upstream worker or old script facade. Native execution currently supports the macOS subset in the [product matrix](docs/product-matrix.md). Existing [release assets](https://github.com/caelis-labs/desktop-world/releases) are historical products; they do not install this implementation.

## Connect an Agent

Build with Rust 1.98.0 and, on macOS, Xcode/Swift:

```sh
cargo +1.98.0 build --manifest-path cmd/dtw/Cargo.toml --locked --bin dtw
cmd/dtw/target/debug/dtw --help
```

Configure the Agent's ordinary stdio MCP connection to launch `/absolute/path/to/dtw mcp`. For clients using `mcpServers` JSON:

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

If `CARGO_TARGET_DIR` is set, use its executable instead. Add the repository's [desktop-world Skill](skills/desktop-world/SKILL.md) through the Agent's normal Skills extension point. [Installation instructions](docs/install.md) include Codex configuration and a locally staged Claude plugin. No grant file, approval callback or DTW-specific Host integration is needed.

## Use the tools

| Tool | Purpose |
| --- | --- |
| `apps` | Discover running applications, their stable app IDs, the script API and current support conditions |
| `exec` | Execute a bounded script against the required `app` ID and return its result and receipts |
| `result` | Read the app’s latest accepted execution; optionally select a historical `execution_id`; never execute it again |

Select an entry from `apps` and pass its `app` string unchanged, for example `exec({app: "com.apple.TextEdit", source: "return await app.windows();"})`. The Harness can inspect that app ID through ordinary tool arguments. DTW resolves and verifies the running instance internally; process IDs and generations are not Agent parameters. Multiple independent instances of the same app ID produce `ambiguous_app`.

The script API provides `app.windows()`, `app.observe()`, `app.capture()`, `app.press()` and `app.click()`. `apps` publishes their descriptions and limits; [TypeScript declarations](docs/app-script-api.d.ts) and the [script contract](docs/app-script-v1.md) describe the same API. DTW generates execution IDs internally; the Agent supplies only an app and its script. A script stays within its supplied app; the conversation can retain and alternate between multiple app contexts. Switching apps preserves committed JSON state and native refs. Scripts default to a caller-adjustable 30-second deadline, with no call-count quota unless one is supplied. A dispatched action is not proof of its intended effect: read the application back independently.

`exec` waits for its bounded execution; normal use needs no job-polling loop. Standard MCP cancellation addresses the active request. Closing the connection closes its native resources. Unknown outcomes remain unknown and are never automatically replayed. See the [MCP contract](docs/app-protocol-v1.md).

## Development and evidence

The [architecture baseline](docs/next-architecture-baseline.md) owns DTW's identity, execution and native-provider contracts. [Build instructions](docs/building.md) separate fast checks, local product builds and real GUI acceptance. The foundation separates session/app state from native resource ownership. Basic window surfaces, bounded semantic discovery, and background/pointer/locked-session modes have explicit [evolution contracts](docs/next-architecture-baseline.md); their unimplemented portions are not advertised as supported. C6 extends complete capability groups on this foundation; Windows, text/keyboard operations and Browser/DOM control are not yet implemented.

The [multi-app foundation report](docs/multi-app-foundation-acceptance.md) records retained state/refs, caller-selected budgets, native resource cleanup and two independent MCP sessions using real test windows. The [standard Agent acceptance report](docs/standard-agent-acceptance.md) records the #61 baseline: standard MCP, native GUI, app switching, cancellation and cleanup checks, plus successful model-driven workflows through ordinary agy and Claude Code MCP/Skills. The [C5 report](docs/c5-completion-acceptance.md) retains earlier window-boundary evidence; the [product matrix](docs/product-matrix.md) separates current and historical coverage.

Local plugin staging is available for development. Signed installation packages and public release delivery remain separate work; see [releasing](docs/releasing.md). DTW is [MIT licensed](LICENSE), with adapted-source attribution in [third-party notices](THIRD_PARTY_NOTICES.md).
