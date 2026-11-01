# DTW architecture

DTW owns its Computer Use types, execution semantics and public API. CUA is a
source donor: complete capability groups are ported with thin adapters and pinned
provenance. The CUA SDK, registry and upstream worker are not runtime dependencies.
The public interface uses standard MCP and an optional Agent Skill; no custom Host
is required. Harnesses own approval and task policy. DTW owns target correctness,
resource coordination, native execution and honest effect reporting.

```text
Agent / Harness policy
  → standard MCP: apps / api / exec / result
  → session: app contexts, script VM, state, receipts and cancellation
  → Computer Core: typed operations, references, routing and effects
  → native providers: application workers and short-lived scene workers
```

## Identity, references and state

`exec({app, source})` takes one stable platform app ID and one script. An Agent
session can use multiple apps. PID, process birth generation and native handles
remain internal. One app ID may resolve to multiple instances; `windows()`
aggregates them and each window reference retains its exact owner. A provider
must not silently substitute another instance or select one by title alone.

A window reference binds its application instance and native owner. Semantic and
image references additionally bind their window, provider, observation generation
and validity; images include geometry and coordinate transforms. Check only
state relevant to the action: unrelated pointer motion does not invalidate a
keyboard operation. A changed target produces a refreshable diagnostic before
input where that change can be detected. Live UI trees are not atomic snapshots.

Each MCP connection owns separate app state, references and receipts. Switching
apps preserves them. Completed receipts use sliding retention independently of
the latest app state; eviction never replays old work. Same-connection scripts
execute sequentially. Worker retirement can reset native references without
discarding committed logical state; actual instance replacement has separate
state-reset semantics. Connection shutdown ends the session; persistent receipt
recovery across server processes is not promised.

Script state commits on success. Desktop effects are not transactional. Outcomes
separate `not_dispatched / dispatched / unknown` from concrete effect evidence;
script completion does not establish business-task completion. Timeout, exception,
cancellation and unknown effects never authorize automatic replay. `result`
inspects the original receipt, while successful ordinary `exec` needs no polling.
See [protocol](app-protocol-v1.md) and [script semantics](app-script-v1.md).

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

Keep a universal path: list apps, list windows, capture a chosen window and click
its image coordinates. WindowSurface identity/geometry and SemanticBinding objects
are independent; unavailable AX/UIA content must not imply that no window exists.
Pixel actions still require exact ownership and image geometry, plus visibility
conditions for their selected delivery route.

Semantic discovery is filtered by role/text and bounded by region/subtree where
supported. Native traversal retains a continuation frontier rather than rescanning
a truncated prefix. Matching count, visited nodes, time and output bytes are
separate budgets. Partial coverage with no matches does not establish absence.
References remain scoped to their window and kind; observing a different window
must not discard unrelated usable references. Detail and full receipts are read
on demand rather than expanding every response into a complete UI tree.

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
Method schemas generate API discovery and the checked-in TypeScript contract.

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
include COM apartments, DPI and delayed input. App launch/quit needs lifecycle
semantics beyond operations on an already-running instance. Private SPI stays
isolated and cannot be replaced by approximate ownership guesses.

Prove one complete workflow through schema, script, native operation and independent
readback before expanding a group in bulk. Keep rejection, unknown-effect and
cleanup controls alongside successful tasks. [Building and acceptance](building.md)
separates source tests, native fixtures, external Agents and installed artifacts.
The [product matrix](product-matrix.md) records current support and limits.
