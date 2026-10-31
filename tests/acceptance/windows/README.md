# Windows candidate handoff

This is an **opt-in GUI smoke for a Windows user**, not a Windows acceptance claim.
It has not been executed on a Windows desktop by its author. macOS syntax/type
checks do not establish Windows compilation, UIA behavior, input delivery or
native linking. The candidate's Windows build is a separate prerequisite.

## Run on an exclusive Windows desktop

Requirements: Windows 10 (1703 or newer)/11 with an interactive unlocked desktop, built-in
**Windows PowerShell 5.1** and .NET Framework, and the candidate `dtw.exe`.
No Rust, CUA, Visual Studio, downloaded package or administrator access is needed
by this fixture. The provider and fixture must run at the same normal integrity
level. Do not elevate the test to work around a failure.

Review the scripts and verify the candidate checksum first. Stop other desktop
automation, close windows that may cover the fixture positions, and avoid
physical input until completion. Use a primary working area at least 900 x 520
physical pixels. Existing windows may still cause `capture` to reject occlusion;
the test does not hide them or retry the action.

From this directory, in `powershell.exe` (not PowerShell 7 `pwsh`):

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\smoke.ps1 `
  -Dtw 'C:\Candidates\DTW\dtw.exe' `
  -OutputDirectory 'C:\Temp\dtw-smoke-first'
```

The output directory must not exist. This creates a small WinForms executable
locally, starts **two processes of that same executable**, and talks to one
`dtw.exe mcp` process. All GUI actions name only that owned app and its observed
window/element refs. It never resolves ambiguity by skipping a process. It does
not edit Agent configuration, call application internals through another
automation route, send input to existing user apps, or use clipboard fallback.

For the optional pointer smoke, use another new output directory:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\smoke.ps1 `
  -Dtw 'C:\Candidates\DTW\dtw.exe' `
  -OutputDirectory 'C:\Temp\dtw-smoke-pointer-first' -IncludePointerTests
```

That selects left/right/double click and three positive vertical wheel steps in
the owned pad. The fixture verifies mouse-up/double-click events and a wheel
delta of -360, the provider's current Windows mapping. It does not assert a
scroll distance in pixels or application content movement. A selected but
unsupported/denied/failed method makes the run fail; it is never counted as a
pass or automatically retried.

## What the automatic smoke checks

- Two fixture process instances aggregate under one stable executable app ID;
  their two titled windows have distinct public refs.
- Each instance uses `find` and selector-form `press` to resolve one `Increment`
  button, including its published `button Increment` label. Its counter must
  increase exactly once. Normal terminal UIA edges must yield complete coverage;
  a genuine UIA read failure must still be partial (the candidate also runs
  HRESULT boundary regressions, including `E_POINTER` and access denial).
- The `Message` field is discovered completely through pagination, set with
  selector-form `setValue`, and saved through its unique semantic Save button.
  Full native fields match separately; a field boundary is not searchable text.
- `focus`, Ctrl+A, Chinese + emoji + CRLF text entry, and Ctrl+S replace and save
  the document. Fixture state and the actual UTF-8 `saved.txt` are checked. Only
  CRLF versus LF is normalized for comparison; Unicode content is not normalized.
- Native `readText` reconstructs the Unicode sample in three-UTF-16-unit pages,
  checking offsets, forward progress, total length and exact text.
- A captured region retains one button or editor reference. The supervisor then
  moves the owned window or its controls. Old `press`, `setValue` and `readText`
  refs must be rejected before dispatch; text and counters must stay unchanged.
  These checks complete before the retained query can expire.
- Each instance's edits leave the other instance's saved text and action counters
  unchanged. This checks precise routing, not parallel operation.
- Both windows return pixel dimensions and a PNG MCP content block. **Inspect
  the PNGs yourself**: the automated check validates framing/signature/dimensions,
  not screenshot visual fidelity or all pixels' target attribution.
- Fresh fixture publications show no held key/button for at least 100 ms, and its
  key event journal contains paired downs/ups. These are fixture/session samples,
  not proof against future events or concurrent physical input.
- Normal MCP stdin closure and owned fixture stop requests complete without
  forced process termination. This is not an in-flight EOF/cancel lifecycle test.

Expected stale-region rejections are recorded as explicit negative controls;
only their specific stale/target-changed errors with no dispatch pass.
Every action is issued once. Read-only polling checks fixture readiness and
independent effects, with bounded waits; it never repeats a DTW command to make
an effect happen. Native errors, partial discovery, ambiguous controls, unknown
receipts and cleanup failures stop the run. Earlier receipts/files remain.

