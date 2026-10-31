# Windows rewrite follow-up — 2026-10-31

The Windows defects found during desktop acceptance are addressed in this change.
This is a normal-user Windows 11 development-build regression record, **not final
release approval**. The broader Windows matrix and final distribution artifact
still require acceptance. The machine-readable companion is
[windows-rewrite-2026-10-31.json](evidence/windows-rewrite-2026-10-31.json).

## Changes and scope

| Failure | Resulting behavior |
| --- | --- |
| A pipe-only native worker could not activate a normal window from another foreground process. Attaching only the foreground thread still failed with Chrome in front. | Create the worker message queue and temporarily attach both foreground and target queues. Issue one activation request, detach in reverse order, and confirm the exact HWND for at most 200 ms. No reserved-key assistance or target input before confirmation. Detach failure remains sticky cleanup failure. |
| Maximized `GetWindowRect` included invisible borders outside the monitor. | Use a positive, contained DWM visible frame for maximized windows, consistently across capture, retained regions and pixel input. Genuinely offscreen windows remain rejected. |
| Desktop `BitBlt` captured another application's pixels during maximize animation, even after geometry, compositor and hit-test checks. | Render only the exact HWND with `PrintWindow/PW_RENDERFULLCONTENT`, crop its physical frame, and reject blank/unsupported rendering. There is no desktop-copy fallback. Existing visibility, occlusion, identity and geometry admission remains. |
| Windows PowerShell 5.1 converted the null `File.Replace` backup argument into an empty string, and stale-reference assertions expected the wrong settled status. | Use `NullString`, assert the actual completed-with-error ledger and not-dispatched receipts, and preserve independent zero-effect checks. Require bounded, ready fixture snapshots before comparing effects; record script stacks on harness failures. |
| Keyboard emoji input generated a transient UTF-16 surrogate in the browser event journal, poisoning every subsequent UTF-8 persistence operation. | Persist and return JSON strings with ASCII escapes. Parsing preserves every code unit, Chinese text, emoji and line break; later saves and attachment receipts remain writable. No text is dropped or normalized. |
| A Claude-format Windows package acquired its extracted directory name as the Caelis plugin ID. | Windows candidates also carry standard Agent Plugins 1.0 manifests. Caelis installs one `desktop-world` MCP server and the bundled Skill under the stable ID `desktop-world`, including paths with spaces and Chinese characters. |

Native runtime changes are confined to `backends/windows`. macOS native source,
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
