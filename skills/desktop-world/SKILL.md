---
name: desktop-world
description: Control one authorized desktop application through DTW using compact semantic observations, app screenshots, mouse and keyboard scripts.
---

# Desktop World

DTW exposes two MCP tools: `apps` and `exec`. Use `apps({query})` to find an
installed or running app and its authorization identity. Run an async JavaScript
body with `exec({app, source})`; `app` is always required and fixes the entire
script's authorization scope.

Use `await app.look()` when the task needs inspection. Known selectors, keyboard
commands, text insertion and default scrolling can start a script directly; DTW
prepares the app and current receiver internally. A stopped app starts where
supported (without requesting foreground on macOS). App context survives between
`exec` calls, so do not prepend a redundant look to each script. Explicit looks
return complete snapshots inside JavaScript; tool messages show a compact baseline,
then changes. Use semantic target IDs from a returned observation or a unique
selector to click known controls. Native object handles can survive filtered reads: DTW revalidates the exact object before input, without guessing a replacement.
Use `await app.capture()` when the UI needs visual understanding or semantics
are unavailable, then click coordinates in the returned image. After input DTW
returns a fresh observation automatically. Use `move({x,y})` for hover and
`drag({path:[{x,y},...]})` for one drag within one window on one display; standard macOS/Windows title bars can
move their window when its native position is writable;
the runtime pairs releases. Group known sequential steps in one script without
adding a read after every action: DTW renews bindings internally and stops when a
target or its relevant state changes. `click('w1.e12')` selects an observed ID;
`keypress('META+L')` sends a chord. Await every app operation.

```javascript
await app.click({selector:{role:'button',text:'Continue'}});
```

Use `setValue({selector:{role:'text_field',text:'Field name'},value:'replacement'})` for a complete native text value replacement when the control advertises `setValue`. For typing events use `typeText` instead.

For crowded pages, use `look({role:'static_text',text:'relevant phrase'})` to
query native semantics beyond the default preview. `look({target:id})` narrows to an observed subtree, such as the File menu, without activating it. Truncated fields are explicit.
Read control values (including sliders and checkboxes) from `look().elements[].value`.
Read native textual content on demand with `return await app.readText('w1.e12')`;
Continue with `{target:'w1.e12',offset:page.next_offset}` without a new look
only while `page.next_offset !== null`. A null next offset marks the end, not zero.
Text offsets are UTF-16; a fresh look starts a new content observation.

Use `await app.focus()` when the task explicitly asks to focus the app/window.
Keep related input in the same script. Ordinary input prepares its destination
automatically; no separate focus sequence is needed.

After a New/Open action, use `await app.waitForNewWindow()` before typing in the new
window. It waits without repeating the action; `wait_timeout` means no new window appeared.

The public methods are `look`, `capture`, `readText`, `waitForNewWindow`, `click`, `setValue`, `move`, `drag`, `typeText`,
`keypress`, `scroll`, `focus` and `quit`. Read [API.md](API.md) for exact arguments and
output semantics;
[PATTERNS.md](PATTERNS.md) covers visual fallback, dialogs and recovery.
There is no Agent-managed Session, window binding, cursor or native reference.
Use optional `print(...)` or return a small JSON value for task-specific output.
Ordinary variables last for one execution. Imports, filesystem, network, process
APIs and timers are unavailable; scripts have a 30-second deadline.

A completed script or dispatched action does not establish the requested app
effect. Inspect the returned changes, read back, or capture when necessary.
Unknown/interrupted input may already have effects: inspect before new input;
never replay a script merely because its response was lost. Native cleanup
failure blocks further input in that connection. Standard MCP cancellation stops
its matching call; quitting never force-kills or discards documents.

`permission_required` identifies missing OS access. Grant the host named by the
OS; Screen Recording changes may require restarting that host and reconnecting.
Discovery reads no window contents and does not itself authorize an app.