## Evidence and interpreting the outcome

`summary.json` records OS, PowerShell, source and binary SHA-256, selected cases,
pending manual coverage and cleanup. `smoke_passed` covers **only the selected
smoke**; `manual_pending` must remain visible in the handoff. Any failure returns
a nonzero script exit and `passed:false`; do not turn a later rerun into an
explanation of the original failure.

- `rpc-*-request.json`, `rpc-*-response.json`, `stdout.jsonl`, `stderr.log`: actual
  standard MCP transport, including original execution IDs and errors.
- `*-compact.json`, `*-ledger.json`: compact `exec` output and the complete
  original `result` ledger, fetched without replay.
- `rpc-*-image-*.png`: original PNG content from MCP.
- `A/`, `B/`: fixture snapshots, event journals and saved disk documents.
- `*-samples.json`, `final-A.json`, `final-B.json`: independent readiness,
  effects/release and final samples; `failure.txt` explains a failed run.

The transport uses bounded .NET pipe readers and deadlines, not POSIX selectors.
Output is capped (8 MiB/frame, 64 MiB total stdout, 2 MiB stderr). Cleanup owns
the exact processes it created: graceful close first, then bounded forced cleanup
if necessary; a force or unconfirmed cleanup invalidates the pass. It does not
enumerate/kill processes by name, repair permissions or synthesize release input.
On timeout, effects may be unknown: preserve evidence and inspect the owned
fixture and original receipt. Do not rerun an action to recover its response.

The smoke's PowerShell and C# sources are ASCII. The Unicode sample uses explicit Unicode code
points, source reads use UTF-8, MCP writes UTF-8 bytes, and fixture arguments use
Windows command-line quoting. A locale-dependent decode cannot silently change
both the expected and actual Chinese/emoji sample.

## Compilation without GUI (Windows CI)

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\smoke.ps1 `
  -CompileOnly -OutputDirectory 'C:\Temp\dtw-fixture-compile-new'
```

This compiles both C# sources, starts neither fixture nor DTW, opens no GUI and
needs no `dtw.exe` (the .NET compiler may run as a child process).
It emits `status:compiled_only`, `passed:false`: compilation is not a smoke pass.
The source directory is never modified. No extra CI job is required by this kit.


## Chrome workbench from the candidate ZIP

The ZIP includes `acceptance/browser-workbench/server.py` and `index.html` from
its recorded source. This optional local test server needs Python 3; DTW itself
needs no Python runtime. All fixture reads/writes explicitly use UTF-8, including
on a Chinese Windows installation. Start from the extracted candidate root in
PowerShell, using a new evidence directory:

