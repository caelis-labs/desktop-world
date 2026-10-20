# DTW runtime and desktop extensions

DTW is a Rust Agent Plugin: MCP + Skills, linked to the official CUA SDK and one
supervised private worker. CUA owns QuickJS `run_script`, accessibility, ordinary
screenshots and general desktop actions. DTW does not copy the JS engine, modify
CUA, or require Node. The SDK and worker are locked to the same released component
revision (currently the 0.37.0 stable component release).
DTW checks click options and translates its documented `modifiers` alias to
CUA's `modifier` array; the native prelude and driver stay unmodified. Per-method
options, regression evidence and upstream limitations are in [the click contract](click-contract.md).

## Runtime-owned execution context

The public surface remains one `exec` tool with `run`, `app`, `click`, small native
pass-through operations, and `status/result/cancel`. There is no public session
parameter. Legacy named-session requests fail explicitly; their state is never
silently merged. Each stdio MCP process owns one internal trusted CUA session,
created lazily with an unpredictable host-generated label, plus JSON script state
and receipts. One admitted execution runs at a time; control requests remain
available. Successful scripts commit bounded JSON state; failures do not.

At the next execution after 25 idle minutes or 25 minutes of context age, DTW
clears state and observation bindings and returns `runtime_context_reset` without
dispatching the requested action. The following observation uses a new CUA context.
These thresholds are shorter than the configured upstream session TTLs. Worker
failure is surfaced on the original receipt; unknown actions are never replayed.
Process restart loses receipts and native tokens. Independent tasks require
independent MCP processes or a host-provided authenticated context boundary; DTW
cannot infer chat IDs from a shared connection. Multiple processes still share
one physical desktop, so their input must be coordinated by the host.

## Application observation extension

`app_view.rs` contains the observation/point contract; `app_view/Scene.swift` is a
small statically linked public macOS API bridge. Each native operation runs in a
bounded child of the same signed DTW executable, without starting another CUA
worker. The MCP parent owns a per-user file lease until the child exits. This
releases its SCK client before another Runtime uses the bridge; multiple live
SCScreenshotManager clients of one executable reproduced missing image callbacks
on the test machine. The lease waits at most three seconds; child execution is
bounded to 25 seconds. Receipts and script state stay private to each MCP process.
There is no shared daemon or separate helper installation. These modules are compiled
extensions, not a dynamic plugin ABI. There are no app-specific WPS/Obsidian rules
or private AX symbols. WindowServer supplies visible stacking; same-PID windows
are associated directly. Remote surfaces additionally require public AX client
attribution, matching window geometry and visible-point checks. The inventory
keeps all attributable visible candidates; default output retains the upper two
images (`max_windows` can expand to eight). Potential unrelated occluders are
reported as metadata, without their titles or screenshots.

Ordinary windows first use CUA's original screenshot contract with a three-second
read timeout. A timed-out read may finish upstream; it never triggers input replay. Raised and service
windows use ScreenCaptureKit directly; failed ordinary screenshot contracts may
also use SCK. DTW captures selected individual surfaces with shadows excluded,
preserving geometry rather than building a synthetic canvas. Discovery is checked
again after capture; incomplete/changed frames return an error, never a misleading
successful partial observation. This is sequential capture, not an atomic frame.

## Screenshot-bound input

An app observation returns a runtime-private 32-hex binding, exact image sizes,
window identities, capture backend and input route. Only one observation is live;
it expires after 30 seconds, is consumed by a click attempt, and is invalidated by
new observations, scripts, other operations or context reset. Retrieving receipt
pages does not invalidate it. No caller-supplied PID or global coordinate overrides
are accepted by `operation=click`.

Before input DTW checks process identity, associated windows and geometry, then
validates the point against current visible stacking. WindowServer can include
transparent full-display surfaces; bounded SCK alpha checks examine only the
point locally and never disclose unrelated pixels. Unknown or opaque coverage
refuses input. CUA frames retain CUA capture IDs and background delivery. SCK
frames use DTW's minimal CGEvent left-click route only at an already visible,
verified point; no activation, hidden-window delivery or automatic fallback.
If a native input child times out or exits without a complete reply, its receipt
is `unknown`; no input is replayed. A delivered event is not proof of effect. Agent must reobserve; stale/failed/unknown
actions are not replayed. Races with another controller cannot be eliminated.

## Capability and integration limits

The application extension needs macOS 14+, Accessibility and Screen Recording.
Other platforms retain the CUA path and report the extension unavailable. Public
APIs cannot prove ownership for every helper app, inaccessible modal, protected
surface, minimized window or other Space. `association_complete` is always false.
Top2 is a focus policy, not a completeness claim. Reobserve or explicitly widen
scope when the expected surface is missing; never infer that the modal closed.

Distribution remains one platform executable embedding the official worker,
Skills, manifests and notices. The Swift bridge is statically linked, so it adds
no separately installed runtime. OS-extension tests and CUA upgrade tests are
separate. Upgrade official release/prerelease candidates as SDK+worker pairs,
lock the exact revision/checksum, run contract and real GUI gates, and keep the
previous tested pair available. Remove an extension only when the upstream path
passes the same cases. See [checkpoint acceptance](runtime-acceptance.md) and [RC.9 acceptance](rc9-acceptance.md).
