# Native document-controller input fixture

This is a real AppKit `NSDocumentController` Open panel backed by `NSRemoteView`,
not a mock modal. The panel starts in an empty directory beside its unique owned
bundle. LaunchServices startup documents are closed during fixture setup; the
monotonic `created` count is never reset. Acceptance requires exactly one new
callback and a different visible document window after the tested click.
The activity token keeps the independent state publisher measurable in background.
No document is made dirty or saved.

Build with `swiftc -warnings-as-errors main.swift -o /absolute/DocumentProbe`.
Run `tests/acceptance/native_open_panel_probe.py --help` for the serial GUI probe.
It also needs the standard ComputerUse fixture and focus observer binaries.
Use a fresh output directory for every attempt. Inspect `mcp/open-panel.png`, then
write `point.json` with the actual button's `x`, `y` image coordinates and the
`capture_sha256` from `handoff.json`. There is no automatic screenshot-coordinate
oracle. Cases cover automatic borrow, explicit focus followed by click, and
`background-only` refusal. Input uses only the advertised `apps`/`exec` tools.

`macos_input_recovery_probe.py` covers fully occluded ordinary AppKit controls,
background text, `app.focus`, windowless shortcuts and subsequent text, closure
restoration and strict policy refusal. Its optional `--new-window-no-key` control
requires the first text to be refused without poisoning cleanup, then explicitly
focuses and edits that window in the following exec. Its coordinates come from
owned fixture geometry and do not claim model vision evidence.

Both probes preserve raw failures and cleanup outcomes. Actual sampling gaps are
diagnostic statistics, without a fixed failure threshold. Wrap a probe with
`cursor_audit.py --assert-policy` to require classified borrowing and cursor
restoration; `--assert-independent` additionally forbids any real cursor movement.
A successful borrow is explicitly distinguished from fully background operation.