```powershell
$work = 'C:\Temp\dtw-browser-first'
if (Test-Path -LiteralPath $work) { throw 'Use a new evidence directory' }
[void][IO.Directory]::CreateDirectory($work)
$utf8 = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $work 'review-evidence.txt'), "DTW Windows review`n", $utf8)
py -3 .\acceptance\browser-workbench\server.py --output $work --case windows-first --file-task
```

The server prints its loopback URL and stays running. Open that URL in a new
owned Chrome window/profile. Give the Agent the URL/window, its `apps` app ID,
and this task (replace the evidence path if needed):

> In DTW Workbench windows-first, set DTW-0088 quantity to 37 and DTW-0240 to 52.
> Mark both Reviewed. Set their Handoff notes respectively to two lines:
> `第一批：已核对 🐦` / `待补货`, and `第二批：已核对 🐦` / `待补货`.
> Save each record exactly once, reopen both and verify every value, then cancel
> without another save. Use Attach review evidence and the native file chooser
> to select `C:\Temp\dtw-browser-first\review-evidence.txt`; verify the received
> filename. Preserve all other records. Use DTW MCP and the shipped Skill for
> all application interaction, without shell, file editing, HTTP, DOM or CDP.

The supervisor independently reads state in another PowerShell terminal. These
commands are verification only; do not give them to the executing Agent:

```powershell
$work = 'C:\Temp\dtw-browser-first'
$utf8 = New-Object Text.UTF8Encoding($false)
$s = ConvertFrom-Json ([IO.File]::ReadAllText((Join-Path $work 'state.json'), $utf8))
$expected = @{
  'DTW-0088' = @{ quantity = 37; note = "第一批：已核对 🐦`n待补货" }
  'DTW-0240' = @{ quantity = 52; note = "第二批：已核对 🐦`n待补货" }
}
if (-not $s.ready -or @($s.items).Count -ne 240 -or @($s.saves).Count -ne 2) { throw 'Incomplete or duplicate saves' }
foreach ($sku in $expected.Keys) {
  $rows = @($s.items | Where-Object { $_.sku -eq $sku })
  $saved = @($s.saves | Where-Object { $_.sku -eq $sku })
  if ($rows.Count -ne 1 -or $saved.Count -ne 1) { throw ('Wrong record/save count: ' + $sku) }
  foreach ($row in @($rows[0], $saved[0])) {
    if ($row.quantity -ne $expected[$sku].quantity -or $row.reviewed -ne $true -or $row.note -cne $expected[$sku].note) { throw ('Wrong saved values: ' + $sku) }
  }
}
foreach ($row in $s.items) {
  if ($expected.ContainsKey($row.sku)) { continue }
  $number = [int]$row.sku.Substring(4)
  if ($row.quantity -ne (10 + $number % 25) -or $row.reviewed -ne $false -or $row.note -cne '') { throw ('Changed neighbor: ' + $row.sku) }
}
$sha = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $work 'review-evidence.txt')).Hash.ToLowerInvariant()
if (@($s.uploads).Count -ne 1 -or $s.uploads[0].name -cne 'review-evidence.txt' -or $s.uploads[0].sha256 -cne $sha) { throw 'Wrong or repeated attachment' }
$s.items | Where-Object { $expected.ContainsKey($_.sku) } | Format-List
$s.saves | Group-Object sku | Select-Object Name,Count
$s.uploads | Format-List
'Independent browser state passed; inspect the transcript/screenshots for reopening and chooser use.'
```

Keep `state.json`, the Agent transcript and original screenshots, plus the
candidate manifest/checksum and shipped Skill hash. Capture Agent wall time,
MCP/exec calls, failures and read-only recovery calls. File/server success alone
does not prove reopening or native chooser interaction. To stop only this owned
server gracefully, write its stop marker from the supervisor terminal:

```powershell
[IO.File]::WriteAllText((Join-Path $work 'stop'), 'stop', $utf8)
```

## Still required for full Windows acceptance

The real-application closure batch adds subtree/region observation, `readText`
and full bounded UIA text matching. Rebuild the final batch revision on Windows;
the earlier linked Windows candidate does not cover these changes. The Windows
archive includes the same-source batch plan in its docs directory; the plan is
an acceptance specification, not evidence that these tasks passed. Run the
existing smoke first (unique selectors, region invalidation, Unicode pages and
two-instance isolation), then validate DPI/monitor transitions and cancellation/
parent-exit cleanup. Proceed to the three real applications only after those
checks pass, using that same binary and Skill:

- Find text beyond the 512-character display excerpt. A search budget limit
  must return partial coverage, not an empty complete result.
- Discover a child container and observe its subtree; observe an image region
  using a fresh capture. Move/replace the source before use and confirm stale
  state is reported without input. Unsupported capabilities must match `apps`.
- Read Unicode/emoji text over multiple `readText` pages; verify UTF-16 offsets,
  the final length and original contents. Edit between pages and require a fresh
  observation instead of combining revisions. Unsupported UIA text patterns
  must not fall back to element names or descriptions as document text.
- Complete the WPS create/edit/Save As/close/reopen, Obsidian new-vault/folder/
  notes/search/link, and Chrome edit/readback/native-file-selection tasks in
  [the batch plan](../../../docs/real-task-closure-plan.md) through ordinary
  Agent + MCP + the shipped Skill. Use the stable Windows app IDs returned by
  `apps`, not macOS bundle IDs. The current `real_app_tasks.py` launcher is
  macOS-specific and must not be presented as a runnable Windows harness.
- Record per-task wall time, MCP/exec/native method counts, failures and recovery
  calls, every business subgoal, saved-file/server readback, and GUI evidence.
  A disk result alone does not prove that the Agent reopened or followed a link.

Do not infer Windows app-menu discovery, background input or foreground return
from the macOS implementation. Windows menu discovery is currently unsupported;
use its advertised basic window/keyboard/pixel route for those tasks.

Record these separately against the same candidate hash; none is implied by the
automatic smoke: cancellation during an input pair and a blocked UIA call;
total deadline/timeout with original unknown receipts; two independent MCP input
owners; killing/restarting one app instance while preserving sibling refs/state;
in-flight EOF and MCP parent exit with independently observed native worker
reclamation; 100/125/150/200% and mixed-monitor DPI including window movement;
permission/elevated/secure-desktop denial; screenshot visual fidelity; and real
application/ordinary Agent tasks. Preserve failures and permission denials as
their actual evidence class. Linux support is not tested by this directory.
