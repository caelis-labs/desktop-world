---
name: dtw
description: Control one authorized desktop application through DTW using compact semantic observations, app screenshots, mouse and keyboard operations.
---

# DTW

The public MCP server is `dtw`. The installed plugin ID remains `desktop-world`;
in Caelis the full Skill identity is `desktop-world:dtw`.

Discover/list the connected MCP tools before choosing how to operate. Default
`dtw mcp` exposes `apps` and `exec`. If `exec` is listed, use app-scoped JavaScript.
With `dtw mcp --atomic`, invoke the listed app operation directly with an explicit
`app`; there is no JavaScript tool. Choose from actual tool discovery, never the
OS or a guessed operation name. Both surfaces use the same names and object fields:
`look`, `capture`, `readText`, `wait`, `waitForNewWindow`, `click`, `setValue`,
`move`, `drag`, `typeText`, `keypress`, `scroll`, `focus`, `quit`.
Read [API.md](API.md) for exact parameters and outputs and
[PATTERNS.md](PATTERNS.md) for recovery and interaction patterns.

Use `apps({query})` to find the authorization identity. Discovery reads no window
contents and does not itself authorize an app. In script mode, `exec({app,source})`
fixes authorization for the entire async JavaScript body. Await every operation:

```javascript
await app.click({selector:{role:'button',text:'Continue'}});
await app.typeText({text:'Hello'});
```

In atomic mode, call the listed tools with argument objects, for example:

```json
{"name":"click","arguments":{"app":"<discovered identity>","selector":{"role":"button","text":"Continue"}}}
```

```json
{"name":"typeText","arguments":{"app":"<discovered identity>","text":"Hello"}}
```

Use `look({})` when inspection is needed. Known selectors, keyboard commands,
text insertion and default scrolling can start directly; DTW prepares the app
and receiver internally. A stopped app starts where supported (without requesting
foreground on macOS). Context survives between calls; do not prepend a redundant
look. In the default mode, group known sequential actions in one script without
reading after every step. Use observed IDs or unique selectors. DTW revalidates
retained native objects and stops on relevant target changes instead of guessing
replacements. There is no Agent-managed Session, window binding or native handle.

Use `capture({})` when semantics are unavailable or visual understanding is needed,
then choose coordinates from that image. For a scaled preview use `unit:'fraction'`.
Use `move({x,y})` for hover and `drag({path:[{x,y},...]})` for a drag within one
window on one display. Standard macOS/Windows title bars can move their window
when its native position is writable. DTW pairs releases.

Use `setValue({selector:{role:'text_field',text:'Field name'},value:'replacement'})`
for complete native text replacement when the control advertises `setValue`.
Use `typeText({text:'replacement'})` for typing events.
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

Use `focus({})` when explicitly requested. Ordinary input needs no separate focus
sequence. Keep related focus/input in one `exec`. On Windows, opening a native
menu and choosing its item must share that script boundary: restoring foreground
at settlement can dismiss the menu. Every atomic call has its own boundary; separate
atomic calls do not preserve that focus/menu scope.

After New/Open, use `waitForNewWindow({})` before typing into the new window.
It waits without repeating input; `wait_timeout` means no new window appeared.
For delayed content in the same window use `wait({title,role,text})` with only the
needed conditions. `wait({ms:200})` pauses. On Windows, `focused:true` identifies
an editable control with keyboard focus; `typeText({target,text})` can require it.

Scene reads return snapshots to the method caller; `readText` returns a text page;
input methods return receipts. The outer MCP response carries `effects`, a compact
`view` and any image. A custom script return selects `result` instead of the normal
view publication. Dispatch and acknowledgements are distinct from observed UI
success. Verify the requested effect from the view or a readback. Unknown or
interrupted input may already have effects: inspect before new input, never replay
a script or atomic action because its response was lost. Identical MCP request-ID
retransmission refers to retained work; a new ID is new work. Standard MCP
cancellation stops its matching call. Native cleanup failure blocks input in that
connection. Quit never force-kills or discards documents.

Scripts can `print(...)` or return small JSON. Variables last for one execution;
imports, filesystem, network, process APIs and timers are unavailable. The deadline
is 30 seconds. Atomic calls execute the same native app facade without JavaScript;
session state, stale-target checks, cancellation and result effects still apply.

`permission_required` identifies missing OS access. Grant the host named by the OS;
Screen Recording changes can require restarting it and reconnecting. macOS and
Windows native behavior needs platform-specific evidence. Linux is experimental,
without AT-SPI semantics; isolated X11 evidence does not establish locked-desktop
support. Keep supported desktops unlocked.
