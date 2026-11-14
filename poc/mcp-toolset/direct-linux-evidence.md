# Direct atomic Linux acceptance

This follow-up compares separate atomic MCP calls with separate script `exec`
calls on one MCP connection per round. The existing
[baseline evidence](linux-evidence.md) is preserved unchanged. **Final isolated
side-effect acceptance passed in script and atomic modes** after the parent fixed
the native window-layout contract. The first frozen candidate's coordinate-click
failure is preserved below. The earlier baseline binary is not follow-up
acceptance evidence.

The updated `linux_locked_probe.py` creates fresh Xvfb + KWin, private runtime,
configuration/cache/data and KWin D-Bus session, and a unique owned Tk fixture
for each round. The real `:0` is limited to lock sampling and X11 server/extension
metadata. There is no unlock, locker operation, real-display app discovery,
focus, input, pixel read or private app access.

Each round uses the following separate calls after scoped `apps` discovery:

1. `look`, `capture`, button coordinate `click`, `look`, `capture`.
2. Fresh `capture`, entry coordinate `click`, `type_text`, `keypress`, `look`,
   `capture`.

Script mode uses one `exec` per operation; atomic mode sends flat object arguments
with `app` and supplies no JavaScript source. All calls within a round use the
same live server and MCP connection. Each fresh fixture starts with count zero
and empty text. A button callback increments the visible counter and writes
independent fixture state; Return writes the exact entry text to visible ACK,
stdout and an atomic state file. Acceptance requires count exactly one after the
button and at final readback, exact entry and ACK text, changed PNG bytes for both
effects, and MCP process exit zero. Unknown or failed input is never replayed.

Coordinates come from actual Tk widget geometry and client dimensions. The probe
checks returned coordinate canvas bounds against the actual client root x/y
and dimensions (`winfo rootx/rooty/width/height .`),
checks PNG IHDR dimensions against MCP image metadata, then scales each widget
center into image pixels. It does not guess control positions. Each returned PNG
is retained with its call ID, dimensions, path and SHA-256; full child logs are
also retained. JSONL stdout records exact call arguments and structured receipts.

A fixture-only bootstrap rehearsal on 2026-11-14 used isolated display `:1` and
accepted exact child PID, unique canonical WM_CLASS and JSON geometry. It stopped
intentionally before DTW launch, with no input, and is not product acceptance.
The rehearsal used `/usr/bin/true` only for the isolated runner's version/hash
placeholder; no DTW candidate was run. Its owned leaders `[5, 8, 115]` were reaped
with statuses `[0, -15, -15]`; explicit session 7 lock samples were `yes` before
and after. Known working wish script-first launch, explicit matching Tcl/Tk
libraries and exact canonical script class checks are preserved.

Reproduction (artifact directory must be new):

```sh
python3 poc/mcp-toolset/linux_locked_probe.py --session 7 --isolated \
  --toolset both --binary /tmp/dtw-direct-layout-candidate-ebjgdaeg/dtw \
  --artifacts /tmp/dtw-direct-linux-layout-artifacts \
  > /tmp/dtw-direct-linux-layout.jsonl
```

Linux semantics are unsupported without AT-SPI in this backend. A successful
result here proves app-scoped screenshot and coordinate/input effects on the
owned isolated X11 fixture. It does not prove semantic desktop operation,
locked-session app operation, or another platform. Before/after `loginctl`
samples are not continuous monitoring.

## First frozen candidate: coordinate acceptance failed

Run time: **2026-11-14 10:07:02 +0800**. Probe exit: **1**.
Candidate: `/tmp/dtw-direct-candidate-gdlqdykf/dtw`, version `dtw 0.4.2`.
SHA-256 verified before execution and recorded inside the probe:

```text
0c38a44abf97cb256a1f887fbcc270b304554039c5a53edd79ce06f603fb921a
```

Raw evidence: `/tmp/dtw-direct-linux-final.jsonl`.
PNG and complete child logs: `/tmp/dtw-direct-linux-final-artifacts/`.
These files are preserved, including the first failure; no input was replayed.

