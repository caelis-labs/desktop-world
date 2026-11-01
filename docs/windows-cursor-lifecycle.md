# Windows cursor presentation and MCP connection lifetime

Windows pointer feedback automatically hides after two seconds without an
accepted pointer update. The renderer uses monotonic time and retires that
presentation operation when its idle deadline expires. A delayed or reordered
packet cannot keep it visible or bring it back after retirement. The deadline is
checked before packet admission and before idle z-order maintenance/painting;
an expired cursor cannot be repeatedly shown by the renderer's ordering timer.

Only authenticated, in-order pointer feedback for the current operation and
valid target coordinates refreshes activity. Starting a new operation hides the
previous point and accepts new feedback normally. The click pulse still lasts
200 ms; pulse completion and idle retirement are separate events.

## A conversation is not an MCP connection

DTW already has internal session close and cursor hide operations. Normal MCP
stdin EOF, transport failure and parent-process loss close the server's owned
resources; cursor workers also watch their exact parent identity. Active
cancellation hides its presentation without destroying the shared session
helper. These cleanup paths remain in place.

Some hosts keep one `dtw mcp` process alive across conversations. DTW's current
session belongs to that connection, and tool requests do not carry a conversation
completion notification. Ending an Agent response therefore does not close the
connection or retire a successfully presented cursor. Previously, a valid target
could retain the last pointer point indefinitely.

Automatic idle hiding handles that visible residue without requiring the Agent
to call another tool. It keeps the connection, helper identity, application
state, retained references and original receipts. A later independent pointer
operation reuses the helper and displays feedback again. A live helper owned by
an open MCP connection is not treated as an orphan and is not terminated merely
to hide its window.

This change is confined to Windows presentation. It adds no public `close` tool,
Agent argument, input event, foreground activation, plugin lifecycle behavior or
macOS change. It never enumerates other applications' overlays to hide them.
`DesktopWorld.SessionCursor.v1` and `CodexComputerUseCursorOverlay` are distinct
window classes; class names alone do not prove process/session ownership.

## Verification

Run the opt-in probe on an exclusive unlocked Windows desktop, with Python 3,
Pillow and Windows PowerShell 5.1:

```powershell
python -X utf8 tests/acceptance/windows/cursor_idle_probe.py `
  --dtw 'C:\Candidates\DTW\bin\dtw.exe' `
  --output 'C:\Temp\dtw-cursor-idle-new'
```

It creates an owned fixture and two ordinary MCP connections. Independent
window/process and physical-pointer observations check successful and failed
script completion, read-only tool traffic, continuous idle hiding, same-connection
recovery, stable references/receipts and independent session timers. EOF closes
one helper while the other connection remains usable. No installed host plugin
or personal document is changed. The existing `cursor_probe.py` separately
regresses cancellation, EOF, parent loss, helper recovery, capture exclusion and
native input ownership.

Pure `Presentation` tests exercise accepted versus invalid/reordered packets,
the exact two-second boundary, a late packet arriving before the next renderer
tick, retirement and independent clocks. These are the precise packet-level
controls; the MCP probe does not inject authenticated replay packets or expose
an internal endpoint for testing.

On 2026-11-01 the Windows candidate with SHA-256
`8e38fb7166756638cbd12a7f6b771fc315ae62b326f3531006e0e540dc8cc9fd`
passed 435 release-profile library tests, three CLI tests, the new five-case
`idle-mcp-1` probe and the existing `cursor-lifecycle-1` suite. The new probe
observed hiding about 1.8–1.9 seconds after its first visible sample, with no
physical-pointer or foreground change during the idle interval. Both MCP
processes, both exact helper processes and the owned fixture exited normally;
the lifecycle suite reported no unresolved cases. Artifacts retain the frozen
binary, original ledgers, independent samples and cleanup outcomes. This is
scoped Windows fixture evidence, not final-package or arbitrary-host validation.
