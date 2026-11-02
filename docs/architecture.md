# DTW architecture

DTW owns its Computer Use types, execution semantics and public API. CUA is a
source donor: complete capability groups are ported with thin adapters and pinned
provenance. The CUA SDK, registry and upstream worker are not runtime dependencies.
The public interface uses standard MCP and an optional Agent Skill; no custom Host
is required. Harnesses own approval and task policy. DTW owns target correctness,
resource coordination, native execution and honest effect reporting.

```text
Agent / Harness policy
  → standard MCP: apps / exec (required app authorization identity)
  → app facade: scene deltas, semantic/pixel targeting, script VM and cancellation
  → Computer Core: typed operations, references, routing and effects
  → native providers: application workers and short-lived scene workers
```

## Identity, references and state

`exec({app, source})` always binds one logical authorization target. `apps`
searches installed/running applications; Skill API.md documents the seven public
methods. There is no public API/result tool, app-less lane or Session object.
macOS resolves stopped bundle IDs and explicit .app paths. A look starts a stopped
app; input readiness and native focus are internal. Process identities and all
window/observation/cursor handles remain private.

A window reference binds its application instance and native owner. Semantic and
image references additionally bind their window, provider, observation generation
and validity; images include geometry and coordinate transforms. Check only
state relevant to the action: unrelated pointer motion does not invalidate a
keyboard operation. A changed target produces a refreshable diagnostic before
input where that change can be detected. Live UI trees are not atomic snapshots.

Each MCP connection owns app contexts, native references, scene baselines and a
bounded execution ledger. Calls are serialized in one admission slot. Worker or
process replacement resets the app's scene baseline. Ordinary JavaScript variables
last for one execution; the Agent does not commit or restore Session state.

Outcomes separate `not_dispatched / dispatched / unknown` from concrete effect
evidence. Script completion does not establish task completion. Cancellation or
unknown effects never authorize replay. Retained identical MCP request frames
return their original execution; new request IDs are new work. Full receipts are
internal diagnostics, with no public query tool. See the [Agent reference](../skills/desktop-world/API.md)
and [working convergence plan](agent-protocol-convergence-plan.md).

## Native resources and input

Blocking native operations belong to cancellable private workers. Providers own
AX/COM objects, native handles, event sources and lifecycle evidence. Core owns
typed operations and validation, not an upstream ToolRegistry. The existing Rust
package and Swift bridges are sufficient; capability expansion does not require
a new plugin framework or crate per capability.

On macOS, an instance lease serializes writes to one process incarnation across
sessions. Directed writes to independent instances can run concurrently. Shared
physical input and ScreenCaptureKit have their own leases and cleanup boundaries.
The scheduler serializes conflicting resources, not unrelated apps by default.
Closing an owner must settle its native resources before releasing ownership;
failed cleanup must not silently reopen admission.

macOS prefers background delivery when the action supports it. Only a preflight
failure with no preparation or dispatch side effect permits foreground fallback.
Once preparation or dispatch begins, errors do not reroute or replay input.
Explicit focus and automatic fallback leave the target in front after completion.
The user or Agent may switch away or close the owned task window when finished;
cleanup releases held inputs and respects external focus changes without restoring
an earlier app. Windows selects supported client-area mouse messages and otherwise raises the
exact target for foreground input; Agents do not select routes. Background input,
a stationary user pointer and locked-session operation are distinct capabilities.

## Window and semantic paths

The public path is apps → app look → input → look. The facade selects current
app surfaces internally and compares compact window/semantic baselines. macOS
composes independently owned surface images in app-relative z-order; image hit
testing uses the same layers and alpha. Coordinate input rechecks current app
membership, order and geometry before entering the existing native action path.
A windowless macOS app can receive an application shortcut: its process birth,
foreground ownership, input policy and paired key release remain mandatory. WindowSurface identity/geometry and SemanticBinding objects
are independent; unavailable AX/UIA content must not imply that no window exists.
Pixel actions still require exact ownership and image geometry, plus visibility
conditions for their selected delivery route.

Semantic discovery is filtered by role/text and bounded by region/subtree where
supported. Native traversal retains a continuation frontier rather than rescanning
a truncated prefix. Matching count, visited nodes, time and output bytes are
separate budgets. Partial coverage with no matches does not establish absence.
References remain scoped to their window and kind; observing a different window
must not discard unrelated usable references. Only bounded semantic changes are public; unsupported semantic activation returns
an explicit no-dispatch error and enables the screenshot fallback. Full native
queries and receipts remain available to internal diagnostics and tests.

Surface inventory may include menus, sheets and positively associated helpers.
Logical app identity and actual surface owner remain distinct. Cross-process
association needs native proof, not title/geometry guesses. ScreenCaptureKit
window capture does not establish AX operability, lock-screen support or a browser
tab-sharing implementation. A future exact-tab provider must migrate tab/frame
lifecycle, navigation invalidation, DOM references and CSS/image transforms
together; native-window pixels are not tab viewport CSS coordinates.

## Script and transport boundaries

QuickJS executes source with DTW's app facade; it does not expose arbitrary files,
network, process creation or native-tool dispatch. JSON source is decoded once,
without shell expansion or guessed unescaping. A future local file convenience
can read and serialize source on the client; remote execution must not depend on
shared local paths. Execution IDs are internally assigned, not model inputs.

The official Rust MCP SDK supplies framing, negotiation and stdio transport.
Transport feeds thin handlers and the existing execution lifecycle. HTTP/remote
transport is an extension point, not current support. Remote deployment identity
would belong to transport/deployment, while action approval remains with Harnesses.
The private atomic method schemas still support internal typing/tests. The public
facade and installed Skill API.md define the Agent contract; neither exposes raw
window or observation arguments.

## Extending capabilities

Migrate a complete source dependency closure with pinned commit/path/hash,
license, mechanical adaptations and deliberate behavior differences. Keep source
verification lightweight; it does not require compiling the full CUA repository.
[Native provenance](native-source-provenance.md),
[cross-platform provenance](native-source-provenance-cross-platform.md),
[background input](background-input-port.md) and the cursor port documents record
those boundaries. Source hashes prove attribution, not adapted behavior.

Each group must identify its state owner, dispatch boundary, failure evidence and
cleanup on success/cancel/timeout/parent exit. Input groups include held keys,
buttons and scroll state; browser groups include sessions/tabs/refs; Windows groups
include COM apartments, DPI and delayed input. App launch/quit/closeWindow use distinct typed lifecycle admission. macOS launch serializes a canonical installed path before one NSWorkspace request; quit binds one exact process incarnation and closeWindow one proven native close control. Confirmed facts are process-running, process-exited or exact-window-closed; pending save dialogs remain unverified. No force kill or automatic replay is exposed. Private SPI stays
isolated and cannot be replaced by approximate ownership guesses.

Prove one complete workflow through schema, script, native operation and independent
readback before expanding a group in bulk. Keep rejection, unknown-effect and
cleanup controls alongside successful tasks. [Building and acceptance](building.md)
separates source tests, native fixtures, external Agents and installed artifacts.
The [product matrix](product-matrix.md) records current support and limits.
