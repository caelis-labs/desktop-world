# Browser integration and review acceptance

> Historical evidence: this report applies only to its named source revision, binary and entry point. The current product uses [standard MCP and explicit app IDs](app-protocol-v1.md); any grant, manifest, SDK/worker or custom Host setup below is superseded and is not a current installation instruction. No acceptance result here is claimed to have been rerun for the new entry.

Checked on macOS arm64, 2026-10-25, using the unmodified CUA 0.38.0 worker at
`85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`. This record supersedes the initial
form-elicitation design in PR #47. The current adapter delegates tool approval
to the Harness, matching CUA's native MCP ingress (`cua-driver-core/src/server.rs`).

## Review fixes

| Finding | Result |
| --- | --- |
| All calls stop after 25 minutes of absolute age | Removed the absolute-age cutoff. After 25 idle minutes, return `runtime_context_reset`, discard handles/state and allow a new observation in the same authorization context. |
| Cancelling a pending confirmation kills the Runtime | Removed the confirmation subsystem. Request cancellation tracks its own admitted job; before admission it leaves the Runtime usable, and late/read-only cancellation cannot kill another job. |
| Bounded mode cannot discover the approved window | `describe` without a name returns configured targets and directly callable `next_call`. No script scout is required. |
| Default receipts omit tabs/refs/outline | Small native results are inline; large results retain whole refs and advertise pagination. Raw diagnostics remain available. |
| Recovery and descriptions contain unusable session/authorization instructions | Adapted descriptions and `next_call` to DTW's actual operations; host-only profile requirements are marked. Native schemas remain derived from the pinned worker. |
| Timed-out elicitation leaves a form open | No DTW elicitation, approval timeout or stale form remains. The Harness controls whether to dispatch the call. |
| Windows needs special canonical path spelling | DTW accepts ordinary absolute directories and canonicalizes them before CUA dispatch. |

The main Skill now teaches discovery, observation, action and verification in
about 100 lines. Advanced click details remain in recipes. Deployment and scope
configuration live in [harness-integration.md](harness-integration.md); the unused
secondary confirmation-policy checklist was removed.

CUA still owns bounded manifest validation, live PID/window/origin checks, ref
freshness, file scope and grant expiry. DTW freezes the host-approved manifest at
startup and never reloads it during idle recovery. The custom SCK app/click route
is unavailable in bounded mode because it does not enforce native manifests.
There is no new tool, approval flag or permission cache.

## Real Harness and browser evidence

`tests/acceptance/browser_authorization_probe.py --caelis` runs an independently
launched disposable Chrome profile, a localhost fixture and an isolated Caelis
Store. Host setup selects its owned window through macOS and configures the native
manifest. The Agent prompt contains a task and download destination, with no
PID/window/tab/ref IDs. The Agent reads the installed Skill and obtains every
browser binding from DTW. The test does not fetch hidden structured results for it.

The real Caelis v0.73.2 Agent used the user's existing local provider configuration
(`deepseek/deepseek-flash`). It used Skill, ToolSearch, one browser-reference read
and **12–14 DTW calls across two runs**, including `describe`, native setup recovery, observations,
typing, clicking and download. There were **zero structured-result fetches**, zero
scripts and no shell/HTTP/fixture-file reads by the Agent. Re-observation confirmed
the textbox and rendered output both contained `DTW-Harness-verified`.

Independent fixture evidence confirmed the Apply HTTP event and one Harness
initiated download, containing the exact 35 bytes with SHA-256
`ce22c6927afd8a37dba402702c5b7e050e85b33c70d817cc17eb905ef1b15072`.
The direct protocol check separately downloaded one file. Wrong windows, denied
origins, outside-manifest directories, stale refs and private-field forgery were
refused. Changing the original manifest after startup did not change the frozen
scope. The pre-existing Chrome process survived DTW EOF, and only the fixture's
owned process was cleaned up afterwards.

Final-package evidence: `/private/tmp/dtw-review-final5-caelis/report.json` and
`caelis-transcript.jsonl`; the first run is in `/private/tmp/dtw-review-browser-caelis1`. This is a real Caelis model/Skill/MCP acceptance, not
CaelisBot UI or Telegram acceptance. No production installation, personal browser
profile, account credential or cross-Harness approval UI was changed or tested.

## Lifecycle and package checks

The opt-in Rust recovery test uses the real supervisor/worker and advances DTW's
idle timestamp, without waiting 25 wall-clock minutes. It proves fresh state after
idle reset, no dispatch of the reset-triggering script, pre-admission cancellation
recovery, and expiry of a three-second native manifest after a context reset.
Continuous use is covered by removing the absolute-age state and idle-boundary
unit checks; this is not a multi-hour soak test. The native expiry test is wired
into macOS and Windows package CI.

Source checks cover Rust formatting, 70 pinned-prelude cases, strict TypeScript
and eight Python unit tests. The Windows failure on the prior PR head was a test
reading UTF-8 native output through cp1252; the probe now specifies UTF-8.
Final local package validation passed 17 Rust tests (including native recovery),
MCP/receipt/schema/authorization/isolation/cancellation/worker-lifecycle probes,
and standalone installation/export into a path containing spaces and Chinese.
Both package variants have 711 independently verified checksum entries and valid
ad hoc signatures. Final artifacts:

