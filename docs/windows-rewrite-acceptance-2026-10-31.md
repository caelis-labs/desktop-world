# Windows rewrite follow-up — 2026-10-31

This record includes multiple successive Windows candidates. **Current decision:
No-Go for release**, because lossless cancellation stress still leaves a terminal
high surrogate in the original app. Fixes and passing tasks below do not override
that failure. The original machine-readable companion is
[windows-rewrite-2026-10-31.json](evidence/windows-rewrite-2026-10-31.json).

## Reviewed CI candidate and latest result

The latest native source is `a13775908df0052fea3227a23c816df90aea8612`, built by
[Windows candidate CI](https://github.com/caelis-labs/desktop-world/actions/runs/41208527909).
Binary SHA-256: `3c80e2384cf292de176a390a610fcfa4c2411a3285db06790bf5e782daa9252f`;
package ZIP: `aa63bf1647d5ff89d966a9b64725ce63b418d99855db337e562ce0d4988e8c20`.
All 197 packaged checksums matched before and after the tasks. Source/macOS native
checks passed in [CI](https://github.com/caelis-labs/desktop-world/actions/runs/41208528717).
The current sanitized evidence is
[windows-parity-2026-10-31.json](evidence/windows-parity-2026-10-31.json).

On normal-user Windows 11 build 22631, primary display at 125%, this candidate
passed 386 Rust library tests and 3 CLI tests in CI, plus these actual local GUI
runs: base 25, cursor/session/lifecycle 15, blocked UIA/cancel/timeout/protocol 4,
DPI-unaware physical capture/input/cursor 3, and sibling/instance restart 3.
The latest base run uses a read-only snapshot sharing-race fix. The cursor run
uses lossless surrogate snapshots and fresh stable key-release samples.

The separate 20-transaction cancellation stress stopped at transaction eight
after seven passes. Its lossless result is a UTF-16 prefix ending in U+D83D,
not a scalar prefix. Original cancellation retained unknown dispatch and null
shutdown error; the queued owner's text stayed empty, its click occurred once,
no save occurred, and keys/native processes cleaned up. This is an unresolved
application-processing boundary, not evidence of successful atomic text delivery.
The private-tag queue observer experiment also failed and was removed. No global
keyboard hook, replay/repair text, guessed delay or relaxed gate is shipped.

The next fix must prove that an accepted Unicode pair has reached the original
application before focus handoff, or explicitly fail closed where that proof is
unavailable. `SendInput` acceptance, low-level hook observation and a synchronous
sent-message fence do not establish processing of the application's queued
`WM_CHAR` messages; sent and posted messages can precede input in the
[Win32 retrieval order](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessage).
Do not broaden this fix into macOS input or change shared
receipt semantics. Re-run the same lossless stress and original cancellation,
EOF, parent-exit and concurrent-owner cases before removing the release block.

### Real DeepSeek Flash tasks

The standard installed DTW MCP and bundled Skill performed every application
interaction. No shell, DOM/CDP, direct business-file editing, alternate automation
or delegated Agent was used. Read-only independent disk/hash verification and
native screenshots establish the results. The user allowed installed substitutes
for unavailable WPS/Obsidian; WordPad and Explorer are those substitutes, not
coverage claims for the missing apps.

| Host/task | Time | exec / MCP / native | PNGs | Result and preserved failures |
| --- | ---: | --- | ---: | --- |
| Caelis 0.75 / Chrome | 158.83s | 35 / 37 / 104 | 25 | Two target records saved once, exact keyboard-entered Chinese/emoji notes, reopened readback, matching native file upload; 238 neighbors unchanged. A workbench screenshot was visually checked. 5 errored execs / 5 failed native steps remain recorded. |
| Caelis 0.75 / WordPad, initial | 7.79s | 0 / 2 / 0 | 0 | Host rejected a thirteen-method API response with `11188 > 10000` before any GUI input. Preserved as a failed session. |
| Caelis 0.75 / WordPad, independent workaround task | 344.35s | 55 / 58 / 160 | 39 | Exact four paragraphs, heading bold/body regular, saved/reopened RTF, native readText and body screenshot; seed unchanged. API queries capped at three methods only for this old-host run. 14 errored execs / 24 failed native steps. |
| Caelis 0.75 / Explorer | 147.22s | 28 / 30 / 50 | 16 | Created Reviewed, renamed/moved the target, inspected child/parent, exact resulting tree and all hashes; two neighbors unchanged. 3 errored execs / 6 failed native steps. |
| Core PR #182 `3c3e09f3` / WordPad | 298.82s | 44 / 47 / 109 | 16 | One thirteen-method API query, complete artifact read and uninterrupted continuation; exact RTF/formatting/seed and reopened native readback. 20 failed native steps remain recorded. |

WordPad reports partial UIA coverage (`native_read_gap`); this is never promoted
to complete coverage or uniqueness. Native `readText` returns the actual four
paragraphs, including Chinese and emoji. Offline RTF parsing confirms content and
bold state. Both WordPad runs exit normally; read-only DTW window enumeration
confirms the owned Explorer task is closed without terminating the shared shell.
Screenshots show actual document/workbench/Explorer content, not only dialogs.

### Caelis Core integration

The old 0.75 error was a host canonical serialization-budget defect; it was not a
DTW API limit. PR #182 already fixes that calculation. The follow-up on its latest
branch adds complete multipart artifacts and reserves the trusted recovery
reference behind earlier text, using the generic pipeline for all tools.
[Core commit](https://github.com/caelis-labs/caelis/commit/3c3e09f336e5f919bf805fc05f77d458a5ab1e78).

The frozen Core binary has SHA-256
`dae9b2259189cd13a539ccffc42d2f8408c0702b993f802bdd0952710664e16d`.
Its live API artifact is 109,720 bytes, two ordered text/JSON parts containing all
13 method definitions. Text parses to the same complete structured payload;
the Agent reads the runtime artifact and continues. The model-visible result,
durable event and file-store replay remain canonical and bounded. One unique
WordPad app-access approval request covers the session's 44 execs (three approval
events are lifecycle updates of that one request). SDK tool/session/runtime suites
and `make commit-check` pass. No global Caelis configuration or installed binary
was replaced. [Tool-loading assessment](plugin-tool-loading-evaluation-2026-10-31.md)
separately evaluates eager DTW entrypoints; it changes no loading policy here.

Remaining matrix gates include mixed-monitor/movement and additional DPI scales,
Windows minimum-version execution, elevation/secure desktop, and final release
packaging/signing. Native changes remain within Windows implementations and
Windows-only wiring; macOS native source, public API/Skill and Cargo.lock are
unchanged. The sections below preserve earlier candidates and their limitations.

## Earlier Windows parity verification

The later development binary `ea973ccd8c52e5dd262d0400e684da4131f3b1fd7328eb0bac6d0a261f84c9ad`
passes **385 library tests, 3 CLI tests, the full 25-case GUI smoke and 15
cursor/native parity GUI cases**. The original final-queue binary also passed
all 25 smoke cases on a normal foreground desktop before these changes.
Earlier failures below remain historical results of their recorded binaries.

Windows session cursor feedback is feasible and included. The
[Windows port record](windows-cursor-port.md) describes exact upstream source
hashes, session ownership, physical pixel checks, cross-session exclusion and
the new native modal waits and retained pixel/semantic wheel anchors. In-flight
cancel/EOF/parent-exit and concurrent-owner handoff now have actual process-exit
and released-input evidence. The queued-owner abandoned-mutex defect and a
parent-exit worker classification error were reproduced and fixed.

Common adapter wiring is Windows-conditional; macOS native code, public API and
Skill remain unchanged. Four existing windows-crate features support pipes,
file handles and the native helper module. No dependency version changes.
These later results supersede only the corresponding pending gates below.
Final candidate construction, new Caelis runs and remaining matrix results are
recorded separately; a development binary is not release approval.

## Changes and scope

| Failure | Resulting behavior |
| --- | --- |
| A pipe-only native worker could not activate a normal window from another foreground process. Attaching only the foreground thread still failed with Chrome in front. | Create the worker message queue and temporarily attach both foreground and target queues. Issue one activation request, detach in reverse order, and confirm the exact HWND for at most 200 ms. No reserved-key assistance or target input before confirmation. Detach failure remains sticky cleanup failure. |
| Maximized `GetWindowRect` included invisible borders outside the monitor. | Use a positive, contained DWM visible frame for maximized windows, consistently across capture, retained regions and pixel input. Genuinely offscreen windows remain rejected. |
| Desktop `BitBlt` captured another application's pixels during maximize animation, even after geometry, compositor and hit-test checks. | Render only the exact HWND with `PrintWindow/PW_RENDERFULLCONTENT`, crop its physical frame, and reject blank/unsupported rendering. There is no desktop-copy fallback. Existing visibility, occlusion, identity and geometry admission remains. |
| Windows PowerShell 5.1 converted the null `File.Replace` backup argument into an empty string, and stale-reference assertions expected the wrong settled status. | Use `NullString`, assert the actual completed-with-error ledger and not-dispatched receipts, and preserve independent zero-effect checks. Require bounded, ready fixture snapshots before comparing effects; record script stacks on harness failures. |
| Keyboard emoji input generated a transient UTF-16 surrogate in the browser event journal, poisoning every subsequent UTF-8 persistence operation. | Persist and return JSON strings with ASCII escapes. Parsing preserves every code unit, Chinese text, emoji and line break; later saves and attachment receipts remain writable. No text is dropped or normalized. |
| A Claude-format Windows package acquired its extracted directory name as the Caelis plugin ID. | Windows candidates also carry standard Agent Plugins 1.0 manifests. Caelis installs one `desktop-world` MCP server and the bundled Skill under the stable ID `desktop-world`, including paths with spaces and Chinese characters. |

The initial native runtime changes were confined to `backends/windows`. macOS native source,
shared runtime/protocol, public API, Skill, Cargo dependencies and macOS package
layout are unchanged. Shared fixture serialization is lossless JSON escaping;
the package builder adds the standard manifests only for the Windows target.
Caelis itself is not patched. Switching an existing registration to another
directory requires removing that exact old ID first; unrelated plugins remain.

## Source comparison

The donor remains CUA 0.38.0, commit
[`85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`](https://github.com/trycua/cua/tree/85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19).
The comparison uses `platform-windows/src/input/inject.rs` and `capture.rs`, with
exact donor hashes in [the provenance record](native-source-provenance-cross-platform.md).
DTW adds the target-queue attachment required by the local Chrome reproduction,
paired cleanup, exact-target confirmation and existing receipt/deadline rules.
Upstream's reserved-key foreground assistance and desktop fallback are not used.

## Verification

- Rust 1.98.0, MSVC x64, static CRT: **379 library tests and 3 CLI tests passed**
  on the final native source. The stdio test target selected zero tests; it is
  not counted as another passing integration suite.
- Candidate/fixture Python checks: **27 tests passed** on Windows. The remaining
  macOS signing shell test needs the supported Unix shell environment in CI;
  its Windows invocation failed before building and is not counted as passing.
- Earlier exact-window-render build: **25 owned WinForms smoke cases passed**,
  including two process instances, Chinese/emoji keyboard input and paged native
  readback, pixel PNGs, stale-region zero-effect controls, pointer/wheel effects
  and normal process cleanup. This result belongs to its recorded binary hash,
  not automatically to later binaries.
- Three maximize/restore cycles exercised exact-window rendering. Immediate
  geometry changes could return `target_changed`; settled frames showed only
  the owned window. The Agent also exercised maximize/restore and pixel input.
- A real HTTP fixture negative control with the old serializer returned 400
  after an intermediate surrogate and failed later saves/uploads. The fixed
  control returned 200 throughout, with one save, one attachment, equal disk and
  memory state, and no leftover temporary file.
- The standard Windows package was installed with the actual Caelis 0.75.0
  plugin manager from a path containing spaces and Chinese characters. Status
  reported installed ID `desktop-world` and exactly one MCP server; the Agent
  loaded the shipped Skill and invoked that server.

The complete Chrome task uses **Caelis 0.75.0 + deepseek-flash**, in an isolated
store using only that model configuration. Agents may use only the shipped Skill
and DTW MCP for app interactions: no shell, HTTP, DOM/CDP, business-file editing,
clipboard fallback or delegated Agent. An independent supervisor reads fixture
disk state and hashes after the run; the model's final report alone is not proof.

The completed pre-final-queue run took 342.58 seconds: 37 execs, 38 MCP calls,
160 native calls and 20 returned PNGs. Disk verification confirmed exactly one
save each for DTW-0088 (37) and DTW-0240 (52), Reviewed true, exact two-line
Chinese/emoji notes, one matching attachment and all other 238 records unchanged.
Native readback after reopening confirmed the saved values. The first Cancel
succeeded inside an exec that later failed on an unrelated missing selector;
the later Cancel and closed editor were confirmed separately. The fixture does
not journal Cancel, so a zero Cancel-event count is not evidence of failure or
success. Two surrogate-bearing intermediate input events persisted successfully.

The **final native binary** (`41721bb7…96199d`) completed another real Agent run
in 427.64 seconds: 44 execs, 46 MCP calls and 153 native calls. Independent disk
verification again passed all business goals: two saves total, exact values,
reopened readback, one matching attachment, and 238 unchanged neighbors. Both
notes used UIA `setValue`: keyboard input was rejected because GameInputSvc owned
the foreground queue. Thus the required keyboard subtask **did not pass**. Page
capture was also rejected while its own translation bubble occluded the window.
One successful PNG showed that bubble; native PID/owner-HWND readback confirmed
it belonged to the owned Chrome instance, not another Chrome window. It does
**not** count as a successful workbench screenshot. This run is **partial**,
with 4 errored execs and 27 failed native steps including caught errors. It used
no alternate automation route. Normal cleanup completed and the original Chrome
scene was preserved. All 189 package checksum entries still matched afterwards.

The final-binary smoke remains gated on a normal interactive foreground desktop.
Failures, partial runs and superseded binary hashes remain separate from passes
in the companion JSON. Independent shared core checkpoint suites also passed
(83, 22, 20 and 5 tests); they are not additional desktop acceptance.

## Preserved failures and limits

- Foreground-only attachment was rejected with Chrome in front. The isolated
  native reproduction succeeded with both queues attached and both detached.
- The first final-queue smoke completed 11 cases, then hit a snapshot-property
  dereference failure in the harness. Bounded ready snapshot comparisons were
  added; their final GUI verification is still pending. This is not a 25-case pass. Subsequent smoke admission
  was denied while the foreground HWND belonged to the GameInputSvc system
  service (`AttachThreadInput`: access denied); receipts were not dispatched and
  no elevation or system-window input was attempted.
- The completed model run retained **8 errored execs and 24 failed native steps**
  (including caught lookup misses). These include a model TypeError, a non-JSON
  return, occlusion rejections, missing selectors and an unknown click receipt
  when the native file chooser had already opened. The Agent observed the new
  dialog before continuing. Business success does not erase those failures.
- `PrintWindow` can expose an application's transitional or older rendered frame.
  It does not establish synchronization with UIA or prove UI effect completion.
  The Agent used native value readback and subsequent frames rather than
  replaying input based on an older image. Unsupported/blank native rendering
  is an explicit failure; there is no WGC/background-capture support claim.
- This run does not close mixed/per-monitor DPI, monitor movement, in-flight
  cancellation/UIA blocking, concurrent owners, restart/EOF reclamation,
  elevated/secure desktop, or additional real-app matrix coverage. Final packaged
  macOS checks and Caelis Core's planned per-Session app approval remain separate
  release gates. A local unsigned debug package is not the next release artifact.

Raw local transcripts, screenshots and process/configuration evidence are kept
under ignored acceptance artifacts. They are not committed because earlier
rejected desktop-copy images can contain unrelated application content. Public
evidence contains sanitized counts and hashes rather than personal paths,
credentials or those images.
