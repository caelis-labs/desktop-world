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

The adapter constructs `Command::App { method, args, timeout_ms }` directly.
The method comes from static metadata and arguments remain JSON data throughout.
Atomic calls generate no source and do not enter QuickJS. Native argument limits
and the MCP frame bound still apply; script source limits apply only to `exec`.
The script entry constructs `Command::Script` and keeps its existing QuickJS path.

`app` remains a required, explicit authorization identity on every call. App ID
and shared scene argument validation happen before provider connection; detailed
input validation, target binding and native preflight remain in the facade. MCP
approval remains the host's responsibility. Listing/discovery does not grant app
access. All atomic operations conservatively advertise `readOnlyHint:false`,
because even a scene read may start a stopped app. These hints are not enforcement.

The existing `Server::begin/run` owns admission and app resolution for both
entrypoints. `Coordinator::execute` dispatches `App` directly to `execute_app` and
`Script` to the engine driver. Both use `app_execution.rs` for facade dispatch,
step receipts, automatic readback, native execution cleanup and view publication.
Request-specific MCP cancellation, retained receipts and connection shutdown
remain shared. Original MCP tool name **and original JSON arguments** identify
retained executions, so a retransmitted request returns its receipt and a changed
tool name conflicts. Unknown input is never replayed. Output uses the same
compact view/effects/error projection and PNG content blocks as `exec`. Actions
use the default view output; `read_text` returns its text page as `result`.

`AppRun` is per execution. App instances, the scene baseline, decoded raster and
coordinate bindings, semantic object credentials and stale-target checks belong
to the retained coordinator/runtime. Successful settlement calls
`finish_execution` without closing that owner or clearing those references.
Releasing a completed job removes its execution record, not its session state.
Direct operations do not read or commit JavaScript state. Actual cancellation,
timeout, app replacement and native cleanup failure still invalidate the affected
owner/bindings; later work must obtain fresh native evidence. Cancellation at the
final commit boundary is reported independently of a JavaScript state update.

The coordinate acceptance also exposed missing Linux window layout metadata:
without bounds, unchanged captured coordinates always failed the existing
`stale_scene` guard. Visible owned X11 windows now report their actual client
root bounds and root-frame stacking rank from bounded `QueryTree` evidence.
The geometry/ownership/occlusion checks remain in the existing input path.

One atomic call is one execution boundary. On Windows, focus return at that
boundary can dismiss a native menu; related menu/focus sequences should continue
to use `exec`. “Atomic” here names one facade operation, not a transactional UI
operation or a promise of only one native event. Preparation and readback may run.

## Reproduce

```sh
cargo test --release --manifest-path cmd/dtw/Cargo.toml --locked
cargo test --release --manifest-path cmd/dtw/Cargo.toml --locked --lib toolset
cargo build --release --manifest-path cmd/dtw/Cargo.toml --locked --bin dtw
sh scripts/check-source.sh
```

The SDK tests exercise the real MCP byte transport with a controlled native
backend. They cover mode enforcement, mandatory app identity, rejected arguments,
shared context across `exec` and atomic calls, automatic readback, exact setter
values, PNG blocks, text pagination, native text limits, completed and active
retransmission, cancellation, cleanup failure and recovery. Capture → semantic
read → pixel click survives execution cleanup. Retained semantic handles survive
unrelated reads and reject removed/replaced/renamed controls before input.

The current release suite passes 552 library tests and 5 CLI tests; the focused
toolset suite passes 12 tests. The source checks and formatting checks pass.
Test-only counters run at the exact `run_with_session` entry, keyed by MCP
session: atomic calls, cancellations and retransmissions keep the count at zero;
`exec` increments the same counter and subsequent atomic calls leave it unchanged.
This is a live engine-entry check alongside the explicit native command dispatch.
These controlled-backend regressions do not establish native semantic GUI support.
See the direct-call evidence report below for current test counts and native
results. The existing Linux dead-code warning for `encode_png_rgba` remains.

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
  --artifacts /tmp/dtw-direct-images-new \
  > /tmp/dtw-direct-probe.jsonl
```

The default run checks the real lock hint and X11 server extensions only. The
isolated run creates its own Xvfb, KWin, D-Bus, configuration and uniquely
identified Tk app, then checks actual backend behavior there. It never sends
input or captures pixels on the real display. The current probe verifies
separate-call capture → pixel click → fixture counter change, then entry click →
typing → Enter → capture with exact fixture state and retained PNGs. See
[`direct-linux-evidence.md`](direct-linux-evidence.md) for current outcomes and
limits; [`linux-evidence.md`](linux-evidence.md) records the historical wrapper
PoC and its earlier, narrower experiment.

## Limits

This is an opt-in PoC, with no default/plugin manifest change. No Caelis runtime
or policy integration was changed or tested. Host approval UI and real model tool
selection were not exercised. No performance or token-cost advantage is claimed.
macOS/Windows native behavior and menu sequences were not exercised on this Linux
machine. Linux has no AT-SPI semantics; semantic lifetime regressions use a
controlled backend and are not real semantic desktop acceptance.
The provider has no lock-state guard: this adapter adds no locked-desktop support.
Native X11 evidence applies only to the explicitly recorded display and fixture.
