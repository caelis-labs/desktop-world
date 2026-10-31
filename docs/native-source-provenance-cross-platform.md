# Windows and experimental Linux source adaptation

Source baseline: [Cua Driver 0.38.0](https://github.com/trycua/cua/tree/85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19), commit `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`.
Original file hashes below were read from that exact checkout for the 2026-10-28
inventory; the foreground attachment reference was verified on 2026-10-31.
This is a source record, not a Windows/Linux runtime or release acceptance claim.

| Original repository path | SHA-256 | Adaptation/reference |
| --- | --- | --- |
| `libs/cua-driver/rust/crates/platform-windows/src/win32/apps.rs` | `3842a32c1c98bb286c33c958a907dc8b003bb4a8d5940e441cec915bbf847413` | ToolHelp process/parent enumeration |
| `libs/cua-driver/rust/crates/platform-windows/src/win32/windows.rs` | `0e4013949b10a5c991ae7245fe1250fb9249682ad23e894c2f62d2bd91e1e3b0` | exact HWND/PID checks and window enumeration |
| `libs/cua-driver/rust/crates/platform-windows/src/uia/mod.rs` | `90361fbcd469c2f26eccd129232d6959ceccc7b3a8bb044988d3d808839c5bcc` | UIA native properties and Invoke/Toggle/Selection/Value patterns |
| `libs/cua-driver/rust/crates/platform-windows/src/input/keyboard.rs` | `bc9aa34e811505fc75a34b1e36f6879e73a5e9dcfad4582253d2e7fe2996d5c8` | UTF-16 SendInput pairs and virtual-key transactions |
| `libs/cua-driver/rust/crates/platform-windows/src/input/mouse.rs` | `1f4b77b21183636425b353a9b0f225fe5c9961f9a228a27925dfdbac1ca66971` | Win32 pointer/button input primitives |
| `libs/cua-driver/rust/crates/platform-windows/src/input/inject.rs` | `3898a9154fa79467bde340bd69fa655b88bc81f20390819964b04cf983c70859` | Scoped foreground-thread attachment; DTW also attaches the target queue, creates the worker queue, pairs detaches and bounds exact-HWND confirmation |
| `libs/cua-driver/rust/crates/platform-windows/src/capture.rs` | `480e7715c970df0d32459e3ac97fe13911f20f79d01d76f08f75c7fdc0fc8b98` | Exact-window PrintWindow/GDI rendering, physical frame cropping and blank-render detection; no desktop-copy or WGC fallback |
| `libs/cua-driver/rust/crates/platform-linux/src/x11_client_pid.rs` | `9af4e64b32c04e65215d2ad1d08987a9ec2d93e89361adb8c8b7441546560ef4` | verbatim pure XRes ownership selection and eight tests |
| `libs/cua-driver/rust/crates/platform-linux/src/x11/mod.rs` | `66aac02e51b1e0d161846910910cf4b1031df0f28127223cd1c4df12f8cabda3` | XRes calls, X11 window discovery and input primitives |
| `libs/cua-driver/rust/crates/platform-linux/src/capture.rs` | `c1eaf2046fd49af871e77ad460d2e71e4a46d70fcbb68ad12c362700bbc74306` | XGetImage native frame/pixel layout validation |
| `libs/cua-driver/rust/crates/platform-linux/src/input/foreground.rs` | `ab66a0922e0f0389c774500c0be3377108627216916c9b3188720dc553a4567c` | foreground target guard boundary |
| `LICENSE.md` | `c0779290c1d4783169aa3dbfb55feb505e563ef8a004bbf55298ceffcfbda8d9` | CUA MIT license |
| `libs/cua-driver/rust/THIRD_PARTY_NOTICES.md` | `82470c85cb48dd084fccbf192d4a9f0b9d0afe614ab9b63e0fceac0eb315f412` | upstream Windows source notices |

## Implemented source boundaries

- `backends/windows`: native Win32 process creation stamp and executable app ID;
  EnumWindows inventory independent of UIA; UIA lazy child/sibling traversal,
  role/text filtering and retained element actions; exact-window rendered GDI PNG;
  foreground UTF-16 text, keyboard chords, scroll and left/right/double clicks.
- `backends/linux`: experimental local X11/EWMH inventory and XRes 1.2 process
  proof, `/proc` process birth, unoccluded XGetImage client PNG, XTest clicks,
  focus, mapped text, chords and scroll. Wayland and XWayland are explicitly
  rejected; there is no portal, shell helper, AT-SPI, clipboard or keymap fallback.
- The pure `linux/client_pid.rs` is copied intact, including its eight ownership
  tests. Other files adapt the donor mechanisms into DTW's contracts; they are
  not wholesale copies of upstream tools or a claim of upstream behavior parity.
- No SDK, upstream worker, ToolRegistry, browser/DOM, keyring, clipboard,
  Wayland helper or complete platform crate is imported. Runtime dependencies are
  the target-specific `windows 0.58.0`, `x11rb 0.13.2`, PNG encoder and libc, plus
  the existing DTW protocol dependencies. Actual package dependency notices are
  still a release-candidate responsibility.

## Windows session cursor follow-up (2026-10-31)

The Windows-only presentation worker additionally adapts CUA commit
`b38360443960a6540685a9a5eb5407f10fe3d03b`:
`platform-windows/src/overlay.rs` (SHA-256
`cb3cac0b729d0dd7ee88d6887eb18b76fd6aa62ebb05ab9eb4e380e6b6f40b4a`),
shared `overlay/theme.rs` (`08e04ecab19be1d9cdf5beaa2564e9855457f3b50dd2b7d38e9487214b51bc77`)
and `build_default_theme.py` (`caa536a26215e51d0d1cc9e05f632ffc9f2b1a60b696cbe497af20c0f49d0b1c`).
Full donor paths, current upstream comparison, behavioral differences and
verification are recorded in
[the Windows port record](https://github.com/caelis-labs/desktop-world/blob/main/docs/windows-cursor-port.md).
The full MIT license is retained in
`packaging/third-party/cua-background-input/LICENSE`. No upstream registry,
image/font asset, SDK or renderer dependency is imported. Additional windows
crate feature flags are Windows-target-only; locked dependency versions remain
unchanged. Session wiring in common source is conditional on Windows.

## Deliberate DTW behavior

Each provider is a private process of the installed DTW executable. Native UIA
objects and X11 state never cross IPC. The common adapter binds each response to
its original request ID and exact app/window generations; transport interruption
is never replayed. Windows monitors a held parent-process handle and a cooperative
stop event; Linux monitors parent reparenting and SIGTERM. Both also close input
admission on pipe EOF. An admitted transaction gets at most 400 ms to drain; the
supervisor's existing 500 ms grace and exit 74 cleanup failure remain authoritative.
Generic Windows subprocess transports retain their prior termination behavior;
native adapters explicitly choose cooperative shutdown.

Window incarnation is an opaque nonzero 64-bit OS-window property, not a UIA
runtime ID, PID alone, title or geometry guess. It is preserved across DTW owners
and automatically lost when the OS destroys the window. Inventory may install the
property; it does not remove a marker still used by another live session. A race
creating a marker can invalidate a newly observed reference but cannot validate
an old incarnation. These markers are not authorization or security tokens.

Windows semantic queries retain a lazy DFS frontier, native runtime-ID visited
set, root, matched elements and at most one pending output row. Limits are eight
queries, 8192 visited nodes, 4096 retained matches, depth 64, 256 page steps,
750 ms cooperative page time, 100 output matches and 12 KiB element JSON per page.
No whole-tree discovery occurs before filtering. The original 15-second query
lifetime does not renew on continuation; earlier page elements remain usable.
Missing/changing native reads are partial coverage, never proof of absence. UIA
calls can still block: the supervising process deadline is the final bound.

Input uses one full transaction lease across DTW processes: a Windows named mutex
for the login session, or the validated same-UID `/tmp/dev.caelis.dtw-input-UID.lock`
on X11, independent of each host's `TMPDIR`. Preconditions read real held
keys/buttons, including the full XInput 2 button mask for horizontal wheel buttons;
an unlocked OS lease never proves a previous input was released.
Geometry/DPI and point ownership are checked after focus and pointer placement.
Text scalars, complete chords, click pairs and wheel steps pass through the same
small sequence runner, which rechecks exact target/focus before every segment.
Text accepts at most 16 KiB UTF-8 and 48 KiB JSON-encoded data, folds CRLF to LF,
and has no extra 8192-character limit. Scroll uses signed wheel steps within
-100..100: Windows uses 120 wheel units per step, X11 one button pair per step.

Partial injection is never replayed. Windows releases only the held inserted
prefix reported by SendInput. X11 acknowledges each native event and releases only
confirmed remaining downs on a definite protocol failure. A missing up
acknowledgement is never retried; uncertain held state becomes separate sticky
`shutdown_error` without replacing the primary native result. Failed cleanup keeps
the global lease owned until its worker process exits. Only actual mutations and
paired cleanup enter the cancellation gate; waiting and read-only readiness do
not fabricate a cleanup failure. Cancellation rejects new segments and lets an
admitted pair finish releasing. A forced termination cannot prove late remote
application effects were undone.

Windows capture retains the **visible, fully unobstructed window** admission
boundary. It renders the exact HWND with `PrintWindow/PW_RENDERFULLCONTENT` into
a zero-initialized GDI DIB. Maximized captures crop the raw physical window
rectangle to a proven contained DWM visible frame; retained regions and pixel
input use that same origin. Compositor fences and root-window hit checks
supplement the existing z-order and before/after identity/geometry checks.
Blank or unsupported native rendering fails explicitly. There is no desktop
BitBlt fallback: Win32 geometry and hit tests can already report maximized bounds
while desktop pixels still contain other applications during an animation.
This is independent of UIA, not a WGC or occluded/background surface promise.
Pixel clicks bind captured physical bounds and DPI and reject changed geometry
and foreign-process hit targets. Foreground input briefly attaches the worker's
message queue to the current foreground and target threads, detaches both before target input,
and observes one activation request for at most 200 ms. Unproven activation
dispatches no target keystrokes; failed detach preserves cleanup failure. The
upstream reserved-key foreground assistance is not transplanted.
X11 requires native local X11,
XInput 2, 32-bit little-endian TrueColor pixels and proven ownership from XRes (not merely
_NET_WM_PID); only characters present in the current keyboard map can be typed.
Only keyboard group zero is accepted for text; active alternate groups or
modifiers and unsupported characters are rejected before injection.

## Evidence and remaining gates

- Local macOS run: original shared IPC lifecycle suite, 20 passing tests;
  transplanted XRes ownership suite, 8 passing tests; shared input sequencing,
  held-prefix cleanup, mutation-gate and size/lock/full button-mask contracts, 11 passing tests.
  The input tests run directly with `rustc --edition 2021 --test
  cmd/dtw/src/backends/input_sequence.rs -D warnings -o /tmp/dtw-input-tests`,
  followed by `/tmp/dtw-input-tests`. They require no GUI or platform library.
- Both `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu` provider source and
  test code type-check against their actual target SDK crates. Reproduce with
  `python3 tests/native/check_provider_types.py --target TARGET`; it deliberately
  builds an isolated checker rather than cross-compiling QuickJS or CUA.
- Windows Unicode-pair/chord and cooperative-stop tests are present and type-checked,
  but were not executed on macOS. This is not a linked Windows/Linux product build.
- The original cross-platform inventory included no Windows/Linux GUI run.
  Follow-up Windows evidence is in
  [the 2026-10-31 acceptance record](https://github.com/caelis-labs/desktop-world/blob/main/docs/windows-rewrite-acceptance-2026-10-31.md).
  Final candidate construction, the broader normal-user Windows matrix,
  DPI/geometry, modal/focus races, cancellation/parent exit and independent effect
  readback remain required. Linux desktop combinations remain unverified and do
  not acquire a support commitment from successful compilation.

## Source notices

The full CUA MIT copyright/license is retained in
[native-source-provenance.md](native-source-provenance.md).
The pinned donor attributes the reused Windows groups to:

- trope-cua: Copyright (c) 2026 Victor Vannara.
- Interface-Agent: Copyright (c) 2024 Francesco Bonacci.

The donor's T3 Code Linux notice names Wayland/helper/SDK source groups, which are
not imported here. This scope record does not replace the candidate's dependency
license inventory. The applicable upstream MIT license text follows:

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
