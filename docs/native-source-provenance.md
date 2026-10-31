# Native source provenance

## Real-application closure additions (2026-10-30)

This batch extends the existing provider; it does not add the CUA SDK, upstream
worker, or a second script runtime. The earlier exclusions below describe their
original checkpoints. The following additions supersede their blanket exclusion
of menus, sheets, and associated surfaces; real-task acceptance is recorded
separately from source and pure-control evidence.

- **Modal and menu ancestry:** the bounded AXParent walk, menu-bar ancestry and
  sheet-to-owning-window activation mapping come from CUA 0.38.0
  `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`,
  `platform-macos/src/ax/exact_target.rs:22–209`. The local background-input donor
  at `b38360443960a6540685a9a5eb5407f10fe3d03b` was consulted and that exact block
  was compared byte-for-byte with 0.38.0: 6,694 bytes, SHA256
  `76d684180b73acc56e5b1458d91a68f1a0d5d8414ba3e1f4168cae977a42d396`.
  The later commit's different background inventory code is not part of this
  ancestry port. DTW keeps each sheet's own surface identity and uses its owning
  window only for raise/activation. Exact focused-surface verification still
  requires the requested sheet's own AX object and native window ID.
- **Associated helper surfaces:** the public AX-client hit, nearest menu/window
  root and visible exact WindowServer surface mechanism is recovered from DTW
  `79a097b`, `cmd/dtw/src/app_view/Scene.swift` (`hit`/`discover`). The new adapter
  retains the logical client, actual CG owner and kernel birth, and native AX
  root. Each operation rechecks those relationships. The candidate surface must
  supply its own nontransparent pixel at the tested point. Geometry or title
  alone cannot establish association. CUA directed background input is declined
  before dispatch when the actual owner differs; the existing foreground path
  then uses the validated owner. An unprovable helper remains unsupported.
- **Discovery scopes:** menu, retained-subtree and capture-pixel-region queries
  reuse DTW's existing bounded frontier, pagination, lifetime and ownership
  checks. On macOS, a region also clips through proven AXScrollArea viewports;
  disjoint contents can be skipped without walking their full tree. Retained
  candidates carry their element and viewport geometry for later validation.
  Missing geometry remains partial coverage. A region is not OCR or a pixel
  occlusion guarantee. A derived subtree retains its original AX root identity,
  even if the same child object is reparented. Replacing a subtree query consumes
  the prior semantic query, while other windows and the capture binding retain
  their existing lifetime.
- **Long text:** `readText` is a small native `AXValue`/`AXStringForRange` adapter,
  not a claimed copy of an upstream whole method. UTF-16 pages preserve surrogate
  pairs and bind to one text revision. Titles/descriptions do not stand in for
  full text. Retained text is bounded to 1,048,576 UTF-16 units per element and
  2,097,152 per observation; exhaustion is explicit rather than truncation.
- **Input and recovery adapters:** the existing CUA keyboard/mouse port remains
  unchanged. Native modal keyboard targets can decline background input before
  dispatch so the existing automatic foreground route can run once. A modal
  appearing during an admitted input sequence stops that sequence; it does not
  select a second route. Short recovery for typed AX `cannotComplete` rebuilds
  an entire read-only snapshot within the original deadline. A stopped provider
  can be replaced at the next request only after successful cleanup, with old
  refs retired and the original execution evidence preserved. These are
  DTW-owned adapters, not additional claims of upstream source migration.
- **Focused application proof:** only typed system-wide AX `cannotComplete` or
  `noValue` permits an alternate candidate, verified through the existing CUA
  WindowServer PSN comparison and process birth identity. Known targets use
  their PID directly; AppKit supplies only a candidate for an unknown prior
  application. Exact app-owned AXFocusedWindow identity remains mandatory for
  window input and return. Contradictory successful system AX evidence, other
  errors and unreadable exact windows do not select a favorable fallback. The
  observer no longer guesses a focused window from CG list ordering; a PID-only
  observation cannot establish window ownership. No new CUA ABI or input replay
  is introduced.

Association discovery covers proven visible relationships, not every possible
helper or all Spaces. The inventory exposes this distinction. An attached sheet
blocks its parent, without making an independent sibling window modal-blocked.
An application-modal dialog blocks other ordinary targets. For stacked direct
application-modal siblings, an exact current AXFocusedWindow root can prove
which modal is active and permit that modal to operate. That proof checks the owner, native window ID and typed
modal state, and is rechecked after sibling inspection. It does not bypass a
descendant modal or depend on titles or WindowServer ordering. Unreadable modal
state is not silently treated as false. No-window-ancestor menus do not
gain fictitious keyboard focus, and locked-screen support remains unclaimed.

