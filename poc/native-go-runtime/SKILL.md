---
name: desktop-world-native-go-poc
description: Use the isolated native Go Desktop World POC through its one MCP exec tool.
---

# Desktop World core POC

Use the one MCP tool, `exec`. Set `operation` to `exec`, `status`, `result`, or
`cancel`. Always supply a short, unique `execution_id`; use that same ID to
query or cancel an execution. For `exec`, supply JavaScript in `code`.

The default MCP result is
one short `content.text` block with the execution ID and printed facts; routine
`structuredContent` is absent. A window capture adds an image only when the
original execution is queried with `operation:"result", include_image:true`.
Use `detail:"full"` on that original result only when its receipt is needed.

JavaScript has persistent `state`, `print`, `await`, loops, filters, exceptions,
and the `dtw` object. The core path is App → Window → Element:

```javascript
const app = await dtw.app('Obsidian');
const win = await app.window('exact window title');
const button = await win.one({name:'新建笔记'});
state.button = button;
print(button); // e.g. W1/B1 新建笔记 · button · invoke
```

The next `exec` can use `await state.button.invoke()` or
`await dtw.at('W1/B1').invoke()`. For several actions, use
`await dtw.transaction(tx => { tx.scrollIntoView(state.button); tx.invoke(state.button); })`.
Available
Element methods include `read`, `focus`, `invoke`, `setValue`, `setChecked`,
`setSelected`, `setExpanded`, `scrollIntoView`, `move`, `click`, `dragTo`,
`scroll`, `press`, and `typeText`; `win.capture()` requests a window image.
Check the behaviors printed for the observed element before acting. The
execution layer chooses semantic background or coordinated foreground input.
Scripts do not select a transport or input mode.

Use `win.find({name:'...'})` or `win.find({nameContains:'...',role:'...'})` for
narrow discovery. `win.one({name:'...'})` requires exactly one match. A large
AX tree stays inside the script: filter it and print only facts needed for the
next decision. Low-level `dtw.observe`, `read`, `sync`, `act`, and `capture`
remain available for behavior not yet wrapped by the object surface.
`dtw.disclose` exposes changed fields with short Session-local addresses;
unchanged nodes do not need to be printed again. Addresses expire when their
native epoch changes. Incomplete observation is unresolved, even with zero
matches; narrow or refresh before an action.

DTW's own grant, declaration, and revocation API is absent from this core POC.
The helper must be built with `dtw_poc_noauth`; the MCP server rejects a normal
helper. Operating-system Accessibility, Screen Recording, and input permission
checks still apply. Select only the intended App and window.

An action receipt reports the chosen route, delivery, verification, and
restoration. Dispatch alone does not prove that an App handled the action;
read back the intended effect. For cancellation, partial completion, unknown
delivery, late receipts, or lost script state, query `result` with the original
`execution_id`. Never replay an action whose delivery is unknown. A script is
limited to 64 KiB, 8 KiB printed text, 32 native calls, and 128 retained
executions per Session. This Skill describes a POC, not a product runtime.