| MCP call ID | Script round | Atomic round | Actual result |
| --- | --- | --- | --- |
| 1–2 | Initialize and list | Initialize and list | Successful, separate persistent connection per round |
| 3 | `apps` | `apps` | Exactly one matching unique owned executable |
| 4 | `exec` → `look()` | `look` | PNG; semantics `unsupported_capability`, zero elements, window bounds null |
| 5 | `exec` → `capture()` | `capture` | PNG 920×480, canvas bounds `[0,0,460,240]` |
| 6 | `exec` → `click({x:460,y:199})` | `click {app,x:460,y:199}` | Error `stale_scene`: “Canvas anchor geometry changed; inspect the returned view before new input” |

The button widget's measured client rectangle was `[162,82,136,35]` within a
460×240 client. Its center `(230,99.5)` therefore mapped to `(460,199)` in the
920×480 PNG, the same in both rounds. Capture and click occurred in separate MCP
calls on the same connection. All six PNGs exist and share SHA-256:

```text
48f10485f294611c16cc3ad02cdc800250a952ca0425cf86196ee8f072e32cb8
```

Representative inspectable PNGs:

- `/tmp/dtw-direct-linux-final-artifacts/script-05-exec-1.png`
- `/tmp/dtw-direct-linux-final-artifacts/script-06-exec-1.png`
- `/tmp/dtw-direct-linux-final-artifacts/atomic-05-capture-1.png`
- `/tmp/dtw-direct-linux-final-artifacts/atomic-06-click-1.png`

Visual inspection of the final atomic image shows an empty entry, `Count=0` and
`ACK pending`. Initial independent fixture-state readbacks in both rounds were
`{counter:0,entry:"",ack:""}`. This attempt stopped on the first failed click;
**counter-one proof, entry click, typing, Enter, exact ACK, post-input readback,
and clean zero MCP exit were not reached**. Failed MCP servers were terminated
by owned-child cleanup, rather than reported as successful shutdowns. The probe
now also retains independent fixture-state readback during final cleanup on
future runs; that additional state artifact was not present in this first run.

Read-only source inspection identified a concrete contract gap: the Linux
`native.rs` `windows` branch emits target, title and visibility but no bounds
(the `out.push` near line 466). The returned public `look` confirms
`windows[0].bounds:null`. The common scene coordinate guard in `scene.rs` near
lines 1312–1318 requires the fresh window row's bounds to equal the retained
capture anchor. Thus a correctly measured click is rejected before coordinate
input dispatch. Both script and atomic paths hit this same check. No native
backend or common scene code was changed by this probe task, and no guard was
bypassed. This failed candidate cannot support a side-effect acceptance claim.

The isolated display was `:1`; KWin's EWMH check returned window `0x20000c`.
Fixture PIDs were 116 and 215; exact unique copied executable, canonical script
WM_CLASS and child PID were checked. Both fixture and MCP stderr logs were empty.
Cleanup reaped owned leaders `[6,9,116,194,215,227]` with statuses
`[0,-15,-15,-15,-15,-15]`. The scratch directory
`/tmp/dtw-locked-probe-3pm7a_u9` and X1 socket were absent afterward; retained
artifacts remain outside scratch.

Explicit `loginctl show-session 7 -p Id -p Type -p Display -p LockedHint` returned
`Id=7`, `Type=x11`, `Display=:0`, `LockedHint=yes` before and after. The real
session remained outside the application-operation scope; no private app,
locker or real-display pixels were accessed. Lock samples establish the two
observed times, not continuous monitoring.

## Final frozen layout candidate: both modes passed

At **2026-11-14 10:11:22 +0800**, fresh script and atomic rounds completed with
probe exit **0**. Candidate `/tmp/dtw-direct-layout-candidate-ebjgdaeg/dtw`,
version `dtw 0.4.2`, SHA-256 verified before and during the run:

```text
8a2068fdf41480bc7752af62d5ecb26318ba9585a13f77696cc643e20e5fa93e
```

Raw per-call evidence: `/tmp/dtw-direct-linux-layout.jsonl`.
All **22 actual PNG files**, full child logs and independent final fixture-state
JSONs: `/tmp/dtw-direct-linux-layout-artifacts/`. These use new paths and do not
overwrite the failed candidate evidence. Each PNG is 920×480. The reproduction
command above is the exact successful command; choose new output paths when
rerunning so these existing artifacts remain preserved.