Windows uses the existing UIA provider for subtree/region discovery and bounded
Value/TextPattern reads. Query matching now examines bounded native text before
making the 512-character display excerpt; a text budget gap is reported as
partial coverage. Menu discovery is explicitly unsupported on that provider.
Windows runtime behavior still requires the independent Windows acceptance run.

The macOS checkpoint provider in
`cmd/dtw/src/backends/macos/Native.swift` is a narrow, adapted source port of
[Cua Driver 0.38.0](https://github.com/trycua/cua/tree/85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19/libs/cua-driver/rust),
fixed at commit `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`. It uses the native
kernel and AX mechanisms described below directly. The checkpoint worker is a
DTW process boundary; it does not run the CUA worker or ToolRegistry for these
operations. The rewritten provider is now the only runtime; no CUA SDK or
upstream worker remains in the dependency graph.

This record covers source origin and deliberate adaptations. It does not assert
that a compile, fixture run, or generated implementation constitutes C2 or C3
acceptance. Checkpoint evidence and remaining platform coverage belong in the
checkpoint acceptance record.

## Donor manifest

Paths below are relative to the pinned CUA repository. Hashes are SHA-256 of the
original, unmodified files, checked on 2026-10-27. The source root for rows with
short paths is `libs/cua-driver/rust/crates/platform-macos/src/`.

| Original path | SHA-256 | Adapted responsibility |
| --- | --- | --- |
| `apps/mod.rs` | `ef76ce2eb70c857e571db5251b12ad3693e96c770a4998dbbfaa327d57b5368e` | `RequestContext.catalog`, `executable`, `bundle`: kernel PID enumeration, executable lookup, innermost application bundle classification |
| `windows.rs` | `60958be50493d4e3e52243dfdf6e39d765423f2059cb4cb7f8d3f2428948da87` | `cgWindows`: public WindowServer owner/ID/geometry metadata; no SkyLight enumeration path |
| `ax/enablement.rs` | `0c254408f71684bf8687cda4754ab5f9d99d6d3076579b7b656b17b87ae10c90` | `RequestContext.startStamp`: exact-size `proc_pidinfo(PROC_PIDTBSDINFO)` birth timestamp; browser AX opt-in and window-local readiness are adapted in the first release core batch |
| `ax/bindings.rs` | `00b68eaab97fdbef71402bc48e37941f84940e02facbb89362355aaeaa484df7` | AX attributes/actions, PID lookup, retained window arrays, exact `_AXUIElementGetWindow` identity and bounded parent ascent |
| `ax/exact_target.rs` | `a06cd9602add11130935a6d6777031b0aefccc2d50ffbe94f48ac4a171def3a7` | `NativeStore.candidates`, `validate`, `owned`: exact window ID and element/window relationship; no title/geometry identity guesses |
| `ax/window_scope.rs` | `e01d85f0218cb8bc38004489bb444479dd4b10d29bd5068e1a94896ebe5f3f39` | Initial `ordinaryWindow` role/subrole and open/save-panel classification; current transient/helper support is described above |
| `ax/snapshot.rs` | `15b05543c02e41a7499574f508e5a80ced4585b69d37651030cd21ddbbdc6874` | `RegisteredWindow`, `NativeElement`, `NativeObservation`: keep native objects alive with the reference and release with its owner |
| `ax/tree.rs` | `bdd1b29d6f3374bf9633c9669b9458e7d15690973f8070c932403461426980ab` | `NativeStore.observe`: bounded AX child traversal, readable attributes and original native element retention; full semantic tree heuristics are excluded |
| `LICENSE.md` (repository root) | `c0779290c1d4783169aa3dbfb55feb505e563ef8a004bbf55298ceffcfbda8d9` | Original CUA MIT copyright and license, retained below |
| `libs/cua-driver/rust/THIRD_PARTY_NOTICES.md` | `82470c85cb48dd084fccbf192d4a9f0b9d0afe614ab9b63e0fceac0eb315f412` | Reviewed source of upstream derived-code notices |

## Mechanical adaptation and DTW-owned behavior

The Rust donor's libproc calls use Swift's SDK-imported Darwin declarations and
`proc_bsdinfo`, avoiding a handwritten C ABI layout. The donor's process start
time tuple becomes a checked microsecond `UInt64` generation. Kernel birth time
is read before and after executable/bundle classification. App identity is an
unambiguous JSON array of canonical bundle path, bundle identifier and executable
path, with PID and birth generation kept separately.

macOS may report a code-sign-clone hard-link alias (`.app.bundle`) from
`proc_pidpath`. When that kernel path is not a normal application bundle path,
DTW obtains only that PID's registered executable/bundle URLs as a candidate.
It accepts the candidate only if its innermost bundle matches the registered
bundle and both executable paths are regular files with the same device/inode.
The surrounding kernel birth-stamp checks remain unchanged. Ordinary app and
nested helper paths never enter this normalization; no global Launch Services
lookup, parent-app guess or broad `.app.bundle` admission is used. Real hard-link,
different-inode, missing-file and helper regression controls cover the boundary.

The per-request metadata cache replaces repeated Info.plist reads within one
request. It does not cache process existence across requests. Unlike the donor's
deduplication policy, the catalog retains every qualifying, proven process
instance instead of choosing the lowest PID for a bundle. It excludes unreadable
processes, non-bundle executables, `LSUIElement`/`LSBackgroundOnly` bundles and
nonstandard-root bundles without an ordinary WindowServer window. Catalog
coverage is explicitly incomplete. It uses neither NSWorkspace's running-app
cache nor AX, screenshot capture or permission prompts.

Swift ARC strong references replace the donor's explicit `CFRetain`/`CFRelease`
wrappers. A registered semantic window holds its native AX object; a published observation
holds its own element objects. Native bindings are opaque UUIDs, scoped to the
worker and exact target window. A new semantic query replaces only that
window's previous semantic query; continuation retains its binding and earlier
elements. Input admission consumes both binding kinds on its own window.
Bindings are released by their Core owner and `release`. Cleanup is permitted after a deadline because
it only drops retained objects. There is no implicit action, focus change or
re-observation during cleanup.

Window surface generations combine a worker-local random namespace and a
checked monotonic counter. Inventory registers exact WindowServer owner/ID
against the verified app instance without requiring an AX window inventory.
An associated surface additionally retains its proven AX client root and actual
owner birth; a changed association receives a new generation. Semantic bindings
separately retain and revalidate their exact AX objects. Inventory reconciliation
retires departed apps and absent surfaces, then filters semantic windows and
observations against the final target generations. It has no 512-identity
lifetime cap. The Runtime's provider generation adds its separate worker
lifetime boundary. Tokens and target generations are internal references, not
authorization tokens.

`_AXUIElementGetWindow` remains a private macOS SPI. It is dynamically resolved
from HIServices and failure is explicit `unsupported_platform`; there is no
replacement based on titles, bounds, focused windows or a different process.
The current WindowServer surface inventory is bounded to 128 candidates per
app request. Semantic root discovery separately reads AXWindows, application
children and attached sheets, with a 128-root bound; it is not a prerequisite
for listing same-process surfaces. Semantic child discovery uses
`AXUIElementGetAttributeValueCount` followed by indexed
[`AXUIElementCopyAttributeValues`](https://developer.apple.com/documentation/applicationservices/1462060-axuielementcopyattributevalues) ranges of at most 17 objects: 16 new children
and one overlap anchor. An earlier adaptation encountered `illegalArgument`;
this implementation skips empty ranges, clamps to the current count, and reports
unavailable or changing ranges as incomplete coverage without a full-array
fallback. Count and overlap changes are detected; a live AX walk is not an
atomic snapshot and may miss changes outside its anchors.

Filtering happens while traversing native objects. A query retains its frontier,
child offsets, visited objects and already returned actionable elements. Each
page is bounded independently by match count (40 default, 100 maximum), 256
frontier steps, a 750 ms cooperative traversal budget and 12 KiB of native element
JSON. Identity verification still uses the enclosing call deadline. A row that
does not fit is buffered once for the next page, so it is not skipped. Each
query retains at most 8192 visited nodes, 4096 returned elements, 256 frontier
frames, 32768 child edges and depth 64; one worker retains at most eight semantic
queries. Known read gaps and exhausted scan bounds set `complete=false` and a
reason. Exhausted traversal may keep earlier elements actionable. Core and
protocol retain semantic and pixel observations independently per window, with
an unchanged 15-second lifetime from initial acquisition. Cursor continuation
does not extend it. Consumed public bindings release their element lookup maps.

These are DTW policies, not copied CUA limits. Native providers may still allocate
an AX string or the complete AXWindows CFArray before DTW bounds retained data.
The native element-byte cap does not include public UUID refs and envelopes, or
arbitrary values explicitly returned by a script. Read-only identity discovery
does not scan modal child trees. Input still requires exact window identity,
current enabled/action/ancestry checks and a proven modal boundary; its direct
child inventory uses checked indexed batches and a 4096-node cap. Unsupported
or failed mandatory child reads reject input.

Each admitted request converts its absolute deadline once to a monotonic budget
(at most 30 seconds); each AX call checks it and sets an element messaging timeout
of at most 0.2 seconds. The parent worker transport supplies the tighter active
request deadline and terminates a stuck process. This is cooperative checking
plus process isolation, not a claim that macOS native calls are cancellable.

The initial semantic action subset is enabled `AXButton`, `AXCheckBox` and
`AXRadioButton` elements that explicitly advertise `AXPress`. Before dispatch,
the provider rechecks process birth/identity, current exact window identity,
action support, enabled state and ancestry. The parent chain must reach the
retained window within 40 steps, even when `AXWindow` already matches, so a control
reparented into an unsupported sheet/menu is rejected. An explicit mismatched
`AXWindow` is never replaced by an ancestry guess. Success means only
`dispatched / unverified`; once `AXUIElementPerformAction` begins, errors and late
returns mean `unknown`, requiring independent observation before another action.

The `validate` operation and `dtwCheckpointValidate` bridge reuse this exact
window verification for the existing SCK capture and visible pixel-click path.
The SCK implementation is not sourced from CUA by this port. Its geometry and hit
stack checks remain separate from native app/window identity validation.

## Initial C2 deferrals (historical)

This paragraph records the original C2 boundary. Later C5/C6 and first-release-core
sections below record the additional engine, enablement, input and platform groups.
The initial slice did not migrate the full CUA snapshot store, ToolRegistry, SDK,
QuickJS bridge, browser session lifecycle, browser AX enablement, menu activation,
selection, set-value, helper/modal window correlation, focus activation or input
fallbacks. The current implementation still does not import SkyLight enumeration or the
`input/skylight.rs::activate_without_raise` sequence. Windows UIA is now recorded separately in the cross-platform manifest; Windows
Graphics Capture remains deferred. macOS compilation or GUI evidence says nothing
about Windows acceptance.

The reviewed upstream notice attributes the focus-without-raise sequence to
yabai, Windows paths to trope-cua/Interface-Agent, JS engine paths to
QuickJS-ng/rquickjs, and named SDK/Linux paths to T3 Code. The native macOS port
does not use those named source paths; the separate C5 engine port below does
use the named JS engine dependencies. This is a source mapping, not a
legal conclusion. Historical packages of the complete pinned CUA worker retain
their accompanying notices. The rewrite does not redistribute that worker;
its adapted source retains the full CUA MIT text below and the attributions in
`THIRD_PARTY_NOTICES.md`. A future rewritten package needs its own dependency
inventory and license payload.

## Updating the donor baseline

1. Fetch an explicit upstream tag and commit into a separate source directory;
   do not change DTW's runtime pin as a side effect of examining a release.
2. Verify all original file hashes in the manifest against that revision and
   compare each mapped responsibility, including its ownership and helper
   dependencies. Review upstream license/notice changes at the same time.
3. Record whether each upstream change is applicable, deliberately excluded or
   ported, with the original commit and the DTW adaptation. Update hashes only
   when the donor baseline actually changes, preserving the prior manifest in
   version history.
4. Re-run deterministic Runtime/worker boundary tests, native builds, then C2 and
   C3 separately on real macOS GUI fixtures. Verify changed-window/PID generation,
   permission denial, bounded deadlines, cleanup, cancellation and unknown
   effects with dispatch counts or independent state readback. Never infer effect
   verification from an AX success code or infer native acceptance from a worker
   wrapper test. Platform-specific acceptance remains separate.

## Modal exclusion in the initial slice

The donor's `window_scope.rs` includes application-level sheet surfaces and
`exact_target.rs` maps a sheet to its proven parent window. DTW deliberately
does not port sheet routing yet. Instead, every operation requiring an exact
window checks the current app's AXWindows and AXChildren for sheets and modal
windows. A reported modal/sheet surface rejects the app's current operation with
`unsupported_modal`. This is conservative app-wide exclusion; finer handling of
document-modal sheets attached to another window is deferred. A normal floating
peer window with `AXModal=false` does not itself block the operation.

The public SDK declares AXModal required for every window. Missing or malformed
Boolean state, missing required inventories, and exhausted verification limits
return `modal_state_unproven`; they do not prove the window is unblocked. The
target also checks a provider's optional `AXSheets` array and its standard
AXChildren. AXSheets is not a public SDK attribute constant, so its absence is
never the sole evidence of no sheet. The same checks run before publishing an
observation, again during press admission, and through the shared validate path
used for capture and pixel dispatch. They do not consult focused windows or
titles and do not enable controls behind a modal merely because AXEnabled is true.

Modal verification is bounded by the existing request deadline, at most 128
element visits across each native call's checks, and at most 128 entries in each
required inventory. This budget is separate from the snapshot's 128 output
nodes. Incomplete verification rejects the operation instead of widening scope.

During initial GUI acceptance, AXWindows returned `cannotComplete` at the 0.2
second messaging timeout three times; the final source rebuild subsequently
read the same fixture successfully. The cause remains unproven. This observed
limit is retained separately from the AXChildren `illegalArgument` observation; neither
case justifies automatic retries or a focus/title fallback. The dedicated live sheet refusal and nonmodal floating-peer coexistence checks
have passed; their scope and evidence are recorded in the acceptance report below.

## C5.2 inventory attribution adaptation

DTW now classifies discovery exclusions separately from unresolved PID, role,
subrole, exact ID and CG owner evidence. Any unresolved candidate rejects the
whole inventory; no partial registry is published. Empty AX inventory alongside
eligible same-PID CG surfaces also rejects. Fixed counters and at most eight AX
SPI error codes reach the existing backend error message; no titles or foreign
app metadata are included. Explicit nonzero layers remain excluded, and extra
CG decoration surfaces alone do not establish missing AX windows. This is a
DTW-owned fail-closed adaptation, not a new donor migration or Space resolver.

Real closed-window fixtures may retain CG surfaces after AXWindows becomes
empty. They remain unresolved under this limited provider; offscreen status is
not proof of closure. [C5.2 acceptance](window-inventory-acceptance.md) separates
this observed limit, controlled AX-empty failures, and successful foreground /
background fixture reads from the still-unresolved historical intermittent case.

## Retained CUA license

The following is the original `LICENSE.md` text at the pinned commit.

```text
MIT License

Copyright (c) 2025 Cua AI, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

Real fixture evidence for discovery, SCK, AX, sheet refusal and cancellation
is recorded in [C2/C3 macOS acceptance](native-checkpoint-acceptance.md).


## C5 bounded app script port

The default app protocol directly uses rquickjs **0.14.0**, with only its `std`
feature. The dependency no longer comes through the CUA SDK. Its bundled
QuickJS-ng is **0.16.2**. No custom allocator, module loader, OS binding or extra
JavaScript parser is introduced. The fixed CUA donor remains
`85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19` / 0.38.0.

| Donor path (under `libs/cua-driver/rust/crates/cua-driver-core/src/script_tool/`) | Original SHA-256 | DTW adaptation |
| --- | --- | --- |
| `engine.rs` | `fa61393ba5b685df34858a81e8e32e8fbdab77e17f2e1094514b43510891242d` | QuickJS runtime/context setup, memory/stack limits, interrupt and job draining; `script_engine/mod.rs` uses bounded std channels and an absolute Core deadline |
| `prelude.js` | `af23eaf9c7c198f7feb6ac38e26c94d5b4d76bc0b794c491b80adb9933ed2865` | Hidden bridge, console/error/settlement mechanics; the initial C5 facade replaces upstream methods with five app-bound methods (now expanded to ten), with strict JSON state/result encoding |

The complete upstream `App`, `Locator`, raw invocation, global discovery/launch,
file/network/process and unrestricted platform objects are excluded. DTW owns
target binding, protocol parsing, sequential Core dispatch, per-step receipt retention,
state commit and cleanup; the VM is not an authorization authority. Unlike the
donor's execution path, every native operation goes through the same Core used
by atomic app requests. The memory budget uses QuickJS's native allocator limit;
a caught allocation failure may be recovered from within that unchanged budget.

The upstream notice attributes QuickJS-ng to Fabrice Bellard (2017–2026), Charlie
Gordon (2017–2024), Ben Noordhuis (2023–2026), and Saúl Ibarra Corretgé
(2023–2026); rquickjs to Mees Delzenne (2020) and the rquickjs contributors
(2025). Original license/copyright files remain in the dependency sources; the
rquickjs-core license copy and provenance are retained under
`packaging/third-party/`, in addition to the CUA MIT notice above. Validate this port's VM, Core and
real GUI gates independently when updating any of these pinned sources.


## C6 source verification and resource ownership

The fixed donor revision is evidence, not a runtime dependency or an instruction
to compile all upstream CUA. Before adding a capability group:

1. Read the relevant file at the recorded donor commit, record its repository
   path and SHA-256, and compare the original bytes with the recorded hash.
   Hash `git show <donor-commit>:<path>` output, not a dirty working-tree copy.
2. Record exactly which behavior is reused and what DTW changes. Preserve the
   source's copyright/license and inspect later upstream fixes affecting that
   behavior; a newer upstream release does not automatically change this pin.
3. Name the persistent resource owner before porting code: Core owns target/ref
   validation, the provider owns native resources, and the app execution context
   owns their lifetime. Input sessions must explicitly own event sources, pressed
   buttons/modifiers and cleanup across success, cancellation, timeout, EOF and
   parent exit. Browser profiles and UIA/COM threads require their own owners.
4. Implement one complete vertical example through MCP, script schema, Core,
   provider, independent effect readback and cleanup before duplicating a group.
   Never copy CUA's registry, global facade, worker packaging or approval model as
   a shortcut around these contracts.
5. Record supported platform/window conditions and real native evidence separately
   from unit and fixture tests. Update this source ledger and
   [the product matrix](product-matrix.md). CI should protect demonstrated stable
   behavior without rebuilding the complete donor.

The detailed capability-group owner matrix is in the
[current architecture baseline](next-architecture-baseline.md). This procedure
adds no new public capability and makes no claim that C6 has already shipped.


## First release core: macOS surfaces and common input

The donor commit remains `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`.
`platform-macos/src/input/keyboard.rs` SHA-256 is
`5ed07b20a5f378c8429cd09b5a5cf608ddcd1a6a840888d23d37e30ca800514a`.
The previously listed `ax/enablement.rs` and `ax/bindings.rs` supply the native
opt-in/attribute references; no upstream worker, SDK or full event orchestrator
is linked. Unicode pairs, keycode/chord mapping and release ordering are adapted
to DTW's bounded owner. CRLF is one Return; physical input checks the exact target
between complete input segments.

DTW additionally retains each private CoreGraphics event source through release
acknowledgement. It compares the private and combined-session baselines with
actually posted release counts and both tables' progress, held keys/buttons and
touched modifier flags. Private progress can precede combined-session progress;
combined counters alone cannot attribute physical input to this source.
Normal completion and worker shutdown use the same bounded pending-input
drain; a temporarily empty global key table is insufficient. Counter reads run
through at most one asynchronous probe per source so a stalled getter cannot
trap the shutdown thread. A drain timeout permanently closes admission and
retains cleanup failure, including across concurrent shutdown attempts. This
is internal delivery/cleanup evidence, not application-effect verification.

`platform-macos/src/input/mouse.rs` SHA-256 is
`66d728101546a4de74fe4bfab5c302922c1ec45d7f77ac9ba940f85a5a7a85d8`.
Lines 269–301 inform pointer warp/tracking; 304–343 document upstream scroll
routing uncertainty; 1696–1773 provide the targeted line-scroll reference.
DTW uses one line event after pointer settling, retains the source through a
bounded drain and revalidates the target. It does not import the donor's private
event stamps, multiple dispatch routes, primer events or pixel conversion.
Dispatch still requires independent application-effect readback.

The WindowServer surface registry, separate SemanticBinding, per-window cold
content readiness, process-wide input lease, exact target revalidation and
semantic setter are DTW-owned native-SDK integrations. macOS string setters
are limited to writable AXTextField/AXTextArea/AXComboBox controls; a broad
AXValue-settable response on a structural node is not text-edit capability. Surface enumeration no
longer requires AX windows; semantic and keyboard operations retain their own AX
and focus conditions. Per-point ScreenCaptureKit visibility checks are owned by
a short-lived scene process, including wheel input. These changes do not promise
background, lock-screen or pointer-independent input.

Windows and experimental X11 donor hashes, changes and additional notices are
recorded in [the cross-platform manifest](native-source-provenance-cross-platform.md).
