# Protocol boundary acceptance after implementation

Implementation batch: `1d48b3570d0233cf9e77e878dbdc8858da6e4060`, followed by
the visible-pointer correction described below. No model/Harness A/B ran.

## Native and actual-application evidence

- The new two-tool lifecycle runner passed seven owned cases: automatic startup,
  semantic input/capture, quit prompt cancellation, normal exit/restart,
  LSUIElement discovery/input/readback, nested-helper filtering, and cancellation
  recovered by ordinary look without Session/result queries. Report:
  `/private/tmp/dtw-protocol-prep-lifecycle-20261102-a/report.json`.
- The initial native matrix passed 37 cases. After the real Chrome finding and
  its fix, the final matrix passed 38, including physical image clicks refusing
  background-only before input. Final report:
  `/private/tmp/dtw-protocol-prep-native-20261102-final/report.json`.
- A redacted, no-input/no-quit corpus read running Finder, Chrome and real
  Caelis Bot. Bot was discoverable with two windows, complete eight-element
  semantics and a valid composite PNG. WPS was running without windows and was
  recorded as such, not counted as a completed WPS task. This corpus used the
  pre-pointer-fix binary `7a1238bf…`; its exact hash is retained in the report:
  `/private/tmp/dtw-protocol-prep-real-readonly-20261102/report.json`.

## Actual Chrome translation popup: failure and correction

A signed Chrome app was cloned to an owned temporary bundle and launched with a
separate profile and a local Portuguese page. This isolates app-scoped captures
from the user's existing browser windows. Setup/independent observation/normal
cleanup belonged to the supervisor; all business input used public apps/exec,
without DOM/CDP, HTTP input or private DTW actions.

The genuine native translation popup and browser composed into one 1600×1105
image. A delayed first image click was refused without dispatch. A subsequent
fresh operation exposed a real failure in the PID-directed background click:
the popup disappeared and key-window restoration could no longer prove its
surface, producing unknown input and cleanup failure. No uncertain action was
replayed. The failed MCP exited 2 and its workers were reaped; Chrome then closed
normally. Original report/trace remain at
`/private/tmp/dtw-protocol-prep-chrome-20261102-b/`.

Image coordinates now select the visible physical-pointer path on macOS, like
move/drag. Semantic presses retain their background path. Background-only denies
physical image clicks before native input. This removes background key-window
preparation/restoration from the transient-popup click, while preserving native
ownership, geometry, hit visibility and paired-release checks.

On a fresh owned Chrome instance after the correction:

1. Captured the real native translation popup above its browser window.
2. Closed it through image coordinates; the returned delta removed that surface.
3. Clicked the local-page button semantically, independently observing count 1.
4. Clicked it through image coordinates, independently observing count 2.
5. MCP and both workers exited normally, with no forced cleanup; Chrome exited 0.

Evidence: `/private/tmp/dtw-protocol-prep-chrome-20261102-c/`, including original
wire frames, PNGs and independent count. Its four exec calls returned 2422,
5937, 1444 and 333 JSON bytes (image blocks separate), taking approximately
0.79, 1.80, 0.49 and 1.13 seconds in this one run. These are protocol observations,
not model token, speedup, task-completeness or cross-platform claims.

The first setup attempt used a `/private/tmp` query while discovery returned the
same bundle's `/tmp` identity. Broad name discovery returned both copies and
explicit-path execution reused the owned instance. The literal query mismatch is
retained in attempt `a`; it is not counted as a failed screenshot or fixed by
replaying input.

Final development binary SHA-256:
`688cd42df00a22061134ca307fef59f10f33660b3e601b5b646cffd54bb50224`.
Final Rust regression: 496 unit + 7 integration = 503 passed. The added worker
regression proves visible pointer operations decline the background path before
native preflight. The final owned native matrix uses this exact binary.

The lifecycle/readonly runs used the earlier implementation binary; fixed-source
candidate-byte checks are separate. Windows full build/GUI/DPI, clean installation,
upgrade, consumer-host migration and signing/notarization remain independent
acceptance boundaries. Source/provider checks do not close them.
