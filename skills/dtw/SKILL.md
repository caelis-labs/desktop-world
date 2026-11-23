---
name: dtw
description: "Operate desktop app interfaces through DTW: discover apps, inspect UI or screenshots, edit text, and use menus, file dialogs, mouse and keyboard. Use apps and app-scoped exec."
---

# DTW

The public MCP server is `dtw`. The installed plugin ID remains `desktop-world`;
in Caelis the full Skill identity is `desktop-world:dtw`.

Discover the connected MCP tools. `dtw mcp` exposes `apps` and `exec`.
`exec` runs app-scoped JavaScript with these operations:
`look`, `capture`, `readText`, `wait`, `waitForNewWindow`, `click`, `setValue`,
`move`, `drag`, `typeText`, `keypress`, `scroll`, `focus`, `quit`.
Read [API.md](API.md) for exact parameters and outputs and
[PATTERNS.md](PATTERNS.md) for recovery and interaction patterns.

Use `apps({query})` to find the authorization identity. Discovery reads no window
contents and does not itself authorize an app. `exec({app,source})`
fixes authorization for the entire async JavaScript body. Await every operation:

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

Use `capture({})` when semantics are unavailable or visual understanding is needed,
then choose coordinates from that image. A complete AX query with no elements
only describes that native tree; it does not prove the visible screen is empty. For a scaled preview use `unit:'fraction'`.
Use `move({x,y})` for hover and `drag({path:[{x,y},...]})` for a drag within one
window on one display. Standard macOS/Windows title bars can move their window
when its native position is writable. DTW pairs releases.

Use `setValue({selector:{role:'text_field',text:'Field name'},value:'replacement'})`
for complete native text replacement when the control advertises `setValue`.
For typing events, first focus the intended editable control, then use
`typeText({text:'replacement'})`. For example, a Go to Folder path needs typing
to trigger completion; a Web form may need input events to enable Submit. A
`setValue` readback proves the native value, not those events or a completed save.
`keypress({keys:['META','L']})` sends one chord. Existing JS string/number shorthand
remains a compatibility convenience; prefer the shared object form.

For crowded pages, `look({role:'static_text',text:'relevant phrase'})` searches
native semantics beyond the default preview. `look({target:'w1.e12'})` narrows to
an observed subtree without activating it. Truncation is explicit. Control values,
including sliders and checkboxes, are in `elements[].value`. Read native text with
`readText({target:'w1.e12'})`; in a script, return that page to publish it. Continue
with `{target:'w1.e12',offset:page.next_offset}` only while `next_offset !== null`,
without a fresh look between pages. Offsets are UTF-16; a fresh look starts a new
content observation.

Use `focus({})` when explicitly requested. Ordinary input prepares its receiver
automatically. macOS prefers working background capabilities and directly uses
foreground/real mouse input when necessary; Windows defaults to foreground input.
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

`permission_required` identifies missing OS access. Grant the host named by the OS;
Screen Recording changes can require restarting it and reconnecting. macOS and
Windows native behavior needs platform-specific evidence. Linux X11 discovers running
apps and reads window metadata and unobstructed client pixels. Use screenshot
coordinates for click, move, drag and scroll, and use mapped keyboard input. Linux
has no AT-SPI semantics, native `readText`, or installed-app launch/quit. Keep the
local X11 desktop unlocked. Wayland and XWayland are unsupported.
