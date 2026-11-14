# DTW selectable MCP toolset PoC

DTW-only experiment based on `69742b189e7f86771c4909fb0a46836fdb271369`
(0.4.2), on branch `poc/mcp-atomic-toolset`. No Caelis changes or merge.

The host chooses a fixed surface when starting each MCP connection:

| Command | Advertised and accepted tools |
| --- | --- |
| `dtw mcp` or `dtw mcp --toolset script` | `apps`, `exec` (existing default) |
| `dtw mcp --toolset atomic` | `apps` and the 14 public app operations below |
| `dtw mcp --toolset all` | Both surfaces, sharing one app context and execution owner |

Atomic tools: `look`, `capture`, `read_text`, `wait`, `wait_for_new_window`,
`click`, `set_value`, `move`, `drag`, `type_text`, `keypress`, `scroll`, `focus`,
`quit`. Snake case tool names map to the existing camel case app facade. The
private atomic DSL (`windows`, `observe`, `press`, native references) stays private.
Selection is enforced at admission, including direct calls to unlisted tools.
There is no client-writable switch inside the connection.

## Upstream verification

Inspected `trycua/cua` at commit
[`c7db4c85f65b1575cb4b3fffb9b8effa04bc796f`](https://github.com/trycua/cua/commit/c7db4c85f65b1575cb4b3fffb9b8effa04bc796f).
These are distinct upstream surfaces:

- The native driver [registry](https://github.com/trycua/cua/blob/c7db4c85f65b1575cb4b3fffb9b8effa04bc796f/libs/cua-driver/rust/crates/cua-driver-core/src/tool.rs#L1591)
  registers atomic tools and QuickJS `run_script` against the same session/tool
  dispatcher. `run_script` is enabled by default where compiled; the host can
  disable it using `CUA_DRIVER_DISABLE_RUN_SCRIPT=1` or `disable_run_script:true`.
  [Configuration parsing](https://github.com/trycua/cua/blob/c7db4c85f65b1575cb4b3fffb9b8effa04bc796f/libs/cua-driver/rust/crates/cua-driver-core/src/script_tool.rs#L80)
  excludes script enable/disable options from tool-writable configuration.
- The [script schema](https://github.com/trycua/cua/blob/c7db4c85f65b1575cb4b3fffb9b8effa04bc796f/libs/cua-driver/rust/crates/cua-driver-core/src/script_tool.rs#L205)
  accepts an async function body in `script`, optional `timeout_ms` (100–120000,
  default 30000) and `max_calls` (1–500, default 100), with extra fields rejected.
  Each call creates a [fresh QuickJS runtime](https://github.com/trycua/cua/blob/c7db4c85f65b1575cb4b3fffb9b8effa04bc796f/libs/cua-driver/rust/crates/cua-driver-core/src/script_tool/engine.rs#L173).
  [Script dispatch](https://github.com/trycua/cua/blob/c7db4c85f65b1575cb4b3fffb9b8effa04bc796f/libs/cua-driver/rust/crates/cua-driver-core/src/script_tool.rs#L431)
  uses the existing registry and rejects session switching/private fields.
- `cua mcp --permissions` is a separate sandbox/computer-tools entry, not a native
  driver atomic-versus-JavaScript selector. The three DTW modes above are a design
  for this PoC; they are not an upstream API.

The relevant precedent is shared dispatch and host-controlled exposure. This PoC
imports no upstream code, policy engine, public session controls, raw key-down/up
API, or new QuickJS dependency.

## Implementation boundary

`toolset.rs` supplies closed JSON schemas and translates each atomic call into
one public facade invocation, for example:

```json
{"name":"click","arguments":{"app":"<discovered identity>","target":"w1.e1"}}
```

The generated source is equivalent to
`await app.click(JSON.parse("{\"target\":\"w1.e1\"}"));`.
The method comes from static metadata; arguments are serialized twice, so quotes,
newlines, Unicode and `__proto__` remain data. No user source is accepted by atomic
tools. A generated wrapper exceeding the existing 64 KiB UTF-8 source budget is
rejected.

`app` remains a required, explicit authorization identity on every call. App ID
and shared scene argument validation happen before provider connection; detailed
input validation, target binding and native preflight remain in the facade. MCP
approval remains the host's responsibility. Listing/discovery does not grant app
access. All atomic operations conservatively advertise `readOnlyHint:false`,
because even a scene read may start a stopped app. These hints are not enforcement.

After translation the existing `Server::begin/run` owns admission, the 30-second
execution deadline, QuickJS, app state, automatic readback, request-specific MCP
cancellation, input cleanup, retained receipts and connection shutdown. Original
MCP tool name **and original JSON arguments** identify retained executions, so a
retransmitted request returns its receipt and a changed tool name conflicts.
Unknown input is never replayed. Output uses the same compact view/effects/error
projection and PNG content blocks as `exec`. Actions use the default view output;
`read_text` returns its text page as `result`.

One atomic call is one execution boundary. On Windows, focus return at that
boundary can dismiss a native menu; related menu/focus sequences should continue
to use `exec`. “Atomic” here names one facade operation, not a transactional UI
operation or a promise of only one native event. Preparation and readback may run.

## Reproduce

```sh
cargo test --release --manifest-path cmd/dtw/Cargo.toml --locked
cargo build --release --manifest-path cmd/dtw/Cargo.toml --locked --bin dtw
sh scripts/check-source.sh
```

Validation on this Linux machine: the release suite passed 548 library tests
and 5 CLI tests; the focused toolset suite passed 8 tests. `check-source.sh`,
formatting and `git diff --check` passed. An independent read-only review found no
material implementation issue and verified real-process MCP initialization,
2/15/16 tool listings and clean EOF shutdown for all modes without native app
calls. The native-worker runner explicitly skipped because it requires macOS.
The existing Linux dead-code warning for `encode_png_rgba` remains.

The SDK tests exercise the real MCP byte transport and QuickJS with a controlled
native backend. They cover mode enforcement, mandatory app identity, rejected
arguments, unavailable apps, shared context across `exec` and atomic calls,
automatic readback, exact string delivery to a setter, PNG blocks, duplicate IDs,
cross-tool conflicts, UTF-16 text pagination, generated-source byte bounds,
cancellation during an action, busy admission until cleanup, unknown dispatch
evidence and later recovery. These are simulated native tests,
not GUI or host authorization approval evidence.

Use a normal MCP host launch configuration, for example:

```json
{
  "mcpServers": {
    "desktop-world": {
      "command": "/absolute/path/to/dtw",
      "args": ["mcp", "--toolset", "atomic"]
    }
  }
}
```

Call `apps` with a query for the task-owned fixture, then use its exact `app`
identity. `type_text` accepts `{app,text,target?}`, `keypress` accepts
`{app,keys:["CTRL","A"]}`, and numeric wait is `{app,ms:100}`. Other tools use
the object form of the corresponding method in
[`API.md`](../../skills/desktop-world/API.md); tool schemas expose exact fields.
A host configured for `atomic` must consult its listed tools instead of assuming
`exec` exists.

For the bounded Linux experiment, while the real session remains locked:

```sh
python3 poc/mcp-toolset/linux_locked_probe.py --session "$XDG_SESSION_ID"
python3 poc/mcp-toolset/linux_locked_probe.py --session "$XDG_SESSION_ID" \
  --isolated --binary "$PWD/cmd/dtw/target/release/dtw" \
  > /tmp/dtw-linux-probe.jsonl
```

The default run checks the real lock hint and X11 server extensions only. The
isolated run creates its own Xvfb, KWin, D-Bus, configuration and uniquely
identified Tk app, then checks actual backend behavior there. It never sends
input or captures pixels on the real display. See
[`linux-evidence.md`](linux-evidence.md) for observed outcomes and limits.

## Limits

This is an opt-in PoC, with no default/plugin manifest change. No Caelis runtime
or policy integration was changed or tested. Host approval UI and real model tool
selection were not exercised. No performance or token-cost advantage is claimed;
atomic calls still create the existing bounded QuickJS execution. macOS/Windows
native behavior and menu sequences were not exercised on this Linux machine.
The provider has no lock-state guard: this adapter adds no locked-desktop support.
Native X11 evidence applies only to the explicitly recorded display and fixture.
