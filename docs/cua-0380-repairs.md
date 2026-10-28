# CUA 0.38.0 repairs and real-machine acceptance

> Historical evidence: this report applies only to its named source revision, binary and entry point. The current product uses [standard MCP and explicit app IDs](app-protocol-v1.md); any grant, manifest, SDK/worker or custom Host setup below is superseded and is not a current installation instruction. No acceptance result here is claimed to have been rerun for the new entry.

Checked 2026-10-25 on macOS arm64. Upstream remains the exact released component
`cua-driver-rs-v0.38.0`, commit `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`.
This follows the [pre-repair audit](cua-0380-usability.md). No production install,
public release, signing/notarization, Windows GUI or Linux GUI acceptance is claimed.
The later [native authorization acceptance](cua-native-authorization.md) supersedes
the profile/download integration gaps and final3 transport provenance below.

## DTW changes

1. **SCK click delivery:** position and recouple the real cursor, revalidate window
   geometry/visible stack/cursor, set clickState=1, and separate down/up timing.
   The AppKit fixture's own counter and native event log now prove one action.
   No automatic activation, retries or fallback; receipts still require effect readback.
2. **Exact popup screenshots:** all `operation=app` frames now use independent
   single-window SCK capture with `includeChildWindows=false`, explicit output
   rectangle and content/output geometry checks. This avoids CUA's incorrectly
   composed parent-plus-sheet frames. The app route now consistently uses visible
   point input and moves the real cursor. Native script captures remain CUA-owned.
   Exact app capture requires macOS 14.2+; the core runtime still targets 14.0.
   Newly appearing windows can precede SCK shareability; those window/inventory
   transitions get up to two additional whole read-only attempts at 150 ms
   intervals. Permissions, helper timeouts and invalid image geometry fail directly.
3. **Native capability bridge:** expose all ten typed browser operations plus
   `health_report` and `get_guide`. `describe` obtains native schemas/annotations
   from the exact worker. Arguments, results, images, continuation metadata and
   authorization stay in the same session. Private authorization/session fields
   are rejected before dispatch; no generic tool escape hatch is added.
4. **Refusal receipts:** a native `effect:refused` or `status:refused` becomes a
   failed DTW job with its native code even when upstream sets MCP isError=false.
   Complete native data remains available. A successful diagnostic can report
   degraded checks without being misclassified as an execution failure.
5. **Idle browser cleanup:** real use exposed a race where DTW killed its worker
   before native session cleanup, leaving one owned isolated Chrome alive. DTW now
   closes admission under the execution lock and allows up to two seconds for
   an idle native session to end before stopping the worker. In-flight cancellation
   keeps the fast stop path. Three consecutive isolated-launch/idle-EOF checks
   verify browser exit. Forced crashes/interruption still do not guarantee graceful
   profile/browser cleanup; that is separate from ordinary idle session shutdown.

## Real-machine evidence

Inputs used DTW MCP. The browser fixture used a session-owned isolated Chrome,
localhost HTTP server and task-owned files. Page POST events independently record
effects. Browser processes from completed probe sessions were verified gone.
The pre-existing test profile used for the popup is a separate task-owned profile.

