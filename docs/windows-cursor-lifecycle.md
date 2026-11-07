# Windows cursor presentation and MCP connection lifetime

Windows and macOS pointer feedback automatically hides after 60 seconds without
an accepted pointer update. The renderer uses monotonic time and retires that
presentation operation when its idle deadline expires. A delayed or reordered
packet cannot keep it visible or bring it back after retirement. The deadline is
checked before packet admission and before idle z-order maintenance/painting;
an expired cursor cannot be repeatedly shown by the renderer's ordering timer.

On Windows, only authenticated, in-order feedback for the current operation and
valid target coordinates refreshes activity. The last point stays visible while
another operation starts. Confirmed endpoints on the same window are joined by
a short visual movement, and content drags show the accepted native samples.
A successful accessibility click shows feedback when the element has a proven
visible point. The 200 ms click pulse starts when the marker reaches the target.

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

The cursor belongs to its MCP connection and has no public control API. Windows
clips it to the proven target bounds and maintains its order relative to that
window. On macOS the idle timer uses the same interval.
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

It creates an owned fixture and two ordinary `apps`/`exec` MCP connections.
The fixture independently verifies hover, coordinate and semantic clicks,
scroll, content drag and title-bar movement. The observer samples the visible
cursor through a transition and checks that read-only and failed scripts leave
the idle timer running. It also checks same-connection recovery, independent
session timers and isolated EOF cleanup.

Pure `Presentation` tests exercise accepted versus invalid/reordered packets,
the exact 60-second boundary, a late packet arriving before the next renderer
tick, retirement and independent clocks. These are the precise packet-level
controls; the MCP probe does not inject authenticated replay packets or expose
an internal endpoint for testing.

On 2026-11-01 the Windows candidate using the former two-second policy, with SHA-256
`8e38fb7166756638cbd12a7f6b771fc315ae62b326f3531006e0e540dc8cc9fd`
passed 435 release-profile library tests, three CLI tests, the new five-case
`idle-mcp-1` probe and the existing `cursor-lifecycle-1` suite. The new probe
observed hiding about 1.8–1.9 seconds after its first visible sample, with no
physical-pointer or foreground change during the idle interval. Both MCP
processes, both exact helper processes and the owned fixture exited normally;
the lifecycle suite reported no unresolved cases. Artifacts retain the frozen
binary, original ledgers, independent samples and cleanup outcomes. This is
scoped Windows fixture evidence, not final-package or arbitrary-host validation.
