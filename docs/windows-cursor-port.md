# Windows session cursor and native parity follow-up

Windows now implements the existing session cursor contract using a private,
same-executable Win32 presentation worker. This is optional visual feedback;
Windows selects targeted mouse messages where supported and raises the target
for other input. It does not establish full macOS background-input parity.

Application discovery, lifecycle (start/quit) and pointer-gesture support are
separate provider contracts that change more often than this port record. A method
being public does not mean this provider implements it: read the installed
[Skill API](../skills/desktop-world/API.md) and the provider's own acceptance
record for the boundary in force. The current development branch's contracts,
GUI results and limits are recorded in [Windows fixes acceptance](windows-fixes-acceptance-2026-11-03.md).

## Visual feedback and input routing

The colored marker is a presentation window, not another OS mouse device.
The foreground mouse route uses `SendInput` with an absolute move on the shared
system pointer, so the real cursor and marker move together on that route.
Targeted HWND mouse messages leave the physical cursor fixed. Semantic
`press`/`setValue` use the control's UIA patterns. See Microsoft's [mouse input](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput),
[shared cursor](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcursorpos)
and [UIA Invoke](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationinvokepattern-invoke)
contracts.

Moving the real cursor is a property of this input route, not a requirement for
every Windows automation action. A later upstream comparison at CUA
[`8db684185a93e854bb7811189d69f933bb15f55d`](https://github.com/trycua/cua/tree/8db684185a93e854bb7811189d69f933bb15f55d)
finds additional Windows routes:

- [Window-targeted posted messages](https://github.com/trycua/cua/blob/8db684185a93e854bb7811189d69f933bb15f55d/libs/cua-driver/rust/crates/platform-windows/src/input/mouse.rs#L138)
  select a child HWND and post move/button messages without moving the system
  mouse. Acceptance by the target is separate from queue insertion.
- [Synthetic pen injection](https://github.com/trycua/cua/blob/8db684185a93e854bb7811189d69f933bb15f55d/libs/cua-driver/rust/crates/platform-windows/src/input/inject.rs#L307)
  implements another coordinate route intended to preserve the user's mouse
  location, with button, visibility and activation constraints. Microsoft's
  [synthetic pointer API](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createsyntheticpointerdevice)
  creates touch or pen devices; it does not create an independent ordinary mouse
  cursor per Agent.
- The upstream [click dispatcher](https://github.com/trycua/cua/blob/8db684185a93e854bb7811189d69f933bb15f55d/libs/cua-driver/rust/crates/platform-windows/src/tools/click.rs#L1090)
  distinguishes semantic, message, synthetic pointer and foreground SendInput
  routes. Support for one gesture does not establish all buttons, modifiers,
  drag or scroll behavior.

The current Windows interaction path prepares the exact foreground receiver.
Semantic controls use UIA and pointer gestures use shared physical input with
verified geometry and hit testing. The input scope pairs releases and settles
owned foreground and pointer resources. DTW uses synthetic pen only to activate
a proven native caption when ordinary foreground coordination is refused.

## Source comparison and attribution

The reference is MIT-licensed CUA commit
[`b38360443960a6540685a9a5eb5407f10fe3d03b`](https://github.com/trycua/cua/tree/b38360443960a6540685a9a5eb5407f10fe3d03b).
The Windows overlay, input, capture and scroll files were also compared with
upstream HEAD `a9902d272655b65180fa713a8b1f2f1c1935e70b` on 2026-10-31.
The exact files below are under `libs/cua-driver/rust/` unless stated otherwise.

| Donor path | SHA-256 | DTW adaptation |
| --- | --- | --- |
| `crates/platform-windows/src/overlay.rs` | `cb3cac0b729d0dd7ee88d6887eb18b76fd6aa62ebb05ab9eb4e380e6b6f40b4a` | Layered, transparent, nonactivating window and premultiplied GDI presentation; bounded supervised worker instead of a global registry |
| `crates/cursor-overlay/src/theme.rs` | `08e04ecab19be1d9cdf5beaa2564e9855457f3b50dd2b7d38e9487214b51bc77` | Same nine session colors |
| `crates/cursor-overlay/assets/build_default_theme.py` | `caa536a26215e51d0d1cc9e05f632ffc9f2b1a60b696cbe497af20c0f49d0b1c` | DART cubic outline and 20-degree rotation, rasterized locally without upstream assets or fonts |
| `crates/platform-windows/src/input/inject.rs` | `3898a9154fa79467bde340bd69fa655b88bc81f20390819964b04cf983c70859` | Queue attachment is best effort; one foreground request and exact-HWND confirmation remain mandatory |
| `crates/platform-windows/src/capture.rs` | `480e7715c970df0d32459e3ac97fe13911f20f79d01d76f08f75c7fdc0fc8b98` | Exact-HWND PrintWindow rendering, without desktop-copy fallback |
| `crates/platform-windows/src/wgc.rs` | `a79667cc79e10a2046d356f937618ecf89e0309f350eafaff464e2d7f55d18a4` | Reviewed, not imported: upstream WGC does not supply proven screen-origin geometry for mapped pixel input |
| `crates/platform-windows/src/uia/scroll.rs` | `e87bfcc2a9f6199e60b33f56a99d7ce945518950731f9dc50aa7c43ac98c9923` | UIA bounds and live ancestor clipping for semantic wheel anchors |

The full CUA MIT license remains in
`packaging/third-party/cua-background-input/LICENSE`; existing Windows donor
notices remain in [the cross-platform provenance record](native-source-provenance-cross-platform.md).
The upstream SDK, overlay registry, renderer dependencies, global capture
exclusion, reserved-key assistance and desktop-copy fallback are not imported.

## Ownership and rendering

- One lazily started helper belongs to one MCP session and is shared by its app
  providers. Semantic-only sessions start no helper. Its internal UUID and visible palette color
  stay stable across app switches and helper replacement; no identity badge is shown.
- Feedback follows successful pointer input segments. UDP is loopback-only,
  nonblocking and authenticated by a private random token. Operation/sequence
  generations reject delayed events and stale hides. Visual failure never
  changes an input receipt, authorizes input or replays an operation.
- The disabled layered tool window never activates, intercepts input, becomes
  topmost or raises its target. It stays immediately above the exact ordinary
  target, below unrelated covering windows, and is clipped to target bounds.
- Target PID, process birth, executable identity, HWND incarnation, geometry,
  application DPI and monitor layout are revalidated. The owned PMv2 helper's
  effective DPI is monitored independently of the target application's DPI.
  Move, resize,
  disable, close, display change or loss of identity retires the operation.
  Raster scale comes from the helper HWND on the monitor under the physical
  event point, including DPI-unaware targets. Cross-monitor placement happens
  hidden, without activation or raising. Idle checks do not continuously
  composite frames. A rounded OS scale index is not used as raster DPI.
- Capture remains exact-target PrintWindow, so DTW images omit its marker while
  physical screen pixels and external recorders retain it. Only exact proven
  DTW helper windows are exempted from occlusion/hit admission. Class, basename,
  title and files on disk cannot grant this exemption. A kernel-identified local
  pipe server must prove the same executable, user, process birth and immutable
  helper HWND, with a nonce response and live rechecks.
- Cancellation hides the operation after native input shutdown. EOF and parent
  death close the session helper. Recovery after a presentation-only crash is
  permitted only on a later independent action, after the old process is reaped.

## Other Windows parity work

Existing public APIs now support pixel and semantic wheel anchors. Both retain
their original observation lifetime and target/region checks; semantic anchors
use live positive bounds clipped by their UIA ancestors. Native window waits
support closed, unblocked and uniquely proven same-process owned modal dialogs,
using Win32 owner chains and UIA WindowPattern.IsModal. HTML dialogs and
cross-process dialogs are not inferred from titles.

Input cancellation now waits for the complete transaction lease to unwind after
the last key pair. A reproduced abandoned-mutex failure in a queued second
session is retained separately from the passing rerun. Parent disappearance is
a normal native-worker shutdown; malformed private protocol remains an error.

The DPI-unaware control exposed a real capture defect: at 125%, a physical
520x586 frame contained a 416x469 logical PrintWindow render. Windows now derives
the target's backing extent using its exact HWND and
[PhysicalToLogicalPointForPerMonitorDPI](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-physicaltologicalpointforpermonitordpi),
renders only that window, then maps that bitmap into the physical output/crop.
No desktop pixels or guessed DPI multiplier are used. The backing extent is
rechecked after rendering and retained in pixel/region bindings, including each
input segment's admission. DPI-aware windows keep their original one-to-one map.
The cursor is independently sized from its own PMv2 window, not the foreign
target's 96 DPI. The control checks actual physical pad edges and click effect,
plus a 118x85 cursor panel at 120 DPI.

WordPad also exposed an empty native runtime-ID SAFEARRAY (`0..-1`). An
unidentifiable node now records `native_read_gap`, preserving independent
siblings instead of aborting the entire page. It never becomes complete coverage
or proof of absence/uniqueness. Total scanned nodes remain bounded even when IDs
are unavailable. Before reading an ID, Windows validates its dimension, element
size, VT_I4 type and bounds, and uses SafeArrayGetElement rather than unchecked
pointer slicing. These follow Microsoft's
[UIA SAFEARRAY client requirements](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-workingwithsafearrays).
The original real WordPad failure and the later 19-element partial observation
are kept as separate evidence, with tests for empty/wrong-type/nonzero-origin
arrays and preservation of signed opaque ID components.

## Verification boundaries

Use the native Windows acceptance matrix for independent pointer, screenshot,
DPI, session-isolation, cancellation and helper-exit controls. UIA/SendInput return
values alone do not prove application processing. Preserve unknown effects and
read original targets independently; do not replay or repair uncertain input.
Normal-user desktop evidence does not establish elevated/secure desktop,
every mixed-display layout or blocking-provider recovery.

## Historical targeted message source reference

The retired Windows message dispatcher was compared with CUA commit
`8db684185a93e854bb7811189d69f933bb15f55d`:
`platform-windows/src/input/mouse.rs` (Git blob `3923d75f9acdc17fd5f3507c30ed4d7733e800e0`),
`platform-windows/src/tools/click.rs` (`e76b76ddced1a08dff8ff4264d3ad3455c34032a`),
and `platform-windows/src/tools/scroll.rs` (`13e5625fd878f0976a36ca87934117eff14bbec6`).

The current Windows interaction route uses verified foreground receivers and
physical input for pointer gestures. Run `browser_mouse_probe.py` for a real
content click, exact dialog effect and pointer settlement.
