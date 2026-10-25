# CUA 0.38.0 upgrade and usability audit

This is the **pre-repair baseline** from the first audit. The SCK repair and
native browser/diagnostic bridge are recorded in [the follow-up acceptance](cua-0380-repairs.md).
Capability gaps and candidate hashes below describe the baseline, not the repaired tree.

Checked 2026-10-25. The latest published **Driver component** is
[`cua-driver-rs-v0.38.0`](https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.38.0),
commit `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`. GitHub's prerelease flag is the
monorepo's release-label convention, not an additional promotion gate. DTW's
SDK, lockfile, worker builders, version output and notice inventory now use this
same revision. Historical acceptance records still describe their original builds.

This audit distinguishes source/contract checks, real macOS GUI effects, and
unvalidated platform behavior. A successful receipt alone is not a successful
application action. No installed production runtime is replaced by this work.

## What the wrapper does and does not expose

The script adapter wraps `click`, `rightClick`, and `doubleClick` argument
validation; other `App` methods still reach the upstream prelude. It is therefore
incorrect to describe all missing authoring declarations as missing runtime
capabilities. The more substantial omissions are native Driver tools outside
the upstream `run_script` allowlist: DTW exposes neither a native route nor a
script escape hatch for those tools.

| Finding | Owner / status | Useful follow-up |
| --- | --- | --- |
| Screenshot-bound double click (`click` with `count:2`), typing delay, scroll units, drag timing, and observation diagnostics were absent from the authoring reference. | Existing CUA capabilities already pass through DTW. Declarations, examples, and type contracts are completed in this change. | Use one bound gesture; do not emulate double click by reusing a consumed capture. |
| `get_browser_state`, `browser_prepare`, `browser_click/type/select/pointer/dialog/navigate`, file-input and download tools are absent. | Upstream implements them; DTW has no public route. This is a real capability gap. | Add an exact-tab browser route preserving endpoint/profile consent and upstream authorization. Keep native AX for browser chrome and OS dialogs. |
| Web typing receipts recommend `page` / browser tools that DTW cannot invoke. | Upstream guidance crosses the wrapper's available surface. The Skill now explains the limitation. | Translate escalation guidance into available DTW routes; implement the typed browser bridge. Legacy `page` mutation is disabled by default upstream, so simply forwarding it is insufficient. |
| `operation=app → click` supports only a single left click, with CUA frames fixed to background delivery. | DTW extension limitation, distinct from the richer script API. | Extend the observation-bound action contract deliberately; preserve one-use binding, geometry/occlusion checks, and no automatic input replay. |
| Scripts, app observations, and desktop snapshots use different bindings; any script invalidates an app observation. | DTW lifecycle design adds switching cost. | Return explicit available actions and route information with observations, and make invalidation reasons actionable. |
| Screenshots are retained but not included by default; only the first is inline. App observations may capture two or more windows. | DTW response policy. | Make observation images easier to discover and fetch by frame, with visible counts and dimensions. Preserve bounded payloads. |
| `health_report` is absent while `permissions` exposes only a narrower check. | Upstream has end-to-end capture/AX/WindowServer diagnostics; DTW does not expose them. | Add a read-only diagnostic route before expanding input behavior. |
| `set_window_frame`, clipboard, visual-region parsing, and recording are absent. | Native upstream tools, outside DTW's public surface. | Prioritize window geometry and optional visual perception; clipboard and recording need explicit user scope and resource lifecycle. Visual parsing also requires an optional perception backend. |
| `run_steps` is absent. | Deliberately excluded because its stale-token re-find/retry behavior differs from DTW's recovery contract. Scripts already cover bounded sequencing. | Consider action-plus-observation response ergonomics rather than exposing every native tool mechanically. |
| Full desktop capture is primary-display only on macOS. | Upstream `get_desktop_state` limitation; DTW did not discard a display selector. | Keep window-bound capture separate from claims about complete multi-display desktop support. |
| AX/UIA exposes incomplete or misleading information on some canvas, Electron, virtualized and modal surfaces. | Application/provider and upstream limitations remain. | Require screenshot/effect readback; do not equate AX value echo with renderer acceptance. |

The omission of daemon installation/update, arbitrary configuration, forced app
termination, session management and trajectory replay is not automatically a
usability defect. DTW owns its worker and session lifecycle. These operations
would need a concrete product use case and host policy rather than a generic
native-tool forwarding escape hatch.

## Changes since the previous pin

0.38.0 includes `.app` path launch handling, DOM click-effect classification,
window-only foreground hotkeys, and Windows foreground scroll/drag fixes.
The release's foreground **Document** typing fix targets an exact focused,
writable, empty Windows UIA Document. It is not evidence that macOS Electron or
nonempty rich-text input is now universally reliable. macOS 0.38 already supports
background drag on some controls; native drag-and-drop sessions can still require
foreground delivery. Absence of universal background drag is not absence of all
background drag support.

