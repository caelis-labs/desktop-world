# Agent protocol convergence — local acceptance (2026-11-02)

This is the first implemented convergence, not a declaration of complete desktop
automation coverage. The [working plan](agent-protocol-convergence-plan.md) records
the approved scope; the installed [Skill API](../skills/desktop-world/API.md) is
the Agent reference. No real model/Harness A/B, publication or merge was performed.

## Implemented contract

| Boundary | Current behavior |
| --- | --- |
| Discovery | Only `apps` and `exec` are advertised. `apps` searches/paginates installed and running apps on macOS, including stopped bundles; it reports bounded search coverage. |
| Authorization | Every `exec` requires an app identity. There is no app-less lane or app switch inside a script. Duplicate installed bundle IDs use explicit bundle paths; running status matches that path. |
| Scripts | Seven methods: `look`, `capture`, `click`, `typeText`, `keypress`, `scroll`, `quit`. API documentation ships with the Skill. No public `api`, `result`, Session, native refs or window binding methods. |
| Observation | A first full baseline followed by compact changes. Changes compare with the previous returned tool view, so intermediate script reads cannot consume unseen changes. A new process resets the baseline. |
| Fallback | Unavailable semantics trigger a stacked app image. Proven no-dispatch semantic activation failures enable image fallback. Coordinates use the same z-order/alpha mapping as the image and recheck current geometry before input. |
| Lifecycle | macOS look starts a stopped app. Input focus/readiness is internal. Normal quit exposes pending save UI; it does not force termination or automatically restart. A running app without windows can receive a verified foreground shortcut. |
| Output | Current view, bounded print/return, essential dispatch/effect/error evidence. Explicit semantic look ends unnecessary visual follow-ups and retires the previous image binding. |

Retiring the public result tool is viable for the implemented bounded synchronous
MCP flow. Request retransmission/deduplication, cancellation, receipts and cleanup
remain internal. New request IDs are new work. Lost/unknown input does not permit
replaying a script: read the app before deciding on further action.

State remains inside the per-app runtime: scene baseline, native bindings and
execution ledger. JavaScript variables last for one execution. There is no
Agent-owned persistent Session or state commit/recovery protocol.

## Verification

- `cargo test --all-targets --quiet`: 492 unit tests and 7 integration tests passed.
- `TMPDIR=/private/tmp sh scripts/check-source.sh`: passed, including standalone
  core checkpoints, format/source checks and 294 Python tests across the selected
  suites. The package tests verify that API.md ships and only two tools appear.
- `run_native_inventory_tests.py`: native inventory, association, focus, geometry,
  input readiness, cancellation and cleanup contract suites passed. These pure
  controls are separate from GUI evidence.
- Skill frontmatter/local links were checked manually and packaging tests passed.
  The generic Skill validator could not run because its Python lacks PyYAML.
  Clippy is unavailable in the active toolchain; it was not counted as passed.

The final release-mode development binary has SHA-256:

```text
3ec36b241b4dd0cd432d58b02d81561edb59b983c562a3f458b6afa580d24a11
```

`tests/acceptance/app_scene_probe.py` passed 20 cases through the actual public
stdio tools on an owned disposable AppKit app. Independent AppKit state confirmed:

1. Exactly two tools; missing app and retired routes reject admission.
2. Installed stopped-app discovery and automatic startup.
3. Same-app stacked capture and clicks reaching the top overlapping window.
4. Floating popup capture/click and stale geometry rejection.
5. Normal window close and a visible save sheet after a quit request.
6. Coordinate interaction with the save sheet.
7. Closing the last window while the application remains running.
8. Background-only refusal of a windowless foreground shortcut.
9. Cmd+N creating a window through the existing keypress method.
10. Non-pressable text-field rejection returning an image without changing text.
11. Coordinate focus, Cmd+A and Unicode replacement with independent text readback.
12. Semantic look stopping images and rejecting old image coordinates.
13. Normal quit without automatic relaunch, followed by explicit restart with a
    verified new PID, `running:true` and a full reset baseline.

The run exited normally, reaped its native workers and required no forced cleanup.
Local evidence: `/private/tmp/dtw-scene-native-20261102-final/report.json`.
The probe source contains all 20 assertions; the list above groups related cases.

Real-app look/capture checks separately exercise currently running Finder, Chrome
and WPS. Payloads and images are redacted in the retained transcript. Finder's
unavailable AX semantics used image fallback; Chrome returned 80 elements with
explicit incomplete coverage; WPS returned 52 elements with complete coverage in
the observed window. All three produced explicit captures. No real-app mouse,
keyboard, editing or quit script was sent. These observations establish read
paths, not document/task completion or whole app-family support.
Local redacted evidence: `/private/tmp/dtw-scene-real-apps-20261102-final2/report.json`,
with the same binary hash and normally reaped native workers.

## Corrections found by native checks

- CoreGraphics `optionAll` is not a front-to-back ordering guarantee. Use the
  on-screen ordered list for visible layer order, retaining all-window membership
  separately. The old order could display and hit the wrong overlapping window.
- Independent SCK window filters can include child composition. Exact display-bound
  inclusion/cropping avoids treating a parent-plus-sheet image as one window.
- A real-app no-window observation exposed the missing app-shortcut route; it now
  uses exact process birth, foreground identity, policy and paired-release guards.
- Final review strengthened the restart assertion and fixed a surviving
  Coordinator returning a delta instead of a new process baseline.
- The disposable editor needed its normal Edit/Select All menu. Input dispatch was
  correctly unverified; independent readback caught append rather than replacement
  before that fixture was corrected. A successful script alone was insufficient.

## Remaining coverage and capability gaps

| Area | Boundary / next validation |
| --- | --- |
| Platform parity | Windows/Linux still report running-only discovery. macOS results do not establish stopped startup, windowless shortcuts, stacked capture or native GUI correctness there. |
| Associated/system UI | The existing ownership proofs remain in force. Actual system file dialogs, helper-hosted surfaces and Chrome's translation popup need separate task coverage; an owned popup is not proof of those app families. |
| Multiple displays | Pure negative-origin/scaling/alpha controls passed. Native cross-display spanning windows, full-screen/Spaces and display changes remain unverified; the independent-filter fallback still needs child-composition checks. |
| Mouse coverage | Hover, held buttons/keys and drag are not public. Clipboard and explicit window geometry controls are also absent. Keyboard/click workarounds do not prove complete computer-use coverage. |
| Semantic coverage | Only the top surface and bounded elements are returned. Improve viewport/control relevance and cover repeated/virtualized controls; truncation must never prove absence. |
| Failure recovery | Existing cancellation/unknown-effect/cleanup controls passed; actual permission revocation, stuck OS callbacks and host-reconnection integration still need dedicated native coverage. |
| Limits | 32 window metadata rows, 80 elements / 24 KiB semantics, 16 visible image surfaces, 1600-pixel maximum canvas dimension. Unsupported/partial reads stay explicit. |

The next capability work should prioritize those boundaries and a richer ordinary
mouse path, with independent readback, before claiming broad task completeness.
Protocol steps/output are structurally reduced; model tokens, speed and task
success improvements have not been measured.
