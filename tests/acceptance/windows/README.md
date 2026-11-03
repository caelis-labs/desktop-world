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
| `app_protocol_probe.py` | App admission, nine-method surface, complete fixture semantics, returned-view deduplication, bounded unchanged delta, semantic action, Unicode/keyboard/save bytes, native modal bounds/order and covered opaque-window composition |
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
