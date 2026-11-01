# Independent Agent cursor: isolated implementation and integration contract

Status: checkpoint 2 is implemented and accepted on signed macOS candidate V.
335 Rust tests, lightweight source/probe checks, native Swift controls and the
owned AppKit/Chrome GUI cases pass. See [acceptance evidence](https://github.com/caelis-labs/desktop-world/blob/f1d243a/docs/cursor-overlay-acceptance.md)
for exact source hashes, original failed samples and platform limits.

## Ownership and effect boundary

`OverlayOwner` owns one optional helper process per standard MCP/CLI session.
All application providers share its `Arc`; no per-app helper is created. The
helper is the same signed DTW executable with `--dtw-overlay-worker`, so it has
one AppKit main thread while the normal MCP and native-worker loops remain intact.
Its internal random session identity and visible stable color survive app changes.
There is no public cursor ID or extra model argument.

The parent owns a fresh helper-instance directory below `/private/tmp/dtw-overlays-<euid>`.
The logical session identity is separate from this instance and survives helper replacement.
Root and instance directories are 0700; event and identity sockets are 0600.
Existing `backends::process::Worker` owns the child, pipes, bounded calls, timeout
retirement, termination and reaping; this module does not duplicate supervision.
Native workers receive a private `OverlayEndpoint`.
They only report an actual posted pointer event and never dispatch input through
the overlay. `OverlayEmitter::posted` is nonblocking. Failure or loss of visual
presentation must not change an already dispatched input result or cause replay.
Keyboard input and AX semantic actions do not invent a pointer position. Chromium
primer and cleanup events must be excluded at the semantic event hook, even when
negative coordinates would otherwise be valid on a secondary display.

Use `OverlayOwner::begin_with_poll()` once per native pointer action, not once per script:
the session assigns a monotonically increasing operation across app providers and
helper replacements. One emitter gives that action's actual events an increasing
sequence. Reconstructing an emitter midway through the same action would reset
that sequence and is invalid. Startup retains the Worker before the first begin
performs its handshake. Cancellation or timeout retires that helper; a later
independent action can replace it only after confirmed cleanup. A PID/birth check
rejects exited and zombie helpers before the next begin. Fresh instance paths and
tokens prevent an old owner's Drop from deleting new sockets; increasing operation
ordinals prevent an old app's delayed hide from hiding the replacement. Unconfirmed
cleanup retains the owner and disables presentation; it never replays input.
Presentation diagnostics ignore stderr IO errors.

The control pipe has at most one outstanding command. Position datagrams are
validated and coalesced into one pending main-thread update; press sequence is
carried into later events so a coalesced release still displays the click pulse.
The pulse expires after 200 ms even if release feedback is dropped; the same
press sequence cannot renew it.
`hide(generation)` invalidates that exact action. A newer generation can start,
but canceled generations cannot restart or be revived by delayed events. A
control timeout closes admission and kills the visual helper; its late response
cannot be mistaken for a subsequent command. Parent EOF stops AppKit, owner Drop
uses the existing Worker stop/reap path (500 ms termination grace and bounded IO
thread cleanup). An atomic-only SIGTERM handler and the parent-death/EOF watchdog
schedule AppKit teardown, with a 400 ms process-exit fallback inside Worker's
termination grace. The helper owns presentation only; exit removes its windows. No input/focus lease is held by this helper.

## Integration contract

1. `backends/macos/mod.rs` exposes the narrow overlay worker entry.
   `main.rs` routes `--dtw-overlay-worker` before ordinary CLI/native-worker
   dispatch. `build.rs` compiles `Overlay.swift` with the existing Swift bridges
   and tracks its changes. It needs only the already
   linked AppKit/CoreGraphics/Foundation runtime; no CUA renderer crate is added.
2. The standard session's native factory shares one lazy cursor session with
   each application provider. Catalog discovery does not start an overlay. Do not close the owner in
   `close_catalog()`: that method is also used for refresh and cancellation.
   The session's terminal close/EOF owns `OverlayOwner::close()`.
3. Before a validated native pointer action is dispatched, `begin_with_poll()`
   supplies its endpoint only to the internal native request. Preserve
   the generation in a cancel/cleanup guard. Cancellation must first prevent new
   native admissions, then `hide()` the in-flight generation even if native
   cleanup fails. A provider shutdown hides its current action; it must not close
   the owner shared with other applications. `spawn_with_poll()` and
   `begin_with_poll()` borrow the caller's remaining/cancellation function and cap
   it with the existing startup/control budgets. Recheck caller cancellation
   before admitting native input even if optional visual preparation failed.
   Never forward this endpoint, token
   or internal generation to MCP output or model-visible script parameters.
4. The native worker creates one emitter from that endpoint. The donor directed
   path hooks after successful `send_event` in `cua_input/control.rs`; the Swift
   foreground/scene path reports actual successful posts through its callback.
   The callback must carry the exact validated PID, kernel birth, CGWindowID,
   full window bounds, final global screen point, actual route and event kind.
   Send only actual pointer motion/down/up/scroll, suppressing primer, emergency
   cleanup and events with uncertain posting. Validate this integration before
   adding any event to the overlay. Do not wait for cursor animation or renderer
   readiness from the input path.
5. The client combines `identity_candidate()` (the owner's nonblocking
   authenticated bootstrap snapshot) with `discover_candidates(caller_deadline)`
   for peers. Scene performs the live checks below and uses the same exact
   exclusion rule for initial and final visibility proofs. Independent window capture
   may already omit the overlay; it still needs pixel/AX and stacked-window GUI
   acceptance. A display screenshot must explicitly exclude proven owned IDs;
   `showsCursor=false` only controls the user's system cursor.

`begin_with_poll()` is visual preparation, not input permission. If it fails, record visual
unavailability internally and continue the authorized input route without an
overlay. The visual helper must never start before native target validation or
from a model-supplied arbitrary process/window identity.

## Window and exclusion proof

The initialization handshake returns the exact helper PID, kernel birth stamp
and one native window ID. `identity_candidate()` does no IO and does not wait on
a contended state lock. This cached identity was authenticated by the owned
Worker bootstrap, unlike peer directory entries. It is only a candidate for a
subsequent live exclusion proof. The production rule lives in `Scene.swift`:
exact window ID, WindowServer's current owner PID, and matching kernel birth
reads before and after the window-owner read.
Missing or changed evidence means no exemption. Exempting every window of a PID,
a window level, title, app bundle or executable is forbidden.

Peer discovery uses each helper instance's `identity` Unix stream endpoint. Directory
entries only locate sockets: stale directories, regular JSON files, symlinks and
wrong owner/mode entries grant nothing. Each connection is nonblocking and bound
by an absolute deadline; discovery caps total work at 40 ms and each peer at
20 ms without an arbitrary session-count limit. Missing/slow peers are omitted.

The client reads the kernel's `LOCAL_PEERPID` and `getpeereid`, requires the current
UID, and compares the peer's canonical `proc_pidpath` to the calling DTW binary's
exact canonical executable path. A fresh random nonce must be echoed with the
helper's stable logical session and immutable process/window identity. Executable checks are bracketed by kernel birth
reads; the response PID/birth must match that socket peer, and exactly one
positive window ID is accepted. The helper publishes this ID directly from its
own AppKit window during startup; requests cannot nominate another window. It
stops publishing on termination. PID reuse, an executable at another path (even
with the same basename), a changed process, a missing proof, or a stale response
produces no candidate. This internal ownership proof does not implement user or
Harness authorization.

After discovery, Scene must independently verify the exact window owner and the
helper's birth before/after its WindowServer read. A proven helper does not exempt
any of its other windows. GUI acceptance must still demonstrate that independent
sessions' overlays are excluded while unrelated windows remain blockers.

The AppKit panel is input-transparent, nonactivating and at ordinary level. It is
ordered immediately above the exact target, below unrelated higher windows; it
never becomes always-on-top. Missing target ordering is not replaced by global
`orderFrontRegardless`. Actual target PID/birth, on-screen presence and bounds are
checked at receipt, again after ordering, and every 50 ms. A close, move, resize,
Space disappearance or changed display arrangement ends the current marker operation; only a new
validated pointer action can show it again. This is sampled observation, not a claim of atomic WindowServer
state through every animation frame.

Global CG points are converted against the actual primary display's AppKit frame,
not `NSScreen.main`. Secondary displays can have negative origins or sit above or
below the primary. AppKit supplies vector rasterization at each display's backing
scale; input coordinates are never multiplied by that scale. Display layout or
scale changes hide the old marker.

## Fixed upstream source mapping

Upstream: <https://github.com/trycua/cua/tree/b38360443960a6540685a9a5eb5407f10fe3d03b>.
All paths below are relative to `libs/cua-driver/rust/crates/` at that exact commit.
The MIT license is retained at
`cmd/dtw/src/backends/macos/cua_input/LICENSE-CUA` (Copyright 2025 Cua AI, Inc.).

| Donor source | Ported block | SHA-256 of source file |
| --- | --- | --- |
| `platform-macos/src/cursor/overlay.rs` | `run_appkit` transparent/nonactivating ordinary-level window flags; `dispatch_pin_above` target-relative ordering | `53c3e42618e40a66c588d2584865b45b6a3e456ff8a2a8158bf9db9a47821400` |
| `cursor-overlay/src/theme.rs` | `SESSION_CURSOR_FILLS`, `stable_session_index` | `08e04ecab19be1d9cdf5beaa2564e9855457f3b50dd2b7d38e9487214b51bc77` |
| `cursor-overlay/assets/build_default_theme.py` | Eight cubic `DART_OUTLINE` segments, `HOTSPOT`, 20-degree rotation | `caa536a26215e51d0d1cc9e05f632ffc9f2b1a60b696cbe497af20c0f49d0b1c` |

The renderer, motion engine, cursor map, font assets and CUA session/runtime are
not imported. The window is a small vector panel instead of a full-screen bitmap;
DTW adds operation generations, the one-slot mailbox, exact identity checks and
owner cleanup. The donor's primary-screen-only guard, large command history,
8-second animation arrival wait and missing-target top-of-desktop fallback are
not carried over. The session identity remains internal. User-visible markers use color, arrow geometry and click feedback without a textual or hexadecimal badge.

## Required acceptance after wiring

- Verify actual directed click/scroll markers and route agree with event journals;
  no marker appears for an undispatched/strictly rejected/semantic action or primer.
- Move the user's real cursor while the Agent clicks a background fixture: physical
  cursor stays independent, foreground remains owned by the user, marker shows the
  actual target point and click pulse, and an unrelated covering window stays above.
- Exercise two app providers in one MCP session, then two independent MCP sessions;
  colors/identities stay stable, peer exclusion is exact and unauthorized fixtures
  remain blockers. The socket proof controls do not substitute for this GUI gate.
- Test cancellation between post and render, late old datagrams after cancellation,
  provider close, session EOF, parent SIGKILL and helper exit. No stale marker may
  return; the exact helper process/window must disappear without input replay.
- Move/resize/close target windows and change Spaces/displays during idle and event
  floods. Test screens left/above/below the primary and mixed backing scales.
- Compare screenshots with/without the actual visible overlay. Only proved owned
  overlay windows disappear from Agent-facing captures; unrelated windows and the
  output seen by external screen recorders retain their normal visibility.
