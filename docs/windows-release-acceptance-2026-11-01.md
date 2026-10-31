# Windows release follow-up — 2026-11-01

This record follows the failed candidates in [the 2026-10-31 record](windows-rewrite-acceptance-2026-10-31.md). Their failures remain evidence. Acceptance applies only to the frozen bytes identified below, normal-user Windows 11 build 22631 and the primary display at 125%.

## Scope and fixes

The Windows changes add session cursor feedback, exact foreground/target validation, retained wheel anchors and modal waits, exact-HWND capture with DPI-unaware backing mapping, bounded UIA/runtime-ID handling and supervised native cleanup. [CUA comparison and attribution](windows-cursor-port.md) records the donor files and differences. macOS native source, public API, Skill and Cargo.lock are unchanged. Shared entry-point changes are Windows conditional wiring.

The original cancellation bug allowed a queued owner to take focus after SendInput accepted a surrogate pair but before the original app finished processing its WM_CHAR deliveries. A lossless stress run left literal U+D83D at transaction eight. Queue acceptance, low-level hook observation and sent-message fences could not establish application processing. The rejected hook experiment is not shipped.

For supplementary-plane typeText, Windows now reads the original focused control and its selection before input, then holds the input lease until that same control exposes the accepted scalar prefix and no held keys. Input remains real SendInput. Read-only acknowledgements retry within a bounded window; mutations are never replayed, repaired or replaced with a setter. Literal UTF-16 decoding rejects incomplete pairs. Existing public receipts remain dispatched/unverified, so callers still need effect verification.

If delivery cannot be confirmed, the Windows worker retains uncertainty and exits 74; another queued owner sees an abandoned input lease and does not dispatch focus. The negative control expects both MCP owners to exit 2, and preserves shutdown errors and the original unknown receipt. These are intentional fail-closed results, never ordinary clean exits.

Real Chrome found two additional admission/readback defects in candidate e3db634. Its terminal caret compared after DocumentRange's end despite belonging to the focused field; an empty edit exposed U+FFFC as an embedded-object placeholder, which disappeared on real insertion. The fix proves the selection's enclosing-control ownership and exact prefix/selection/suffix reconstruction. Only a writable, independently still-empty ValuePattern proves an empty placeholder; its setter is never called and literal U+FFFC text is retained. The regression checks empty/nonempty terminal carets, replacement beside existing surrogates, multiline text, and insertion/append with literal U+FFFC. The unchanged old candidate fails that regression.

This guard requires readable literal text and selection through UIA TextPattern, or a proven same-process Unicode Edit fallback using bounded system-marshalled WM_GETTEXT/EM_GETSEL reads. Password/unsupported controls are denied before text. Unreadable, over-budget, autocorrecting or transforming controls are not claimed as supported supplementary-plane routes. No claim is made for all custom editors.

## Frozen candidate and package

