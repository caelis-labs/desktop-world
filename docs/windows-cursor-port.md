# Windows session cursor and native parity follow-up

Windows now implements the existing session cursor contract using a private,
same-executable Win32 presentation worker. This is optional visual feedback;
pointer and keyboard delivery still use foreground input and the shared physical
pointer. It does not establish macOS background-input parity.

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
  providers. Semantic-only sessions start no helper. Its UUID, palette color and
  four-hex badge stay stable across app switches and helper replacement.
- Feedback follows successful pointer input segments. UDP is loopback-only,
  nonblocking and authenticated by a private random token. Operation/sequence
  generations reject delayed events and stale hides. Visual failure never
  changes an input receipt, authorizes input or replays an operation.
- The disabled layered tool window never activates, intercepts input, becomes
  topmost or raises its target. It stays immediately above the exact ordinary
  target, below unrelated covering windows, and is clipped to target bounds.
- Target PID, process birth, executable identity, HWND incarnation, geometry,
  application DPI and monitor layout/scales are revalidated. Move, resize,
  disable, close, display change or loss of identity retires the operation.
  Raster scale follows the monitor under the physical event point, including
  DPI-unaware targets; idle checks do not continuously composite frames.
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

## Evidence and remaining coverage

The 2026-10-31 development binary `ea973ccd8c52e5dd262d0400e684da4131f3b1fd7328eb0bac6d0a261f84c9ad`
passed 385 library tests, 3 CLI tests, all 25 base GUI cases and 15 cursor/parity
GUI cases. The latter use standard MCP plus an independent read-only Win32 and
physical-pixel observer. They cover two simultaneously visible distinct session
markers, cross-app continuity, exact capture exclusion, a same-class/same-basename
foreign negative control, covering-window order, geometry retirement, native
modal waits, both wheel anchors, helper recovery, in-flight cancellation and
concurrent input handoff, in-flight EOF/parent death and session isolation.
Retained process handles establish actual native/helper exit codes, rather than
treating process absence as successful cleanup.

These are normal-user Windows 11, primary-monitor 125% tests. They do not certify
mixed-monitor movement/scaling, elevation/secure desktop, blocking UIA calls,
background input, WGC geometry, other host session-approval policies or a final
distribution artifact. See the dated Windows acceptance record for Agent tasks
and package evidence. No macOS native source, public API or Skill is changed.
