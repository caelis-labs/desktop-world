# Desktop World

Desktop World (DTW) lets an Agent inspect and operate desktop applications through standard MCP tools and an app-scoped JavaScript API. Every execution names a stable platform app ID, such as `com.apple.TextEdit`. DTW verifies the running instance, window and observation references, executes the operation, and reports its outcome. Tool approval and task policy belong to the Agent/Harness.

The source tree contains the rewrite only, with no CUA SDK, upstream worker or old script facade. The [product matrix](docs/product-matrix.md) separates locally validated macOS behavior, Windows native implementation awaiting independent desktop acceptance, and experimental Linux X11 code. Existing [release assets](https://github.com/caelis-labs/desktop-world/releases) are historical products; they do not install this implementation.

## Connect an Agent

For an unpublished fixed candidate, use the [candidate builder and install guide](docs/releasing.md); its extracted runtime needs no development toolchain. Current candidate evidence is recorded [separately](docs/rewrite-candidate-acceptance.md).

To build from source, use Rust 1.98.0 and, on macOS 14.2+, Xcode/Swift:

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

The service defaults to `auto`: macOS prefers eligible background input and can temporarily focus the target when needed. To disable DTW's foreground routes, launch `dtw mcp --input-policy background-only` (MCP config: `"args": ["mcp", "--input-policy", "background-only"]`). `apps` and `api`, including method-filtered `api`, expose `service.input_policy` as `auto` or `background_only`. This launch setting is an input-routing constraint, not an approval or Harness permission; `exec` remains exactly `{app, source}`. Strict mode rejects explicit focus and actions requiring foreground delivery with `foreground_required` before foreground preparation. Semantic actions remain available where supported; an application can still change its own UI or focus in response to them.

If `CARGO_TARGET_DIR` is set, use its executable instead. Add the repository's [desktop-world Skill](skills/desktop-world/SKILL.md) through the Agent's normal Skills extension point. [Installation instructions](docs/install.md) include Codex configuration and a locally staged Claude plugin. No grant file, approval callback or DTW-specific Host integration is needed.

## Use the tools

| Tool | Purpose |
| --- | --- |
| `apps` | Discover running applications, their stable app IDs, the script API and current support conditions |
| `api` | Read detailed schemas for selected script methods when needed |
| `exec` | Execute a bounded script against the required `app` ID and return its result and receipts |
| `result` | Read the app’s latest accepted execution; optionally select a historical `execution_id`; never execute it again |

Select an entry from `apps` and pass its `app` string unchanged, for example `exec({app: "com.apple.TextEdit", source: "return await app.windows();"})`. The Harness can inspect that app ID through ordinary tool arguments. DTW aggregates windows across all running instances of that app ID and routes each `window_ref` to its exact native instance. Process IDs and generations remain internal. A single script can operate multiple windows, including windows in different instances.

The script API provides `windows`, `observe`, `capture`, `press`, `click`, `focus`, `typeText`, `keypress`, `scroll`, `setValue` and `readText`. `apps` returns a compact index and support conditions; `api({methods:["typeText","keypress"]})` returns selected detailed schemas; [TypeScript declarations](docs/app-script-api.d.ts) and the [script contract](docs/app-script-v1.md) describe the same API. DTW generates execution IDs internally; the Agent supplies only an app and its script. A script stays within its supplied app; the conversation can retain and alternate between multiple app contexts. Switching apps preserves committed JSON state and native refs. DTW currently applies a 30-second script deadline internally, with no call-count quota; the public input remains exactly app and source. A dispatched action is not proof of its intended effect: read the application back independently. On macOS, `scroll` can use a capture point or a semantic element bound to `observation_ref`; omitting the anchor keeps the window-center behavior.

Use `supported_observe_scopes` to discover window, subtree, capture-region and menu support. `readText` returns bounded UTF-16 pages from a native text element. On macOS, proven related dialogs remain under the original app ID; refresh the window list after opening one and use its returned ref. Inventory coverage distinguishes proven associated surfaces from helpers it could not establish. The [real-task batch plan](docs/real-task-closure-plan.md) defines the WPS, Obsidian and Chrome completion checks.

Actions may report `delivery` with the selected semantic/background/foreground route, reason and temporary foreground ownership/return evidence. This does not confirm the business effect. An explicit `app.focus` receipt can remain `pending`; `script.focus_return` separately records the later script cleanup. Missing delivery metadata is not a claim that input stayed in the background. See the [receipt contract](docs/app-protocol-v1.md#结果效果与图像).

On macOS, each session has an independent Agent cursor for actual pointer input. Its window does not accept mouse input; target-window screenshots and input checks exclude only verified DTW cursor windows. It shows where input was posted, not whether the task succeeded. See the [cursor port and validation boundaries](docs/cursor-overlay-port.md).

`exec` waits for its bounded execution; normal use needs no job-polling loop. Standard MCP cancellation addresses the active request. Closing the connection closes its native resources. Unknown outcomes remain unknown and are never automatically replayed. See the [MCP contract](docs/app-protocol-v1.md).

## Development and evidence

The [architecture baseline](docs/next-architecture-baseline.md) owns DTW's identity, execution and native-provider contracts. [Build instructions](docs/building.md) separate fast checks, local product builds and real GUI acceptance. The foundation separates session/app state from native resource ownership. Basic window surfaces, bounded semantic discovery, and background/pointer/locked-session modes have explicit [evolution contracts](docs/next-architecture-baseline.md); their unimplemented portions are not advertised as supported. Common input and native Windows/X11 providers now extend this foundation. Browser DOM, app launch/quit, drag and locked-session operation remain outside this batch. Background delivery depends on the app and action; unsupported paths follow the service input policy. Cross-session physical input is serialized internally; distinct app contexts do not imply independent OS keyboards or pointers.

The [app/source acceptance report](docs/app-source-acceptance.md) verifies the current two-field entry, receipt recovery, native GUI and a real Claude Code workflow. The [multi-app foundation report](docs/multi-app-foundation-acceptance.md) records retained state/refs, caller-selected budgets, native resource cleanup and two independent MCP sessions using real test windows. The [standard Agent acceptance report](docs/standard-agent-acceptance.md) records the #61 baseline through ordinary agy and Claude Code MCP/Skills. The [C5 report](docs/c5-completion-acceptance.md) retains earlier window-boundary evidence; the [product matrix](docs/product-matrix.md) separates current and historical coverage.

Local plugin staging is available for development. Fixed candidate construction is explicit; installed GUI acceptance and public release delivery remain separate gates; see the [first rewrite release plan](docs/first-rewrite-release-plan.md) and [releasing](docs/releasing.md). DTW is [MIT licensed](LICENSE), with adapted-source attribution in [third-party notices](THIRD_PARTY_NOTICES.md).
