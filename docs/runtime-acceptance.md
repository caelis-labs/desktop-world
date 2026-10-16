# Runtime-owned sessions and application observation acceptance

Checkpoint `f07a4d5` evidence is retained below. The subsequent multi-instance fix and RC.9 validation are recorded separately in [RC.9 acceptance](rc9-acceptance.md).

Date: 2026-10-16. This is an uncommitted independent-worktree candidate, **not a
published release or an unconditional stability pass**. Runtime ownership,
application observation and guarded clicks are implemented. The exact package
passes the warm GUI flows and contract gates below, but fresh-process SCK capture
still fails intermittently. That is a release-promotion blocker.

## Exact candidate and dependency boundary

- Worktree: `/Users/xueyongzhi/.codex/worktrees/dtw-associated-capture-poc/desktop-world`
- Branch: `codex/dtw-associated-capture-poc`; base `10d6c056222fd0756f87493a8db62761e34331e7`.
- Final package: `/private/tmp/dtw-runtime-reviewed` (local ad hoc signing).
- DTW SHA-256: `85a02deeea6f3d1a22434dd0e96004c669477c5af0dcac013a4d95f09c9bd60b`.
- Packaged worker SHA-256: `15a2abacf6a856978de4b2c3e8463c79bc3c502321d4169f4bcf38d8e44ba043`.
- Official CUA component 0.35.4 prerelease revision: `d74a9cab3fb2e80ab7f0cef47025ec828586ea75`.
  SDK and worker use the same revision. `/private/tmp/dtw-top2-clean-release`
  remains clean. Re-signing changes worker bytes; CUA source/kernel is unchanged.
- Native machine: macOS 27.0.1 (26A434), Apple Silicon. The candidate requires macOS 14+.

The [machine-readable record](../poc/associated-capture/runtime-acceptance.json)
contains source and artifact hashes. Package building used Rust 1.98.0; local
unit/format checks used Rust 1.97.1. No commit, push, merge or release was made.
The primary checkout and its preexisting `bin/` and `dtw.exe` remain untouched.

## Passed checks on the final package

| Evidence class | Check | Observed result |
| --- | --- | --- |
| Unit/source | Rust tests, formatting, Python syntax, Skill validation | 5 Rust tests pass; geometry mapping, window identity/order and context age covered; formatting and Skill checks pass. |
| Protocol | MCP smoke and contract probe | One `exec`; implicit state persists; legacy `session` fails; concurrent execution refused; receipt paging/error/cancellation reconciliation retained. |
| Process isolation | `runtime_probe.py` | Same process counters 1,2; separate process 1; foreign receipt denied; legacy request has no state effect. |
| Packaging | Independent install and license checks | Unicode/space paths and runtime-only installation pass; 373 dependency packages, 679 notice files. |
| Real WPS GUI | Final Runtime calls 001–003 | Floating new-document surface captured separately from its main window; screenshot-bound click dismisses it and opens the text-document template page. Result verified in the next screenshot. |
| Real Obsidian GUI | Calls 009–017 | Chooser plus main window captured; dropdown adds a third window in a different service PID. Top2 images show menu and chooser, inventory retains all three. Menu click changes `tmp` to `private`; Cancel closes chooser and leaves one main window. |
| Real AppKit fixture, existing Runtime | `scene_fixture_probe.py` existing-process mode | All 3 windows inventoried; default 2 / expanded 3 images. Both SCK visible-point and CUA ordinary-window clicks independently increment fixture counter exactly once. Reused, moved, closed, script-invalidated, opaque-covered and expired observations refuse input; counters unchanged. |
| Fresh process, WPS only | Three sequential `app_view_probe.py` runs | 3/3 reads pass for popup + main window. This small sample does not prove general cold-start reliability. |

GUI receipts, structured inventories and original PNGs:
`/private/tmp/dtw-runtime-reviewed-gui`. Fixture evidence:
`/private/tmp/dtw-runtime-reviewed-fixture-warm-evidence`.
The fixture's JSON counter is an independent effect readback; a `posted` receipt
alone is not considered a successful click. WPS/Obsidian effects were visually
read back through later screenshots; no document or vault was created or saved.

Obsidian used `/private/tmp/dtw-runtime-obsidian-profile` and PID 59194; the user's
existing PID 3167 was not controlled. WPS PID 59196 was launched for testing but
shares WPS account/profile state; template tabs were opened. This is not a fully
isolated live-account test. These GUI checks used DTW through the MCP harness;
they are not a fresh external Agent-host/Skill discovery acceptance run.

## Unresolved failure and promotion gate

A fresh final-package Obsidian read returned `capture_timeout`. A fresh fixture
Runtime failed three read attempts with the same code before any click. A second
fresh Runtime with an explicit worker path exceeded the harness polling window;
reconciling its original receipt `6529882e` returned `capture_timeout`. Evidence is
in `/private/tmp/dtw-runtime-reviewed-fixture-evidence` and
`/private/tmp/dtw-runtime-reviewed-fresh-harness`. The already running Runtime
captured the same fixture and completed the full guarded-click suite.

The failure is therefore **not resolved** by warm-flow passes or the three WPS
cold passes. Earlier development also observed SCK/XPC interruptions, but their
causal role is unproven. An experimental AppKit main-loop bootstrap did not fix
it and is absent from the final implementation. No retries of input were added.
Ordinary CUA screenshot reads have a three-second timeout; SCK callbacks have a
12-second wait per capture, so a complete discovery/capture may take longer than
12 seconds. Failure returns no usable new observation or partial-success claim.

Before promoting this dependency/package pair, reproduce and resolve fresh
process SCK initialization, then pass cold and warm WPS/Obsidian/fixture runs on
the same rebuilt artifact. Keep the previously shipped pair available meanwhile.

## Reproduction and remaining boundaries

```sh
candidate=/private/tmp/dtw-runtime-reviewed
export DTW_CUA_WORKER="$candidate/libexec/cua-driver"
python3 tests/acceptance/mcp_smoke.py "$candidate/bin/dtw"
python3 tests/acceptance/contract_probe.py "$candidate/bin/dtw"
python3 tests/acceptance/runtime_probe.py "$candidate/bin/dtw"
# Launch a fresh fixture directory, then test a fresh Runtime:
tests/fixtures/scene/build-run.sh /private/tmp/dtw-scene-new
python3 tests/acceptance/scene_fixture_probe.py "$candidate/bin/dtw" \
  /private/tmp/dtw-scene-new /private/tmp/dtw-scene-evidence-new
# An optional fourth argument is an existing scene_session.py server root.
# This explicitly tests an existing Runtime and must not be labeled cold-start.
```

Attribution uses public AX client/geometry evidence and is always best effort.
Top2 limits images, not inventory; inaccessible service windows, protected
surfaces, minimized windows and other Spaces cannot be universally guaranteed.
Unattributed occluders are metadata only; no automatic full-desktop broadening.
Geometry/stack checks reduce races but cannot make separate screenshots and input
atomic against another controller. Coordinate clicks still require effect
readback. A host must serialize controllers sharing the physical desktop.

Windows, mixed-DPI/multiple-display live clicks, public signing/notarization,
clean-machine permissions and a fresh external Agent-host run remain unvalidated
for this candidate. Earlier acceptance documents do not transfer those passes.
