---
name: desktop-world
description: Use DTW to inspect and operate local desktop apps through its single MCP exec tool. Use for desktop UI tasks involving windows, controls, screenshots, mouse, or keyboard.
---

# Desktop World

Call the DTW MCP server's **`exec`** tool. Its `code` is JavaScript with
`dtw`, persistent `state`, `print`, `async`/`await`, loops, and exceptions.
Give each call a short, unique `execution_id`; the result starts with that ID.
`print(...)` supplies script findings; DTW also adds concise action and error
lines. Print the facts needed for the next decision, not the entire UI tree or
a native receipt.

Start with an exact app name, then discover its windows:

```json
{"operation":"exec","execution_id":"windows-1","code":"const app = await dtw.app('Obsidian'); print(await app.windows())"}
```

The result lists addresses such as `W1`. Use the printed address directly in
the next script. Addresses belong to this MCP connection and expire when the
underlying app/window changes:

```json
{"operation":"exec","execution_id":"buttons-1","code":"const win = dtw.at('W1'); print(await win.find({role:'button',nameContains:'New'}))"}
```

Use `win.one({name:'exact label'})` when one control should match. The
returned element has an address such as `W1/B2`. Its printed line shows its
name, role, and available behaviors. `activate()` invokes the semantic
behavior when supported, otherwise makes one click. Other methods include
`read()`, `invoke()`, `setValue(text)`, `setChecked(bool)`,
`setSelected(bool)`, `setExpanded(bool)`, `scrollIntoView()`, `focus()`,
`click()`, `move()`, `dragTo(target)`, `scroll({dx,dy})`,
`press(key, modifiers)`, and `typeText(text)`. `win.capture()` takes a
window screenshot.

```json
{"operation":"exec","execution_id":"click-1","code":"await dtw.at('W1/B2').activate()"}
```

DTW chooses background delivery first when supported and coordinates
foreground delivery when needed. Scripts have no mode switch. A delivered
input receipt proves dispatch, not the app's resulting state; read the relevant
control or window again when the task needs effect confirmation. A partial
observation cannot prove that a control is absent. Reacquire addresses after
a window closes or an app restarts.

For a screenshot, execute `await win.capture()`, then call the **same**
`exec` tool with `{"operation":"result","execution_id":"capture-1",
"include_image":true}`. For a long execution, use `status` or `cancel`
with its original ID. If delivery or completion is unknown, query `result`
with that ID (and `detail:"full"` if necessary); never replay the action
with a new ID just to see whether it worked. `state` persists only within
this MCP connection.

DTW blocks actions in terminal applications. Operating system desktop
permissions still apply; the embedding application owns user authorization.
