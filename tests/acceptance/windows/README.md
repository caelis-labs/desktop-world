# Windows desktop acceptance

These opt-in diagnostics exercise the current `apps` / `exec({app,source})`
protocol. CI compilation and MCP metadata checks do not establish GUI acceptance.
Record the exact binary, source and shipped Skill with each run; results for one
build, application or DPI layout do not establish another one's behavior.

Use an unlocked ordinary Windows desktop, Windows PowerShell 5.1/.NET Framework
and Python 3. Run probes sequentially and avoid concurrent physical input. The
feature audit additionally requires Pillow for independent PNG pixel checks.
No Rust toolchain, CUA runtime, administrator access or clipboard is required by
the installed probes. Use a **new output directory** for every attempt.

From this directory in an extracted package, or its corresponding source folder:

```powershell
python -X utf8 .\app_protocol_probe.py `
  --dtw '<package>\bin\dtw.exe' --installed-package '<package>' `
  --output '<new-baseline-directory>'

python -X utf8 .\feature_audit.py `
  --dtw '<package>\bin\dtw.exe' --installed-package '<package>' `
  --output '<new-feature-directory>'

python -X utf8 .\browser_feature_audit.py `
  --dtw '<package>\bin\dtw.exe' `
  --chrome 'C:\Program Files\Google\Chrome\Application\chrome.exe' `
  --output '<new-browser-directory>'
```

For a development binary, omit `--installed-package`. The historical `--chrome`
argument also accepts an installed Edge executable. Browser tests use a temporary
guest profile and a loopback page; they check the owned top window before input.
They do not automate an existing user's browser document.

| Entry | Independently checked contracts |
| --- | --- |
| `app_protocol_probe.py` | App admission, current ten-method surface (including `readText`), complete fixture semantics, returned-view deduplication, bounded unchanged delta, semantic action, Unicode/keyboard/save bytes, native modal bounds/order and covered opaque-window composition |
| `feature_audit.py` | Installed/running discovery and provider metadata; semantic IDs and selectors; left/right/double click; hover/content/title-bar drag; vertical/horizontal/default wheel effects; stale geometry; native menu; modal keyboard routing; rejected selector foreground invariants; two-instance continuity and overlapping pixels; global layered alpha; normal quit/save cancellation; cancellation/retransmission; stopped executable startup |
| `browser_feature_audit.py` | Empty-field Unicode, selection replacement, terminal caret, literal label/U+FFFC content, multiline text, semantic save/click, both wheel axes, independently owned native Open dialog, Escape and normal browser quit |

The WinForms fixture publishes atomic state and an event journal; the observer
shares deletion so reading does not interfere with replacement. Business effects
are checked against counters, exact UTF-8 saved bytes, physical foreground flags
and screenshot pixels. Browser effects come from the page's independent HTTP
journal, with a read-only Win32 proof for its native file dialog. Dispatch or an
image signature alone never passes an effect assertion.

`report.json` records pass/failure, the first blocked stage and cleanup. Preserve
all failed attempts and wire frames. No failed input is automatically replayed;
read-only readiness/effect polling is bounded. A retransmission of the identical
MCP request ID inspects its retained execution. Forced termination of an exact
owned test process is recorded as cleanup failure, never an ordinary passing run.
Normal cleanup requests close only the fixtures/profiles created by that run.

## Shared workbench and development diagnostics

The source checkout also contains the Windows native shell of the shared
[Computer Use workbench](../../fixtures/computer-use-app/README.md). It reuses
`Fixture.cs`, the portable Web/Canvas servers, the 42-scenario manifest and owned
lifecycle. `computer_use_probe.py` checks cold input, dense focused discovery,
Unicode commit, exact long-text UTF-16 paging, stale reads, checkbox, read-only
menu, same-instance modal sequences and real Open/Save/overwrite cancellation.
Unselected scenarios remain `not_run`; this is a direct-script core, not the
complete macOS benchmark or model image-channel acceptance.

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/acceptance/windows/compile-cursor.ps1 -OutputDirectory C:\Temp\dtw-observer-new
python -X utf8 tests/acceptance/windows/observe_run.py --observer C:\Temp\dtw-observer-new\CursorObserver.exe --output C:\Temp\dtw-observed-new -- python -X utf8 tests/acceptance/windows/computer_use_probe.py --dtw '<candidate>\dtw.exe' --fixture-binary C:\Temp\dtw-observer-new\Fixture.exe --output C:\Temp\dtw-core-new
```

`observe_run.py` uses a separate native process to retain foreground HWND/PID,
real cursor position and global held-key samples. Actual gaps above 50ms fail the
strict observation check even if business effects pass. Sampling records Windows
foreground input honestly; it does not grant the macOS pointer-borrow policy to
Windows or prove zero interference for foreground routes.

`input_desktop_probe.py` launches an owned fixture and the actual native worker
on an invisible, non-input desktop. It checks nine new-input admission refusals,
unchanged business state and normal process/desktop cleanup without switching
desktops, locking Windows or changing ACLs. It is a native-boundary diagnostic;
it does not claim to induce secure-desktop or mid-dispatch desktop loss.
`interruption_probe.py --mode cancel|eof|host-death|deadline` requires an actually
observed held drag, exactly one paired up, no suffix, and worker shutdown. Each
attempt needs a fresh output directory. These development-only probes require the
source tree and are not installed-package entrypoints.

The browser audit no longer parks the pointer before opening a chooser. It
verifies exact selected Unicode file contents through the independent page.
`--review-chooser-hover` additionally publishes `hover-request.json` with the
actual PNG/hash and waits for `hover-answer.json` (`sha256`, image-pixel `x`, `y`).
The reviewer must inspect that PNG; the probe then hovers the visible file and
requires a real `tooltips_class32`/`SysShadow` before capturing and selecting it.
No unknown popup or menu is excluded by this diagnostic.

Windows capture uses exact HWND rendering and DWM visible frames. It can read a
covered opaque window, but input still requires visibility and the exact receiver.
Global alpha/color-key surfaces require readable native proof; unsupported
per-pixel layered alpha remains explicit. GUI coverage does not extend to locked,
elevated or secure desktops, every display layout, every UIA provider or arbitrary
cross-process system helpers.

Current user-facing arguments and platform conditions are defined by the bundled
`skills/desktop-world/API.md`. Dated evidence and the earlier failure matrix are
available in the repository's
[Windows audit](https://github.com/caelis-labs/desktop-world/blob/main/docs/windows-feature-audit-2026-11-03.md)
and [host acceptance history](https://github.com/caelis-labs/desktop-world/blob/main/docs/windows-host-acceptance-2026-11-01.md).
Historical atomic probes used `result`, window references and a different script
surface. They are retained as source history/regression specifications and are not
current package entrypoints or commands to run against the public two-tool API.