| Case | Actual effect / result |
| --- | --- |
| Raised AppKit window | Counter +1; down/up events each clickCount=1. Previously the SCK path returned completed without changing the counter. |
| Ordinary AppKit window | Counter +1 using `backend:screencapturekit`, through the same point contract. |
| Observation safety | Reuse, moved/closed window, intervening script, opaque unrelated cover and 30-second expiry all refused; no extra fixture clicks. |
| Chrome OS file picker | The corrected 1600×474 image shows the whole 1512×448-point picker at the expected aspect ratio, without scaled parent content or black padding. One image-bound Cancel click closed it. Immediate CUA listWindows briefly still returned the old sheet; later independent SCK inventory confirmed removal without another click. |
| Browser prepare/bind/navigation | Fresh isolated profile; exact native window binding; opaque target/tab; localhost navigation. |
| Browser snapshot | semantic_v2 action/content refs plus retained CDP viewport PNG and pixel-to-CSS scale metadata. |
| Type and DOM click | Exact `DTW-中文-0380` input event and one Apply event. |
| Select | Independent change event with `value:green`. |
| Trusted pointer | Hover, right click, double click, HTML drag/drop and scroll produced independent page events with explicit foreground delivery. |
| Stale browser ref | Failed receipt with `browser_ref_stale`; no Apply event from the refused attempt. |
| Upload | `browser_set_input_files` assigned only the test file; page change event names `upload.txt`. |
| JavaScript prompt | Pre-registering `browser_dialog inspect` allowed inspection and acceptance of the exact prompt generation; page recorded the answer. The triggering click still timed out, so the overall interaction remains unreliable/slow. Without preregistration, first inspect also timed out. |
| Download | Refused with `browser_consent_required`; no download completed. This is an approval-channel integration gap, not a missing download implementation. |
| Diagnostics | Full health checks and structured data available. This CLI reports unbundled identity/WindowServer degradation while AX and Screen Recording checks pass. Capture readiness is intentionally not probed by read-only health. |
| Idle browser lifecycle | Three consecutive newly created isolated Chromium processes exited on MCP EOF after the shutdown-order repair. The earlier leaked test process was explicitly cleaned up after checking its exact PID and unique task-owned profile. |

Raw local evidence:

- `/private/tmp/dtw-cua-repair-live/scene-1-ready`: first SCK click fix, prior mixed capture path.
- `/private/tmp/dtw-cua-repair-live/scene-2`: unified SCK scene/click and refusal checks.
- `/private/tmp/dtw-cua-repair-live2/session/001–007`: repaired picker PNG, click and post-click inventories.
- `/private/tmp/dtw-cua-browser-native-1`: discovered upstream isError=false refusal.
- `/private/tmp/dtw-cua-browser-native-2`: synchronous prompt failure without preregistration.
- `/private/tmp/dtw-cua-browser-native-3`: successful page effects/upload/dialog recovery, download refusal.
- `/private/tmp/dtw-cua-browser-final`: packaged browser bridge effects and limitations; its exact adapter is recorded below.
- `/private/tmp/dtw-cua-popup-final2`: final capture path, occlusion refusal, two Cancel effects and immediate post-open observation.
- `/private/tmp/dtw-cua-scene-final2-evidence`: complete SCK point/guard regression after transition handling.
- `/private/tmp/dtw-cua-browser-lifecycle`: isolated browser idle-EOF regression.

Reproducible probes are `tests/acceptance/scene_fixture_probe.py` and
`browser_native_probe.py`. The latter is explicitly opt-in, launches an isolated
browser, writes evidence to a new directory and closes its MCP session afterward.
It records refused/unverified gestures separately; a finished probe is not proof
that every browser operation succeeded. The read-only `native_tools_probe.py`
checks all 12 schemas, reserved-field rejection, native error codes, health and
guide forwarding and is included in CI/release contract gates.
`browser_lifecycle_probe.py` is an opt-in real macOS process-cleanup test.

## Remaining gaps, with ownership

**DTW/private-worker integration at this checkpoint** (profile/download entries
are superseded by [the completed authorization integration](cua-native-authorization.md)):

- Existing user-profile attachment needs a native launch grant or authorization
  host. This DTW private-worker path does not yet expose a supported grant/host
  callback. Isolated browser launch is verified; personal-profile attachment is
  not claimed. The upstream embedded host API has an authorization adapter, but
  changing runtime architecture is a separate integration task.
- `browser_download` additionally requires trusted native MCP host approval
  evidence. The ordinary bound SDK call strips it; the later integration uses
  the SDK's dedicated trusted adapter ingress for all session calls. DTW refuses caller
  attempts to forge `_cua_browser_download_mcp_host_approved`. A real host approval
  channel is needed; exposing an agent-writable boolean would not fix this.
- Application-observation clicks still support one visible left click only.
  Richer gestures remain available through CUA scripts or typed browser tools.