The checked upstream main was `14b22f552b3e13e7c633435c405a519159d822b0`, ahead of
this release. Relevant merged but unreleased work includes
[#5712](https://github.com/trycua/cua/pull/5712) (macOS background/window-change
enumeration avoids Space queries) and
[#5680](https://github.com/trycua/cua/pull/5680) (bundled-host diagnostic identity).
These must not be described as missing implementations, nor as fixes included
in the 0.38.0 pin. The former does not supply a measured timing improvement for
this machine.

## Evidence and acceptance

The official macOS arm64 release archive matches GitHub's asset digest:
`5f71191c082ca07c9dfc13b7c9a6eec60a82818c245a93534cc0eb7911471d72`.
The extracted worker SHA-256 is
`92067848d0eeb9de76929fbd06a137af801b0ff6caedbb3109bd7a6cc84a1f9f`.
Package assembly signs its own copy, which has a different digest. The SDK is
built from the exact same release source; the worker used here is the verified
official artifact, not a claim of locally reproducing its release build.

Local raw evidence is under `/private/tmp/dtw-cua-0380-upgrade-evidence` and
`/private/tmp/dtw-cua-0380-live`. The actual official-worker `list-tools` output
is retained as `upstream-tools.txt`. Source anchors are at the pinned revision:

- `cua-driver-core/src/script_tool/prelude.js`: native App methods/options.
- `cua-driver-core/src/script_tool.rs`: script tool allowlist and the native
  latest-two-images retention cap (not a DTW cap on application-extension frames).
- `platform-macos/src/tools/mod.rs`: native tool registration.
- `platform-macos/src/tools/type_text.rs`: web readback and escalation advice.
- `cua-driver-core/src/page.rs`: legacy mutation gate.
- `platform-macos/src/tools/get_desktop_state.rs`: primary-display contract.
- DTW `cmd/dtw/src/main.rs`, `click_contract.js`, and `app_view.rs`: forwarding,
  receipts, option adaptation, observation and click contracts.

## Real macOS results

GUI runs used the slim adapter SHA-256
`a90050da6fc231aeabdb1eaeeffe1f6c1bf5ecc8c94032ff325f40506acd1745`
and packaged worker
`f1463aad7bda8f236003ecb1d8d20c7da7c2c2d4f72127bf7c258f37427f1014`.
The final slim adapter and worker are byte-identical to those used for GUI;
only the accompanying documentation was refined. This is a local ad hoc signed
candidate, not a publicly notarized distribution.

The Chrome task used a new temporary profile and a localhost test page. UI
operations used DTW exclusively. The page independently sent input, apply,
double-click and drag/drop events to a local evidence collector. The AppKit
fixtures recorded their own native mouse events or button counter. No existing
browser profile or document was edited. The test apps and temporary MCP process
were stopped afterward; evidence remains on disk.

| Case | Result and independent evidence | Attribution / implication |
| --- | --- | --- |
| Native click contract | All seven cases passed: modifier array, raw scalar behavior, scalar refusal, bound right click, ordinary right click, double click, modified right click. Fourteen expected native mouse events, no event for the rejected case. | Validates this macOS fixture, not every application's input behavior. `dtw-cua-0380-click-fixture/native-report.json`. |
| Chrome name-only text selector | `name:"Message"` refused as ambiguous: it matched a static label, the field, and a menu item. No page input event. Adding the observed `AXTextField` role resolved it. | Upstream selector semantics plus overly terse authoring examples. `session/002–003`. |
| Chrome Chinese/Latin text and Apply | The page recorded the exact `DTW-0380-中文` text and one Apply event; UI heading matched. | Working native input. Nevertheless the action returns `effect:unverifiable` and advises unavailable `page`. `session/003–004`, `chrome-events.jsonl`. |
| Capture-bound background double click | Completed receipt, but no `dblclick` event and no UI change. | CUA background pointer delivery is not universally effective. `session/006`. |
| Capture-bound foreground double click | After a fresh observation and explicit route change, one page `double-click` event and the expected heading. | The existing `count:2` capability works on this route; no capture was reused. `session/007`. |
| Background HTML drag/drop | Completed receipt, no drag-start/drop event, unchanged heading. | CUA background gesture limitation on this surface. `session/008`. |
| Foreground HTML drag/drop | After fresh observation, page recorded one drag-start and one drop; heading became DROP. | Existing explicit foreground route works. `session/009`. |
| Chrome native file picker | PID observation exposed a sheet, but the 100-node tree budget was spent on its sidebar. `query` for Cancel returned no matches; exact picker binding plus a filtered read returned an empty tree. `operation=app` inventoried and retained both picker and parent images. | Modal discovery is partly working, actionable control discovery is not reliably smooth. The filtered read did not preserve all diagnostic fields, so the exact empty-tree cause is unresolved. `session/010–014`. |
| Picker image geometry | CUA reported a valid 1200×356 frame for the picker, but the image contained a scaled parent-plus-picker composition at the left and a large black area. No pixel input used this questionable frame. | Upstream image/geometry verification gap; DTW trusts that validity flag and has no additional detection. `session/012/image-0.png` and `native.json`. |
| Picker foreground Escape | Receipt completed with `effect:unverifiable`; subsequent window inventory and AX sheet showed the picker still open. | Not a successful cancel. The isolated Chrome process was terminated by its owning test harness for cleanup; no blind key replay or file selection. `session/015–016`. |
| DTW SCK top-window click | Three-window inventory and default-two/expanded-three captures worked. The first SCK button click completed but the fixture counter stayed 0. The existing scene acceptance script stopped at its first effect assertion. | **Failed GUI gate in DTW's own input route**, not a CUA input call. `scene-contract/21-observation.json`, `31-receipt.json`, `scene-contract.log`. Remaining move/occlusion/expiry assertions did not run and are not claimed as passed. |
| DTW ordinary-window click | A separately observed, visible ordinary window used the CUA route; independent counter changed 0→1. | Confirms route-specific behavior; it does not repair the failed SCK case. `scene-ordinary.json`, `session/021–022`. |

The sixteen Chrome executions include one explicit ambiguity refusal. Measured
local request times were approximately 0.5–3.8 seconds for these small cases;
these are harness timings including receipt/image retrieval, not a fresh Agent's
end-to-end task time or a performance benchmark. The main observed cost was
route selection, ambiguous/unavailable controls, and effect reconciliation.

### Why the two image/input findings matter

DTW forwards CUA images without compositing them (`app_view.rs:135–147`). In the
upstream macOS capture path, `px_frame.rs` verifies outer PNG dimensions against
WindowServer bounds at supported scales. Those checks cannot detect incorrectly
scaled or composed content *inside* an otherwise correctly sized PNG. The
picker observation is evidence of this contract gap, not proof of a particular
OS or capture-backend root cause. The DTW fallback currently triggers on reported
capture failure, so a falsely valid CUA frame bypasses it.

The failed SCK click follows DTW's `Scene.swift:196–202`, which posts immediate
HID down/up events. It does not call CUA's foreground mouse routine. The latter
warps/recouples the physical pointer, sets click state, and separates down/up
with bounded timing (`platform-macos/src/input/mouse.rs:208–260`). Upstream notes
that some AppKit hit testing uses the real pointer position. These are concrete
implementation differences to investigate. First-mouse/activation behavior of
the borderless fixture also needs instrumentation; adding a delay alone is not
a demonstrated fix. This audit did not compare the same experiment against
0.37.0, so it does not establish that either failure was newly introduced by
this pin update.

## Build checks and release boundary

Passed: Rust 8 unit tests; Python 4 click-harness plus 4 CI-proof tests; 70
recording-host prelude checks; TypeScript strict positive/negative contracts;
MCP smoke, receipt/paging/cancellation, runtime isolation, click validation and
source-line contracts. The final standalone package passed independent install,
embedded-runtime extraction and MCP checks in Unicode/space paths. Both packages
passed all 711 file checksums, local signature verification, and byte comparison
of packaged Skill/API/recipes with current source.

Final packages:

- `/private/tmp/dtw-cua-0380-package-final` — standalone adapter SHA-256
  `a333542cbbe26801ec59d2a43a896ce1d8991b88f25310065a8f96b96fd027ac`.
- `/private/tmp/dtw-cua-0380-package-final-slim` — GUI-tested adapter bytes above.
- `/private/tmp/dtw-cua-0380-upgrade-evidence/provenance.json` — provenance and
  build/contract evidence, deliberately separate from the GUI failure report.

**The pin update builds and passes package/contract checks, but the complete GUI
acceptance gate is not green.** Fix and revalidate the SCK click and picker
capture/grounding cases before claiming full promotion readiness. Windows GUI,
multi-display pixel accuracy, WPS/Obsidian, a fresh independent Agent run, public
signing/notarization, and production replacement were not performed here.

## Recommended order

1. Instrument and repair the DTW SCK input path, and reject or safely handle
   malformed picker captures. Keep effect readback as the acceptance criterion.
2. Add typed-browser operations with exact-tab binding and the upstream consent
   contract; translate native escalation hints into operations DTW can execute.
3. Add read-only health diagnostics and improve observation/image/action
   ergonomics. Broaden observation-bound gestures only with explicit delivery
   policy and fresh-coordinate checks.
4. Evaluate optional perception and window-frame operations against real app
   tasks. Do not expose host management or replay tools just for tool parity.
