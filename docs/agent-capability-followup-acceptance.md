# Capability follow-up — local acceptance (2026-11-02)

Implements the [follow-up plan](agent-capability-followup-plan.md) from `cbb9e07`.
This is development/native protocol evidence, not a release or real model/Harness
A/B. The two public tools remain `apps` and mandatory-app `exec`.

## Added capabilities

The installed [API](../skills/desktop-world/API.md) now exposes nine app methods.
Two additions use current screenshot pixels:

```javascript
await app.move({x:120,y:180});
await app.drag({path:[{x:120,y:180},{x:360,y:240}]});
```

The runtime validates the bounded full path, its app/window ownership and alpha,
then consumes its capture binding. Native macOS input verifies foreground,
identity, geometry and visibility, supplies absolute and relative motion, and
pairs every admitted press with a release on all exits. No button state persists
between calls. Background-only policy refuses these physical gestures. Other
providers reject unsupported gestures explicitly.

Capture/read-only recovery now permits one bounded resample when a captured
surface disappears during a dialog transition. This retries reads only.

## Verified behavior

Final development binary SHA-256:

```text
9c014dc2ef87bb0ea066bc4badcfb04f312f887069cc255cf9539aae9f017867
```

`tests/acceptance/app_scene_probe.py` completed 37 native cases over ordinary
stdio `apps`/`exec`. Evidence is retained at
`/private/tmp/dtw-capabilities-native-j/report.json` and `mcp.jsonl`. The native
workers exited/reaped normally with no forced cleanup.

| Area | Independent evidence |
| --- | --- |
| Hover and drag | AppKit received a mouse move, one down, intermediate dragged events with nonzero relative motion, one up and the requested endpoint. |
| Ordinary control | A standard NSSlider changed above 90 through the public drag method; raw event delivery alone was insufficient. |
| Interruption | A new opaque window appeared during a drag. The sequence stopped, released once and did not replay. |
| Cancellation | MCP cancellation during a held drag produced one paired release. A new ordinary capture recovered with a fresh baseline, without Session operations or waiting for the cancelled request's response. |
| Admission | Empty/cross-window paths refuse before a down; background-only pointer movement refuses before physical input. Core tests preserve a binding after invalid arguments and consume it once on admitted input. |
| System Open | The genuine NSOpenPanel was captured, cancelled, then used with Go to Folder/path input to select the exact owned temporary file. |
| System Save | Genuine NSSavePanel confirmation created an owned file whose contents were read independently. Existing-file confirmation was observed and cancelled without overwriting it. |
| Display geometry | Primary and secondary physical screens were tested separately, including exact requested-vs-actual positions, capture, endpoint readback and stale-frame refusal after relocation. Both screens use 2× backing scale. |
| Existing lifecycle | Popup/overlap capture, stale geometry, save sheets, no-window shortcuts, Unicode input, normal quit/no relaunch and explicit restart retained their checks. |

The final Rust suite has 493 unit and 7 integration tests. Required source/core,
format and packaging checks pass, including 294 selected Python tests. Native
inventory/focus/ownership/cleanup controls pass, with new pure controls for
spanning displays, mirrored bounds, negative origins and offscreen points.
These controls do not establish real mixed-scale or negative-Quartz-origin GUI
behavior. Clippy remains unavailable in the active toolchain.

## Findings that changed the acceptance boundary

1. **Requested placement is not actual placement.** Initial spanning checks
   lacked the latter assertion. Adding it found automatic OS relocation. The
   owned fixture permits a spanning frame, and subsequent checks inspect actual
   geometry; an unestablished placement is never counted as a passing drag.
2. **A spanning rectangle/image is insufficient physical hit proof.** With an
   actual spanning frame, visibility proof failed and AppKit did not establish
   the expected event sequence. The existing rectangle/independent-image proof
   cannot establish which display/Space renders that portion. Physical pointer
   input now refuses before movement/down with `display_proof_unavailable` when
   a window intersects multiple displays. Capture remains available. This is an
   explicit unsupported boundary, not a passing cross-display drag.
3. **Modal keyboard focus can remain on the underlying filename field.** The
   overwrite dialog's keyboard destination guard correctly refused an unproven
   target. The test uses its observed Cancel button instead; that guard was not
   relaxed.
4. **AXPress may report uncertainty while closing its target.** Cancelling the
   nested dialog can return AX cannot-complete after the UI has changed. The
   probe preserves unknown dispatch, performs fresh reads, verifies the underlying
   Save panel, and continues without repeating the press. Script success was not
   substituted for task evidence.
5. **Nonexistent file URLs need parent canonicalization.** The owned fixture's
   file-write guard initially treated `/tmp` and `/private/tmp` differently.
   Resolving the existing parent before appending the filename fixed the fixture
   guard; successful dialog confirmation alone was not counted as saving.

## Remaining work

- Establish exact physical hit ownership for spanning windows/Spaces before
  lifting the explicit refusal. Do not replace it with an approximate rectangle
  or automatically move the app to make a failed input succeed.
- Cross-window/app drag and drop, modifier/right-button drag, clipboard and
  window geometry controls remain outside this interface. One content gesture
  does not prove the OS drag-and-drop data-transfer subsystem or browser HTML5 DnD.
- Actual mixed-scale, negative Quartz origins, display hotplug, full-screen and
  additional helper app families remain independent acceptance conditions.
- Genuine Open/Save panels are covered; this does not prove every service-hosted
  surface or Chrome's translation popup. Windows/Linux gesture parity is absent.

No token/latency/task-success gains have been measured. The public protocol stays
small while unsupported cases remain explicit.