Native source: `da61485cd41cb0aee880139d94ab86c1e5bb5970`. [Windows candidate CI](https://github.com/caelis-labs/desktop-world/actions/runs/41224837671) passed **388 library + 3 CLI tests**; zero-selected test targets are not additional passes. Source/macOS native controls passed too.

- Binary SHA-256: `ca5f8a4286514ab4ded3479c4fc8f9dee07e838a6ed6619f8e30b44651bd7411`.
- Package ZIP SHA-256: `4c46085adb4d83e6a0c1024b652553159af482dc78afaf49b3f53af41d435922`.
- Actions artifact: `13051482896`, outer ZIP `39d2d9bd76a0b4dbcf59c8ac11f9d3301404e80a88694c61759f024322c6de0e`.
- All **199** packaged checksums match before and after tasks. The standard plugin was installed from a path containing spaces and Chinese characters. The package uses the existing unsigned/static-CRT Windows candidate contract.

Later commits change only the supervisor's focus-readiness setup and acceptance documentation; native source remains identical to this frozen build. [Structured evidence](evidence/windows-release-2026-11-01.json) identifies the exact candidate and completed groups. Publish accepted bytes without rebuilding; another artifact requires its own acceptance.

## Native GUI regression

| Group | Completed checks/transactions | Result |
| --- | ---: | --- |
| browser | 6 | Passed |
| delivery | 20 | Passed |
| delivery-delay | 20 | Passed |
| delivery-guards | 4 | Passed |
| base | 25 | Passed |
| cursor | 15 | Passed |
| blocking | 4 | Passed |
| dpi | 3 | Passed |
| restart | 3 | Passed |

Total: **100**, including 20 independent normal-processing and 20 controlled-40ms-delay cancellation transactions. Original receipts stay unknown; accepted text remains a scalar prefix, the next owner receives no stray text and clicks once, no save occurs, and keys are released.

The first delayed batch stopped at preparation after four passes: typeText was rejected before dispatch because focus was on a button. The revised supervisor places the editor once and requires fresh focused/key-free snapshots before arming; it retries observations only. That original batch stays failed. Two base wrapper preflights stopped before any MCP/GUI action because the inherited module path hid Windows PowerShell 5's Get-FileHash; the final wrapper explicitly uses its built-in module directory without changing global configuration. The 100-case record combines the four completed groups in `final-native-confirmed` and five in `final-native-tail-clean-env`; partial failed batches are not counted as overall passes.

The independent cursor observer verifies two simultaneously visible session markers, stable identity across app switches, exact capture exclusion, foreign same-class/same-basename occlusion, relative window order, geometry retirement and effective PMv2 raster DPI. Input still uses the shared foreground Windows pointer. Separate session markers do not imply independent OS pointers or background input.

## Actual Caelis + DeepSeek Flash

All Agent app observations and actions use the installed standard DTW MCP plus bundled Skill. The only extra Read is the host-owned complete tool-result artifact. No terminal, DOM/CDP, HTTP app access, direct business-file mutation, alternate automation or delegated Agent is used. The supervisor prepares disposable fixtures and reads their files/hashes independently after each task. WPS and Obsidian were absent; WordPad and Explorer are user-authorized substitutes, not coverage claims for those apps.

| Task | Seconds | exec / MCP / native | PNGs | Errored exec / failed native steps | Independent result |
| --- | ---: | --- | ---: | --- | --- |
| chrome | 183.31 | 44 / 45 / 132 | 30 | 4 / 7 | Both records saved once; exact keyboard Chinese/emoji/newlines, reopened values, upload hash and 238 unchanged neighbors. No note setter fallback. |
| wordpad | 166.47 | 30 / 32 / 83 | 24 | 4 / 15 | Exact four paragraphs, heading bold/body regular, Save As/reopen, native readText and unchanged seed; normal owned process exit 0. |
| explorer | 292.78 | 57 / 58 / 148 | 35 | 7 / 10 | Exact renamed/moved file tree and hashes, two unchanged neighbors, child/parent inspection and owned-window closure. |

Actual workbench, reopened document and Explorer content screenshots were visually inspected. The fixture does not journal Cancel events: both Cancel clicks and their subsequent workbench images were checked directly, with no extra save; a zero cancel-event count is not treated as proof. WordPad's partial UIA coverage remains native_read_gap and never proves complete traversal or uniqueness. Explorer uses a shared shell: the launch stub returned 1; read-only DTW inventory confirms the owned task window is absent while Explorer remains running. The shared shell was not killed. Chrome's owned window closed normally during supervisor cleanup. All Agent CLI processes returned 0; no candidate DTW process remained afterward. Failures and recovery calls are retained rather than removed from the task totals.

## Caelis #182 integration

Core is frozen at `3c3e09f336e5f919bf805fc05f77d458a5ab1e78`, binary SHA-256 `dae9b2259189cd13a539ccffc42d2f8408c0702b993f802bdd0952710664e16d`. The final WordPad task queries all thirteen API methods once before its first exec. The host persists **109,720 bytes**, two ordered text/JSON parts, SHA-256 `068325f4467f9e9dabd0f21b09cbec768b3c4a179f5b7230343179f53298e087`. Both parts contain all thirteen definitions; the text parses to exactly the structured JSON. The Agent reads the host artifact and continues for 30 GUI execs without session termination. No API batching workaround is used; the original 10,000-token canonical budget and strict Store validation remain.

Each full app task uses one unique app-access request for its entire Session. A separate actual Flash Session alternates A→B→A→B: exactly **two** unique requests, committed counters **1, 1, 2, 2**, no state reset across app switches. It executes five scripts: the first returns a non-JSON/undefined value and rolls state back; the four successful read-only scripts then complete without another approval. Its elapsed time is 27.24 seconds. Another Session takes 14.21 seconds, one exec/one fresh grant, counter **1**. Fixture files show no text/click/save/key effects; both fixtures exit normally with code 0. Approval lifecycle update events are not counted as additional requests.

These integration results require [Core PR #182](https://github.com/caelis-labs/caelis/pull/182) or equivalent fixes. Stock Caelis 0.75 retains the previously reproduced large-result rejection; DTW does not bypass host budgets or app approvals.

This round does not implement or modify eager/deferred exposure, plugin loading, dynamic tool refresh/retirement or their lifecycle. Core remains on the existing #182 branch. No global Caelis executable or configuration is replaced.

## Preserved unsuccessful attempts

- The earlier reviewed 3c80e238 binary failed lossless cancellation at transaction eight. Shorter passing runs do not override it.
- Frozen e10f205c / e3db634 produced correct Chrome final data through semantic setValue after three emoji keyboard admissions failed InvalidObservation. The required keyboard subgoal failed; the business-only pass is explicitly insufficient.
- On the same e10 candidate, the first WordPad Agent saved exact text but undid its already-bold heading with a second Ctrl+B. Screenshots show the intermediate bold heading; offline RTF contains no bold control afterward. A separate Agent repair restored heading bold/body regular without changing text, then saved/reopened/read back and closed normally. The initial attempt stays partial, separate from recovery and the final-candidate task.
- The first authorization follow-up used windows.map on the inventory object and rolled its state back; recovery used inventory.windows. It also performed an extra read-only access to the second owned app. No input/file effects occurred. Final authorization results are recorded separately above.
- Private development diagnostics and initial harness mistakes (exact Chrome title matching and a managed UIA diagnostic crash) are not counted as candidate passes. All output directories remain separate; original unknown inputs are never retried.

## Decision and limits

**Go for this covered Windows candidate matrix.** The reproduced Unicode cancellation and Chrome admission blockers are closed on the verified readable controls. All three selected real Agent tasks and the #182 authorization/result-recovery integration pass with independent effect checks. Package integrity and original user process preservation pass.

The next release must include DTW #72 and the necessary Core #182 fixes for this Caelis integration. Public release/merge remains a separate action; support claims must stay within the verified matrix. The previously failed candidates remain No-Go for their own bytes.

Mixed-monitor moves/scaling, other Windows versions/minimum-version qualification and elevated/custom editor coverage remain unverified. Background Windows input and secure desktop remain explicitly unsupported/deferred. This record does not expand those commitments, certify absent apps or rebuild/revalidate macOS GUI behavior. Existing macOS evidence remains tied to its accepted candidate. No PR is merged, tag/release published or signing/installation policy changed by this acceptance work.
