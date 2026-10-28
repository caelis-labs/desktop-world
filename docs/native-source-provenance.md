# Native source provenance

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
| `ax/enablement.rs` | `0c254408f71684bf8687cda4754ab5f9d99d6d3076579b7b656b17b87ae10c90` | `RequestContext.startStamp`: exact-size `proc_pidinfo(PROC_PIDTBSDINFO)` birth timestamp; browser AX enablement is excluded |
| `ax/bindings.rs` | `00b68eaab97fdbef71402bc48e37941f84940e02facbb89362355aaeaa484df7` | AX attributes/actions, PID lookup, retained window arrays, exact `_AXUIElementGetWindow` identity and bounded parent ascent |
| `ax/exact_target.rs` | `a06cd9602add11130935a6d6777031b0aefccc2d50ffbe94f48ac4a171def3a7` | `NativeStore.candidates`, `validate`, `owned`: exact window ID and element/window relationship; no title/geometry identity guesses |
| `ax/window_scope.rs` | `e01d85f0218cb8bc38004489bb444479dd4b10d29bd5068e1a94896ebe5f3f39` | `ordinaryWindow`: window roles/subroles and explicit open/save-panel identifiers; transient/helper resolution is excluded |
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

The per-request metadata cache replaces repeated Info.plist reads within one
request. It does not cache process existence across requests. Unlike the donor's
deduplication policy, the catalog retains every qualifying, proven process
instance instead of choosing the lowest PID for a bundle. It excludes unreadable
processes, non-bundle executables, `LSUIElement`/`LSBackgroundOnly` bundles and
nonstandard-root bundles without an ordinary WindowServer window. Catalog
coverage is explicitly incomplete. It uses neither NSWorkspace's running-app
cache nor AX, screenshot capture or permission prompts.

Swift ARC strong references replace the donor's explicit `CFRetain`/`CFRelease`
wrappers. A registered window holds its native AX object; a published observation
holds its own element objects. Native bindings are opaque UUIDs, scoped to the
worker, replaced by the next semantic observation, consumed before a press
attempt and released by `release`. Cleanup is permitted after a deadline because
it only drops retained objects. There is no implicit action, focus change or
re-observation during cleanup.

Window generations combine a worker-local random namespace and a checked
monotonic counter. They are minted only after agreement between current
WindowServer owner/ID and a same-PID AX window's exact ID. Reusing the numeric
window ID is insufficient: a refreshed AX window must also compare equal to the
retained object. The Runtime's provider generation adds its separate worker
lifetime boundary. Tokens and target generations are internal references, not
authorization tokens.

`_AXUIElementGetWindow` remains a private macOS SPI. It is dynamically resolved
from HIServices and failure is explicit `unsupported_platform`; there is no
replacement based on titles, bounds, focused windows or a different process.
The donor's single `AXUIElementCopyAttributeValue` array read is retained. An
initial adaptation to indexed `AXUIElementCopyAttributeValues` returned
`illegalArgument` while reading AXChildren in a real fixture. That error is not
treated as proof of an empty child array: the indexed API was removed in favor
of the donor's array read rather than retried.
The complete native CFArray exists temporarily; only the bounded prefix is
retained as Swift elements. Thus tree/response limits do not bound the native
allocation needed to receive an individual AX array. Standard-window discovery
is limited to 128 AX windows and 512 retained window registrations. Observation
is limited to 128 nodes and depth 16; bounded traversal reports truncation.
The Rust adapter rejects truncated or unmarked snapshots and releases their
retained binding rather than publishing a silently partial tree.
These limits are DTW policy, not copied CUA limits.

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

## Explicitly deferred source groups

This slice does not migrate the full CUA snapshot store, ToolRegistry, SDK,
QuickJS bridge, browser session lifecycle, browser AX enablement, menu activation,
selection, set-value, helper/modal window correlation, focus activation or input
fallbacks. It does not import SkyLight enumeration or the
`input/skylight.rs::activate_without_raise` sequence. Windows UIA and Windows
Graphics Capture are separate future provider groups; macOS compilation or GUI
evidence says nothing about Windows acceptance.

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
| `prelude.js` | `af23eaf9c7c198f7feb6ac38e26c94d5b4d76bc0b794c491b80adb9933ed2865` | Hidden bridge, console/error/settlement mechanics; the facade is replaced with five methods bound to the existing app context, with strict JSON state/result encoding |

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
