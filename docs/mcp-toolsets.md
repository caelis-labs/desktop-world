# MCP toolsets

DTW offers two startup surfaces over the same native app operations. The default
remains unchanged. Choose the surface in the MCP server launch configuration,
restart/reconnect the server, and discover/list its actual tools before use.

| Startup | Listed tools | Execution |
| --- | --- | --- |
| `dtw mcp` | `apps`, `exec` | An async JavaScript body scoped to one app |
| `dtw mcp --atomic` | `apps`, `look`, `capture`, `readText`, `wait`, `waitForNewWindow`, `click`, `setValue`, `move`, `drag`, `typeText`, `keypress`, `scroll`, `focus`, `quit` | One native app operation per call; no JavaScript |

`--atomic` is a boolean startup flag. There is no combined surface. If `exec` is
listed, the Skill uses scripts; otherwise it invokes the listed app operation
directly. The OS does not select an operation vocabulary. Host approval policy
still applies to the discovered app identity.

## Launch configuration

The standard [MCP configuration example](examples/mcp.json) launches the default
surface. For atomic tools, change that server's `args` to `["mcp", "--atomic"]`:

```json
{
  "mcpServers": {
    "desktop-world": {
      "command": "/absolute/path/to/dtw",
      "args": ["mcp", "--atomic"]
    }
  }
}
```

Use the executable from the installed complete package, retain its bundled Skill,
and refresh the MCP connection after changing arguments. Confirm the listed tools
match the table. No alternative package, local probe output or development path
is required. See [installation](install.md) for host setup.

## One method contract

[API.md](../skills/desktop-world/API.md) owns the exact method arguments, bounds,
outputs and provider limits. Both surfaces use its camelCase method names and
canonical object fields. Atomic tools add a required `app` identity returned by
`apps`; scripts bind that identity once through `exec`.

Default tool call:

```json
{
  "name": "exec",
  "arguments": {
    "app": "<discovered identity>",
    "source": "await app.click({selector:{role:'button',text:'Continue'}}); await app.typeText({text:'Hello'});"
  }
}
```

Atomic calls, sent sequentially after inspecting any error or relevant change:

```json
{"name":"click","arguments":{"app":"<discovered identity>","selector":{"role":"button","text":"Continue"}}}
```

```json
{"name":"typeText","arguments":{"app":"<discovered identity>","text":"Hello"}}
```

The same rule covers reads and waits: `app.readText({target:'w1.e12'})` corresponds
to `readText({app,target:'w1.e12'})`; `app.wait({ms:200})` corresponds to
`wait({app,ms:200})`. JS no-argument methods accept `{}`; their atomic tools take
`{app}`. Existing JS string, array and numeric shorthand remains a compatibility
convenience, rather than a second argument contract.

## State and execution boundaries

Both surfaces use the native app facade and retain app context, observations,
coordinate bindings, native object handles and text revisions within the MCP
connection. No public Session or handle management is needed. An atomic call is
a direct native operation, not a generated JavaScript program. Target identity,
stale-scene checks, bounded execution, input cleanup, cancellation and retained
request retransmission follow the same rules as `exec`.

Known selectors, keyboard commands, text insertion and default scrolling can
start directly. Do not prepend a redundant `look` to each default script. Batch
known sequential actions in one script and observe when a decision or verification
requires it. Atomic mode requires separate calls for those operations.

Each call settles its own execution scope. Explicit focus and related input can
share one `exec`; separate atomic calls cannot share that scope. On Windows,
foreground restoration at settlement can dismiss a native menu, so opening the
menu and choosing its item must stay in one script. Retained app context does
not imply retained foreground or an open menu across atomic calls.

## Results and uncertainty

A method return and the outer MCP response serve different purposes. JS scene
reads return full snapshots, `readText` returns a page, and input methods return
action receipts. The outer response reports execution status, accumulated
`effects`, automatic observation status, a compact `view` and any PNG attachment.
Atomic scene operations publish `view`; atomic `readText` publishes its page as
`result`. Atomic input reports its effects and automatic observation through the
outer response, without a custom JS return value.

A script can return a small custom JSON value to select `result`, or omit its
return/return its final snapshot to publish `view`. This selection does not change
what the individual methods return or establish that an input had its intended
effect. Dispatch, acknowledgements and observed business success remain distinct.
Check errors and read back the relevant UI before declaring success.

An interrupted or missing response can leave input effects. Inspect before new
input. Identical MCP request-ID retransmission refers to retained execution; a
new request ID executes new work. Neither surface replays input to recover an
observation or renew a stale target. Standard MCP cancellation stops its matching
call, with partial effects and cleanup failures reported explicitly.

## Compatibility and validation

Existing `dtw mcp` registrations continue to list `apps` and `exec`. The PoC
`--toolset` selector and snake_case public operation names were retired before
formal release; update experimental configurations to `--atomic` and the canonical
camelCase names. There is no mixed-mode compatibility path.

Protocol/schema checks can establish tool discovery, argument parity, shared
state, cancellation and result behavior without establishing native desktop
support. Native Windows and macOS were not tested in this formalization work;
prior platform acceptance applies only to its recorded scope. Linux remains
experimental, without AT-SPI semantics. Evidence from an isolated X11 test server
is limited to that environment and does not establish locked-desktop support.
Supported desktop use requires an unlocked desktop. See the
[product matrix](product-matrix.md) for platform capability boundaries and
[building guide](building.md) for verification workflows.