The parent fixed native window inventory to expose actual client root bounds
and frame stack ordering, retaining the common runtime guard. This run returned
window and image bounds `[50,78,460,240]`, matching independently measured Tk
root x/y and client width/height. The button rectangle remained
`[162,82,136,35]`, entry `[67,43,326,27]`. Scaling their centers by the actual
920/460 and 480/240 image ratios produced button pixel `(460,199)` and entry
pixel `(460,113)`; global coordinates were not passed as image coordinates.

Both rounds used exactly one MCP connection throughout, with unique fresh
fixtures and unique exact executable authorization identities. Script used
`apps` and separate `exec` calls; atomic used flat JSON object tools with no
JavaScript source. No call was retried or replayed.

| Call ID | Separate operation | Verified result in both modes |
| --- | --- | --- |
| 3 | Scoped `apps` | Exactly one matching owned running executable |
| 4–5 | `look`, then `capture` | Initial PNG, independent initial state count 0, empty entry/ACK |
| 6 | Button coordinate click | One dispatched action; independent callback `BUTTON=1` |
| 7–8 | `look`, then `capture` | Changed PNG; independent state `{counter:1,entry:"",ack:""}` |
| 9–10 | Fresh `capture`, then entry coordinate click | Correct image mapping; one dispatched click |
| 11 | `typeText` / `type_text` | One dispatched action with the round's exact token |
| 12 | `keypress` Enter | One dispatched action; exact fixture ACK |
| 13–14 | `look`, then `capture` | Changed PNG; final exact independent state and count still 1 |
| EOF | Close MCP stdin | Both MCP server exit statuses 0 |

Independent final state artifacts:

- `script-fixture-state.json`: `{"counter":1,"entry":"dtw_probe_script_731","ack":"dtw_probe_script_731"}`.
- `atomic-fixture-state.json`: `{"counter":1,"entry":"dtw_probe_atomic_731","ack":"dtw_probe_atomic_731"}`.

These are written from the fixture's own state file, independent of MCP
receipts. Stdout events were exactly `BUTTON=1` and
`ACK=dtw_probe_script_731` / `ACK=dtw_probe_atomic_731` respectively. The counter
was checked after the button click and again after typing/Enter, proving that
the single button click did not increment it twice.

Representative artifact paths, all under `/tmp/dtw-direct-linux-layout-artifacts/`:

| Stage | Script PNG | Atomic PNG |
| --- | --- | --- |
| Initial capture | `script-05-exec-1.png` | `atomic-05-capture-1.png` |
| After button, explicit capture | `script-08-exec-1.png` | `atomic-08-capture-1.png` |
| Final explicit capture | `script-14-exec-1.png` | `atomic-14-capture-1.png` |

Actual decoded PNG SHA-256 values:

| Stage | SHA-256 |
| --- | --- |
| Initial, both | `48f10485f294611c16cc3ad02cdc800250a952ca0425cf86196ee8f072e32cb8` |
| After button, both | `de3c7082ba14e14c9339c7c32e2a2c721e492bc49223eed6ec0d3824faa142c6` |
| Final script | `d9a2881173ba6984a2962d356cb7829f059e7805af06f08319f0e9cdae58fda0` |
| Final atomic | `5ccc65192769590bd19d3a64ae74a39738a6c4f443dbf1a793fb95226c13435c` |

Both final PNGs were visually inspected: each shows `Count=1`, its exact entry
text and exact visible ACK. Receipt status alone was not used as effect proof.

Xvfb again selected isolated `:1`. Owned fixture PIDs were 118 and 231, with
exact canonical unique classes and copied executable paths recorded in raw
JSONL. Fixture/MCP stderr logs were empty. Cleanup reaped owned leaders
`[6,9,118,194,231,243]` with statuses `[0,-15,-15,0,-15,0]`, including both
already-successful MCP servers. The scratch directory
`/tmp/dtw-locked-probe-qoqz5p9_` and X1 socket were absent afterward. Retained
PNG/state/log evidence remains outside that removed scratch directory.

Session 7 again returned `Id=7`, `Type=x11`, `Display=:0`, `LockedHint=yes`
before and after (`lock_check.before=true`, `after=true`). The probe performed
no real-display fixture mapping, app discovery, focus, input, capture, locker
operation or private-app access. These are lock samples, not continuous proof.
The successful result is limited to screenshot-based coordinates and text/key
input on owned isolated X11 fixtures. Native semantics still report
`unsupported_capability` with zero elements; no AT-SPI or real semantic desktop
success is claimed, and no unavailable platform was tested.
