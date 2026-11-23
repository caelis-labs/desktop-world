---
name: dtw
description: "Operate desktop app interfaces through DTW: discover apps, inspect UI or screenshots, edit text, and use menus, file dialogs, mouse and keyboard. Use apps and app-scoped exec."
---

# DTW

Discover `apps` and `exec` on the connected `dtw` MCP server. The installed
plugin ID is `desktop-world`.
`exec` runs app-scoped JavaScript with these operations:
`look`, `capture`, `readText`, `wait`, `waitForNewWindow`, `click`, `setValue`,
`move`, `drag`, `typeText`, `keypress`, `scroll`, `focus`, `quit`.
Read [API.md](API.md) for exact parameters and outputs and
[PATTERNS.md](PATTERNS.md) for recovery and interaction patterns.

Call `apps({query})` and copy the selected row's `app` field into
`exec({app,source})`. Discovery reads only the application inventory. The `app`
field fixes the target of the entire async JavaScript body. Await every operation:

```javascript
await app.click({selector:{role:'button',text:'Continue'}});
await app.typeText({text:'Hello'});
```

Use `look({})` when inspection is needed. Known selectors, keyboard commands,
text insertion and default scrolling can start directly; DTW prepares the app
and receiver internally. A stopped app starts where supported (without requesting
foreground on macOS). Context survives between calls; do not prepend a redundant
look. Group known sequential actions in one script without
reading after every step. Use observed IDs or unique selectors. DTW revalidates
retained native objects and stops on relevant target changes instead of guessing
replacements. There is no Agent-managed Session, window binding or native handle.

Use `capture({})` when you need the actual pixels. An empty semantic tree can
still contain visible controls. Use coordinates from the returned PNG's native
dimensions, or pass `unit:'fraction'` to a pointer method with x/y in `[0,1)`.
Use `move({x,y})` for hover and `drag({path:[{x,y},...]})` for a drag within one
window on one display. Standard macOS/Windows title bars can move their window
when its native position is writable. DTW pairs releases.

Use `setValue({selector:{role:'text_field',text:'Field name'},value:'replacement'})`
for complete native text replacement when the control advertises `setValue`.
For typing events, first focus the intended editable control, then use
`typeText({text:'replacement'})`. For example, a Go to Folder path needs typing
to trigger completion; a Web form may need input events to enable Submit. A
`setValue` readback proves the native value, not those events or a completed save.
`keypress({keys:['ESC']})` sends one chord. Choose shortcuts for the app and OS.

For crowded pages, `look({role:'static_text',text:'relevant phrase'})` searches
native semantics beyond the default preview. On macOS and Windows,
`look({target:'w1.e12'})` can narrow to an observed subtree. Truncation is explicit.
When present, `elements[].value` contains the native control value. Read text with
`readText({target:'w1.e12'})`; in a script, return that page to publish it. Continue
with `{target:'w1.e12',offset:page.next_offset}` only while `next_offset !== null`,
without a fresh look between pages. Offsets are UTF-16; a fresh look starts a new
content observation.

Use `focus({})` when explicitly requested. Ordinary input prepares its receiver
automatically. macOS prefers working background capabilities and directly uses
foreground/real mouse input when necessary. Windows and Linux X11 use foreground input.
DTW leaves foreground, focus and pointer changes in place. Group known related
steps in one `exec` and inspect the current menu and receiver before further input.

After New/Open, use `waitForNewWindow({})` before typing into the new window.
It waits without repeating input; `wait_timeout` means no new window appeared.
For delayed content in the same window use `wait({title,role,text})` with only the
needed conditions. `wait({ms:200})` pauses. On Windows, `focused:true` identifies
an editable control with keyboard focus; `typeText({target,text})` can require it.

Scene reads return snapshots to the method caller; `readText` returns a text page;
input methods return receipts. The outer MCP response carries `effects`, a compact
`view` and any image. A custom script return selects `result` instead of the normal
view publication. For ordinary input, inspect the outer `view`; for script logic,
use `const view = await app.look({}); return view.elements;`; for native text,
use `return await app.readText({target:'w1.e12'});`. Returning `{done:true}` is
not verification. `status:completed` means settlement; check MCP `isError` /
`error` and observation status first. Dispatch and acknowledgements are distinct from observed UI
success. Verify the requested effect from the view or a readback. Unknown or
interrupted input may already have effects: inspect before new input, never replay
a script because its response was lost. Identical MCP request-ID
retransmission refers to retained work; a new ID is new work. Standard MCP
cancellation stops its matching call. Native cleanup failure stops the current
execution and affected native context. Later new requests let DTW retire/recover
the context and check admission again; never replay old input. Quit never
force-kills or discards documents.

Scripts can `print(...)` or return small JSON. Variables last for one execution;
imports, filesystem, network, process APIs and timers are unavailable. The deadline
is 30 seconds.

`permission_required` identifies missing OS access. Grant the host named by the OS.
On macOS, Screen Recording changes can require restarting it and reconnecting.

Linux requires an unlocked local native X11 desktop with an EWMH window manager,
XRes 1.2, XInput 2 and XTest. Wayland and XWayland return `wayland_unsupported`.
Use the executable path returned by `apps`. `look()` can start an eligible XDG
desktop entry. AT-SPI supports window-scoped controls, text reads, button presses
and text setters for a proven single window. Capture and pointer input require
unobstructed client pixels. Keyboard input uses the active group-zero keymap.
Menu scope, subtree queries and normal app quit are unavailable on Linux.
