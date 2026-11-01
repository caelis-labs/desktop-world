# Windows host acceptance — 2026-11-01

This records Windows acceptance for [PR #83](https://github.com/caelis-labs/desktop-world/pull/83),
runtime/test commit `3929560f4f8bc15f6ad95e2aa8b8e4e049833771` and initial
implementation commit `3ea1360ee734e809d8a2394a832c3cf1e271cfdc`.
The final development binary has passed the native and Chrome controls below,
plus targeted keyboard follow-ups in both hosts. This is a
source-change acceptance record, not a release-archive or new macOS GUI sign-off.

Artifact identifiers below name retained evidence directories/files. Evidence
storage paths, credentials, raw conversations and model reasoning are not
published in this report. A completed task, an accepted native dispatch, and a
successful model interpretation are separate results.

## Identity and scope

| Run | Binary SHA-256 | Meaning |
| --- | --- | --- |
| Final Windows candidate | `e946c689d29b673cb8f73b36bd242bdd2ce137082c18bea2a15e93d8b07e8fae` | Frozen runtime used for final native/Chrome controls and staged for both host follow-ups. |
| Initial host tasks, desktop diagnostic and Notepad baseline | `c3a4749a83876db8b20a51e8c1d0613e46998315f8922e5c83e1b34ce4ece871` | Actual host-launched development binary. |
| Queue-synchronized keyboard candidate | `1c367faf87d3227dcd19462c0eed5a643a141495d454394082152d36e2d08253` | Notepad/capture controls passed; Chrome terminal receipt failed. Not promoted to hosts. |
| Terminal-keypress candidate | `a972054dc00054b51c03d5f893f754e5dfbf9afef00c566cdeac61cb376b0836` | Alt+F4 close passed; Notepad acknowledgement/cleanup failed. Not promoted to hosts. |
| Chrome `mouse-candidate-cache-1` | `3a80b79e60895baa07ae175bbfe14828cefe5e92fc174258f018407ac4d5751b` | Separate intermediate binary; results remain bound to these bytes. |
| Retained mouse baselines | `88670d5740e8e5b075ec9cb138056936c7b4f93dc7a9ab2851c05ec99f57d96a` | Previously released v0.2.1 binary, used for comparison. |

These were extracted development plugin stages, not newly accepted immutable
release archives. Portable, Windows adapter, macOS inventory and source checks
passed on the final runtime/test commit in [CI](https://github.com/caelis-labs/desktop-world/actions/runs/41289725974).
The macOS result is **CI only**; its native implementation was not changed.

## Final candidate controls

- Release profile: **431 library + 3 CLI tests passed**, including **56 Windows
  adapter controls**. Formatting, version consistency, plugin package and fixture
  checks passed.
- Three fresh Notepad documents: **27/27** controls passed. All saved text matched
  independent disk reads and protected files retained their hashes. The 279 queue
  acknowledgements include 58 real named-tail completions with business-key
  synchronous/global release and global E8 release evidence before synchronous
  sentinel-up was seen; all 210 Unicode acknowledgements retained strict
  sentinel-up. Artifacts: `notepad-shortcuts-named-tail-candidate-{1,2,3}/`.
- Alt+F4: one dispatched/unverified shortcut, confirmed closed window and natural
  Chrome exit `0`. Artifact: `keyboard-terminal-named-tail-close-1/`.
- Capture: automatic preparation and persistent topmost refusal both passed,
  each with one ordinary focus preparation and unchanged business effects.
  Artifact: `capture-preparation-e946-1/`.
- Chrome: all four normal/trusted semantic/pixel cases submitted exactly once.
  Filtered 12-page and unfiltered 19-page traversals completed with the same exact
  360 links. Background pixel clicks preserved the physical cursor; keyboard
  continuation and natural exit passed. Artifact: `browser-reliability-e946-1/`.
- Browser mouse continuation: one background click opened one dialog, the
  physical cursor stayed fixed, and Escape/Alt+F4 completed with natural exit.
  Artifact: `browser-mouse-e946-2/`. Its first attempt stopped before click at a
  persistent capture occlusion; that failure remains retained and unexplained.
- Native GUI smoke: **25 functional cases passed**, including both fixtures'
  final full-VK release observations. Business keys remain strictly paired;
  global VK 1–255 release checks include E8. Per-Form E8 message asymmetry is
  retained separately because the Form filter is not the complete shared-queue
  event stream. Artifact: `native-smoke-e946-2/`. The first smoke's overbroad
  per-Form marker pairing failure remains in `native-smoke-e946-1/`.

## Final-candidate live keyboard follow-ups

Both hosts were updated to the same `e946c6…` binary and matching bundled Skill.
DSH hot reload retired its stage04 connection and launched stage05. Antigravity
was normally closed before replacing its plugin files, then reopened; its actual
Language Server child launched the updated global plugin executable. Checks used
the executable hashes, not the unchanged development version string.

DSH's fresh `deepseek-flash` session completed one `typeText`, Enter and Ctrl+S
without a menu/setter fallback or write replay. Native readback was Unmodified,
and the independent 68-byte file matched exactly. One earlier read-only script
misused `observe.pages` and raised a model-script TypeError; the failed reads and
error remain recorded. Artifact: `dsh-followup-e946-summary.json`.

Antigravity's first final-byte session also produced the exact 68-byte document
using one actual DTW type/Enter/Ctrl+S group. It additionally used terminal
directory queries and unrelated source/configuration reads, violating the
MCP-only task constraint. Its final claim of no terminal usage contradicted the
recorded tool arguments. This is a **scope-failed attempt with a successful
keyboard effect**, not a clean MCP-only pass. No configuration contents or model
reasoning are included here. Artifact: `agy-followup-e946-summary.json`.

A second, independent Antigravity conversation used a new owned document and a
short business-only request. It used one Skill read and only DTW MCP afterwards:
one `typeText`, Enter and Ctrl+S, with no terminal, unrelated file reads, setter
or menu-save fallback. The 65-byte result matched exactly and all four prior
documents retained their hashes. This attempt passed; the prior scope failure
remains in the two-attempt denominator. Artifact: `agy-simple-e946-summary.json`.
An additional supervisor-only DTW read independently confirmed the current
document text and all five Unmodified tabs; all three follow-up files matched
their expected content and the two original files retained their hashes.
Artifact: `host-final-independent-readback-e946/summary.json`.

These are targeted changed-path checks; the initial complete inventory/document
tasks below retain their original candidate identities.

## Actual host integration and tasks on the initial candidate

| Host and model setting | Result | Independent evidence |
| --- | --- | --- |
| DSH Desktop `0.2.1-rc.5`, `deepseek-flash`, High | Cordis plugin loaded from a file URL containing spaces and non-ASCII characters. Profile hot reload stopped the old standalone connection and launched the staged binary without restarting the App. The task saved `DTW-0042` with quantity `73`, reviewed `true`, and note `DSH 插件验收完成🙂`. | `host-dsh/state.json`: exactly one save, only the requested record changed. `host-independent-readback.json` and `DSH plugin acceptance.txt`: final Notepad Chinese/emoji content matched on disk. |
| Antigravity App `2.28.2`, Gemini 3.8 Flash, High | The native plugin launched its own bundled executable with `command: "./bin/dtw.exe"` and `cwd: "${PLUGIN_ROOT}"` after a normal full App close/reopen. The task saved `DTW-0099` with quantity `64`, reviewed `true`, and note `AGY 插件验收完成🙂`. | `host-agy/state.json`: exactly one save, only the requested record changed. `host-independent-readback.json` and `AGY plugin acceptance.txt`: final Notepad Chinese/emoji content matched on disk. |

DSH's final document result does not erase the shortcut failure encountered
during the task. That failure was retained and reproduced separately below.

Antigravity initially kept an old MCP connection after its plugin manifest and
green status refreshed. A new conversation alone did not establish that the new
binary had launched. Acceptance checked the actual executable identity of the
Language Server's MCP child after reopening the App. See the
[Antigravity installation guide](install-agy.md) for this upgrade distinction.

### Screenshot transport versus model vision

- **Antigravity passed the first attempt** on the owned pixel-only challenge:
  expected and submitted `9F2VDNMA`. Evidence: `vision-agy/state.json` and
  `vision-independent-readback.json`.
- **DSH failed the first attempt** on a separate challenge: expected
  `3V79YAY2`, submitted `3U79YRY2`. Evidence: `vision-dsh/state.json` and
  `vision-independent-readback.json`.
  The original MCP PNG was complete and visually legible, with dimensions
  `1438 × 1251` and size `53,805` bytes. Image delivery was present; this is not
  a model-vision pass and was not converted into one by retrying the answer.

The challenges belong to the disposable local fixture. Their results establish
only these observations, not general visual accuracy or relative model quality.
DSH's final attachment link displayed `Image preview unavailable` despite the
retained original MCP PNG. This separate preview defect was not fixed by changing
DSH Core in this PR.

## Focused Windows regressions

| Check | Observed result and boundary | Artifact identifiers |
| --- | --- | --- |
| Named empty desktop | The actual worker returned `EnumWindows = FALSE`, error `0`, callback count `0`. DTW emitted `desktop_enumeration_unproven` with measured desktop context instead of a misleading "operation succeeded" exception. No host windows leaked into the result. Owned processes exited and the test desktop was removed. No desktop switch, ACL change or sandbox bypass occurred. | `dtw-host-polish-desktop-20261101-07/summary.json` |
| Chrome link traversal | The `3a80b…` run completed `13` filtered and `19` unfiltered pages with the exact expected `360` links. The later `1c367…` run independently completed `12` filtered and `20` unfiltered pages, each visiting `1,786` nodes and returning that same exact set. This is coverage evidence, not a latency comparison. | `mouse-candidate-cache-1/summary.json`, `browser-reliability-final-1/summary.json` and their `filtered-links.json` / `unfiltered-links.json` |
| Chrome mouse/Invoke effects | All four `3a80b…` cases produced one accepted attempt/submission each: semantic press and screenshot-coordinate click in ordinary and trusted-pointer modes. On `1c367…`, the full traversal run's two pixel cases stopped at persistent capture occlusion **before click**; both semantic cases submitted once. A separate fresh case with traversal skipped did not reproduce the occlusion: all four inputs submitted once, and both background pixel clicks left the physical cursor unchanged. That diagnostic run still failed its closing receipt. | `mouse-candidate-cache-1/summary.json`, `browser-reliability-final-1/summary.json`, `browser-occlusion-diagnostic-1/summary.json` |
| Automatic capture preparation | Both `c3a474…` and `1c367…` passed the two owned-window controls: one ordinary focus preparation uncovered the target and produced valid PNG without business effects; persistent topmost foreign cover remained refused after one preparation, with no image. Owned processes exited normally. These controls do not explain away the separate Chrome occlusion failures. | `capture-preparation-c3a-3/summary.json`, `capture-preparation-final-1/summary.json` |

The desktop probe verifies scoped enumeration diagnostics and cleanup. It does
not prove that a restricted host can access the user's interactive desktop,
that AppContainer restrictions can be bypassed, or that locked/UAC desktops are
supported.

### Keyboard delivery and terminal-window boundaries

| Candidate/check | Accepted observations and retained failures | Artifact identifiers |
| --- | --- | --- |
| Initial `c3a474…` Notepad baseline | `8/9` controls passed. `ENTER` followed by `CTRL+S` inserted U+0073 (`s`) and did not save the expected disk text. This reproduced the DSH task defect. | `notepad-shortcuts-baseline-2/summary.json` and `enter-then-ctrl-s-case.json` |
| Queue-synchronized `1c367…` Notepad controls | Three fresh owned files passed `27/27` controls, with native-text/disk agreement, owned-tab closure, normal process cleanup and unchanged protected-file hashes. A scan-code-only experiment had instead passed `7/9` and retained the original failure; scan-only production changes were withdrawn. | `notepad-shortcuts-queue-candidate-{1,2,3}/summary.json`, `notepad-shortcuts-scan-candidate-1/summary.json` |
| `1c367…` Chrome Alt+F4 | Both Chrome runs returned unknown from the old foreground post-check after read acknowledgement. The full traversal probe terminated Chrome before proving natural exit. The fresh occlusion diagnostic instead waited read-only and observed natural exit `0` after `0.125 s`; its unknown receipt still makes the overall run a failure. | `browser-reliability-final-1/summary.json`, `browser-occlusion-diagnostic-1/summary.json` |
| `a97205…` terminal close | The dedicated Alt+F4 case passed: dispatched/unverified receipt, target closed and Chrome exited normally with `0`. Windows release-profile tests passed `427` library and `3` CLI tests; those checks do not establish GUI stability. | `keyboard-terminal-close-1/summary.json` |
| `a97205…` Ctrl+O continuation | Chrome visibly opened an Open window, but `waitForWindow(dialog)` could not prove strict modality while the original parent remained enabled. Two original cases failed. A separate supervisor owner/root-proof attempt also failed; no Escape continuation was sent or accepted. Production modality checks were not weakened. | `keyboard-terminal-candidate-{1,2}/summary.json`, `keyboard-terminal-observed-1/summary.json` |
| `a97205…` Notepad acknowledgement | The first five controls passed, then the End preparation for Shift+A failed: marker state changed `0 -> -127 (DOWN)` without the required UP evidence before the two-second budget. Result: unknown, `cleanup_failure`, MCP exit `2`. Later global/thread UP samples were only after-the-fact observations. Three diagnostic runs with external read-only sampling subsequently passed `9/9` each; they do not erase or resolve the original failure. | `notepad-shortcuts-terminal-candidate-1/summary.json`, `notepad-shortcuts-terminal-diagnostic-{1,2,3}/summary.json` |

After the failed terminal-candidate Notepad run, only its owned tab was safely
closed through the File menu using the initial `c3a474…` binary. Protected DSH
and Antigravity document hashes remained unchanged. This recovery is not a clean
shutdown result for the failed candidate. No timing adjustment, repeated unknown
shortcut or successful diagnostic rerun converts that original failure into a
pass. These controls do not establish an IME root cause.

The Chrome fixture did not reproduce the original live GitHub Create-button
failure. [Issue #82](https://github.com/caelis-labs/desktop-world/issues/82)
remains open. No change to `posted_mouse.rs` was justified by these fixture
results; neither a successful Invoke nor an accepted posted message alone proves
that an arbitrary website completed an action.

Earlier failures remain in `mouse-baseline/`, `mouse-baseline-2/`,
`mouse-baseline-3/` and `notepad-shortcuts-baseline-1/`. Foreign Computer Use
overlay rejection remained strict; successful fixture runs followed normal
coordinator reset. `mouse-baseline-4/` passed input only, with traversal unproven.
The first two capture-probe attempts also remain failed: duplicate exec/result
PNG handling and an owned cover that was not topmost were corrected in the
supervisor before the third run passed; production capture was not changed.

## Remaining gates and exclusions

- The final `e946c6…` native, Chrome, capture and terminal controls and both host
  targeted keyboard follow-ups passed. The initial complete business tasks
  remain bound to `c3a474…`; these changed-path checks do not replace them.
- `a97205…` is not accepted as stable. Its failed Unicode-style named-key
  acknowledgement remains a failure. The final candidate distinguishes a named
  payload-tail witness plus business-key synchronous/global release evidence
  from the sentinel's own synchronous-up observation. Unicode still requires
  strict UP within its operation; a later UP sample does not retroactively
  supply that proof.
- DSH's first-attempt visual-reading result remains failed even though the PNG
  reached the host. Antigravity's separate first-attempt pass does not replace it.
- Antigravity CLI was not installed or tested. Its installation commands are
  documented host entry points, not a CLI acceptance claim.
- Claude Code and Caelis received layout/packaging checks, not new real-host
  GUI tasks in this round. No new macOS GUI, clean-machine installation,
  signing/notarization or release-archive promotion is claimed.
- Document/paragraph reconstruction, generic website readiness, WGC occlusion
  capture and an authenticated cross-desktop broker are not established by
  these tests. Host tool/session lifecycle redesign is outside this change.

## Reproduce the independent probes

Use an unlocked Windows desktop with one automation owner. Run from the
repository root; supply the exact binary under review and **new** output
directories. The probes operate only on owned fixtures/documents. Their summaries
retain binary hashes, outcomes, effects and cleanup. Do not replay an uncertain
action to turn a failed case into a pass.

```powershell
$dtw = '<candidate-dtw.exe>'
$chrome = '<installed-chrome.exe>'
$notepad = '<installed-notepad.exe>'
$evidence = '<new-evidence-parent-directory>'

python -X utf8 tests/acceptance/windows/desktop_probe.py `
  --dtw $dtw --output (Join-Path $evidence 'desktop')

python -X utf8 tests/acceptance/windows/browser_reliability_probe.py `
  --dtw $dtw --chrome $chrome --output (Join-Path $evidence 'chrome')

python -X utf8 tests/acceptance/windows/notepad_shortcut_probe.py `
  --dtw $dtw --notepad $notepad --output (Join-Path $evidence 'notepad-shortcuts')

python -X utf8 tests/acceptance/windows/capture_preparation_probe.py `
  --dtw $dtw --output (Join-Path $evidence 'capture-preparation')

python -X utf8 tests/acceptance/windows/keyboard_terminal_probe.py `
  --dtw $dtw --chrome $chrome --case close --output (Join-Path $evidence 'terminal-close')

python -X utf8 tests/acceptance/windows/keyboard_terminal_probe.py `
  --dtw $dtw --chrome $chrome --output (Join-Path $evidence 'terminal-continuation')
```

For localized Notepad menus, provide the probe's `--file-label`, `--save-label`
and `--close-tab-label` options using the labels actually exposed by that
installation. Chrome/capture probes use Pillow for independent owned-pixel
checks; fixture compilation uses the installed Windows PowerShell/.NET tools.
These reproduction commands do not imply that a pending case has already passed.

Host tasks additionally require a fresh session using the bundled Skill and a
verified host-launched MCP identity. Compare the local fixture journals and
saved document bytes independently; do not accept the model's completion message
or a green plugin status as the sole oracle. See the
[Windows handoff](../tests/acceptance/windows/README.md),
[DSH setup](install-dsh.md) and [Antigravity setup](install-agy.md).