**Unresolved in the pinned upstream CUA implementation/provider:**

- Native popup capture can miscompose parent/sheet pixels while declaring frame
  validity. DTW's app route now avoids it; `app.getState` remains affected.
- Some background double-click and drag routes return delivery without application
  effect. Explicit foreground routes worked on the tested HTML fixture; universal
  background effect is not established.
- A synchronous JavaScript prompt can block `Input.dispatchMouseEvent` (20-second
  timeout) and focus cleanup. First `Page.enable` after it opens can also time out.
  Pre-registering dialog events enables recovery but does not fix the click delay.
  The native refusal advises retry despite uncertain input effect; do not follow
  that advice without checking the actual dialog/UI.
- Native picker AX control discovery and foreground Escape remained unreliable in
  the baseline. This work fixes the screenshot/click path, not the provider tree.
- Firefox/Safari typed browser routes are not implemented by the current Chromium
  CDP engine; source reports route unavailable rather than supplying equivalent
  WebDriver BiDi/WebKit adapters. No real Firefox/Safari acceptance was performed.
- Primary-display-only desktop capture, partial helper/modal attribution and
  provider-dependent AX/UIA behavior remain. This work makes no multi-display claim.

Already implemented upstream but **not in this release pin**: the first audit
identified main-branch fixes #5712 (Space-free macOS enumeration) and #5680
(bundled-host diagnostic attribution). They are not counted as unimplemented,
and the unbundled CLI health result here is not proof of the bundled-host bug.

## Delivered local candidates and validation scope

The final standalone directory is `/private/tmp/dtw-cua-repair-final3`; the slim
Plugin is `/private/tmp/dtw-cua-repair-final3-slim`. Both have 712 independently
verified checksum entries and valid local ad hoc signatures. No public release
or notarization was performed.

| Final artifact | SHA-256 |
| --- | --- |
| Standalone `bin/dtw` | `1b1e36c31dab710fa218a34ea66566b78dba1c892ce87623943706e1e56d22a5` |
| Slim `bin/dtw` | `a467162352606a71e81d98223d468ab56a0de5b6b18875ed2ff0d1c5ed40f4a6` |
| Packaged worker (both) | `f1463aad7bda8f236003ecb1d8d20c7da7c2c2d4f72127bf7c258f37427f1014` |

Final3 passed local MCP smoke, text/structured receipt contract, 12 native tool
schemas/authority checks, runtime isolation, busy cancellation/EOF/signal
lifecycle, three real isolated-browser idle-EOF runs, independent installation
in paths with spaces/Chinese, and 11 Rust unit tests. Earlier in this same change,
70 pinned-prelude checks, strict TypeScript authoring checks, two sets of four
Python tests, click-contract checks and native helper parent-death checks passed.
CI/release gates were updated locally; remote CI was not run.

Exact GUI provenance is retained rather than treating every package as identical:

- Browser form/gesture/upload/prompt/download-refusal evidence in
  `/private/tmp/dtw-cua-browser-final` used slim adapter
  `fabce810f942d91795af733da00c757ca9cb70764eb4466a04c91f4642e15d2f`.
  The browser bridge and worker are unchanged in final3; later changes concern
  SCK transition reads and idle shutdown cleanup.
- Final SCK popup and scene evidence used final2 slim adapter
  `513cdbf8681c2e956ad0fd87fa2f38e7ea54544cfa8d56c49a9cc9275268a576`.
  Its SCK capture/input code is unchanged in final3.
- `/private/tmp/dtw-cua-browser-lifecycle-final3` used the delivered final3 slim
  adapter and proves the last shutdown-order repair on real isolated Chrome.
- Complete local build/check logs and provenance are in
  `/private/tmp/dtw-cua-repair-build` and `/private/tmp/dtw-cua-repair-checks`.

All these are macOS checks. Windows/Linux browser input and OS GUI behavior
remain unvalidated. Test windows, MCP sessions and owned browser processes were
closed; test evidence and the local candidate packages were retained.