| Artifact under `/private/tmp` | SHA-256 |
| --- | --- |
| `dtw-review-final5/bin/dtw` | `8fd214b16efa4604d722f8cc2aefa5dca3b3c91e53172dba4c6db1a8b137ca65` |
| `dtw-review-final5-slim/bin/dtw` | `1014cfc17fb6e1ba95941cd442b6493a93bd494bc34559b1ce5796523ba7ac62` |
| Packaged CUA worker | `f1463aad7bda8f236003ecb1d8d20c7da7c2c2d4f72127bf7c258f37427f1014` |

Logs: `/private/tmp/dtw-review-final5-checks.log`,
`dtw-review-final5-rust.log` and `dtw-review-source-final.log`.

Windows GUI acceptance was subsequently performed below. CaelisBot front-end
acceptance remains unperformed.

## Windows review and follow-up fixes (2026-10-25)

Windows 11 x64 (build 22631), 125% display scaling, CUA 0.38.0 at the same
`85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19` pin:

- Native validation errors without a structured payload now retain their text in
  the default receipt. `browser_prepare` without a PID and `browser_dialog`
  without a target identify the missing argument instead of displaying `null`.
  The real-worker native-tools probe checks both cases on Windows and macOS CI.
- Standard Windows `describe` now advertises `menu`; only the macOS-specific
  `app` and SCK `click` routes are excluded. Bounded discovery still follows the
  host manifest. The WinForms fixture independently recorded exactly one menu
  callback, and discovery has a regression assertion.
- Skill and TypeScript declarations now state that `typeText.delay_ms` applies
  only to macOS synthesis. The pinned Windows implementation ignores it,
  including foreground SendInput. This documents the actual worker contract;
  it does not claim to add delayed Windows typing.

Both standard isolated Chrome and host-approved bounded existing Chrome passed
the inline-receipt workflow: discover/bind, observe, reject a stale ref, type
Chinese, apply once with independent page-event readback, select, download into
a Chinese path, and observe again. No structured-result fetch was needed.
Isolated Chrome exited with EOF; the existing browser survived EOF. These are
scripted Agent-interface checks, not a new Windows model/harness evaluation.

DPI-aware WinForms passed discovery, exact screenshot-frame alignment, foreground
Unicode/multiline input, semantic submit, independent Ctrl+S disk readback,
pixel click and capture-reuse rejection, checkbox, three rounds of setValue and
native menu invocation. MCP/contract/click/native-tools/authorization/runtime/
lifecycle probes, 15 Rust unit tests, 70 pinned-prelude cases, eight Python unit
tests and strict TypeScript passed locally.

Local evidence is under
`D:/WorkDir/desktop-world/artifacts/pr47-fixes-20261025`, including `browser/`,
`browser-bounded/`, and `native/windows-live-4d3f364f/summary.json`.
The tested standalone adapter SHA-256 is
`0b1add24357c220cb4d8a5920fb289466580f1794883ee1b38cd28bd0fd6a6b0`;
the unchanged worker SHA-256 is
`d6105cce860760f0e186b637e81a1782a95d97b7aca812facc2566de8e65b791`.
Release builds have separate provenance and require candidate verification.

The existing [Windows limitations (#36)](https://github.com/caelis-labs/desktop-world/issues/36)
remain limited support under the established release policy. The review again
reproduced DPI-unaware 125% capture misalignment (PNG box `[198,206,155,39]`, UIA
frame `[250,257,194,50]`) and a picker-opening click that returned
`foreground_unavailable` after opening the dialog. Scoped re-observation and
Return completed the picker with zero replay; the opening receipt still failed.
The Skill now explicitly stops pixel input on alignment mismatch and instructs
Agents to observe a new modal before any further input. Pending semantic modal
calls retain the original receipt/cancel-and-reobserve workflow. These are
documented handling paths, not upstream capture or modal fixes.

## Remaining CUA and integration limits

- **Chrome setup without an existing endpoint:** the owned ordinary Chrome
  window failed CUA's exact AX `New Tab` matcher in
  `platform-macos/src/browser/setup_ui.rs`, returning `browser_wrong_target_refused`.
  Attachment with an already available DevTools endpoint is verified. DTW does
  not silently relaunch the browser or guess setup coordinates.
- **Dynamic existing-profile consent:** embedded `DriverAuthorizationHost`
  exists, but the released private-worker protocol has no callback transport.
  The supported route is host-approved bounded startup. Dynamic protected profile
  dialogs need an upstream transport extension and DTW integration; ordinary MCP
  elicitation is not a substitute.
- **Synchronous JavaScript dialogs:** a prompt-triggering click can time out even
  though the prompt opened. Inspect before the trigger and resolve its returned
  generation; do not replay the click. This remains upstream behavior.
- **Other pinned native limits:** parent/sheet screenshot composition, uncertain
  background gestures, picker AX discovery/Escape, Chromium-only typed browser
  engines and primary-display desktop capture remain as recorded in
  [the earlier repair acceptance](cua-0380-repairs.md). DTW's SCK repairs cover its
  own app route, not all native providers.
