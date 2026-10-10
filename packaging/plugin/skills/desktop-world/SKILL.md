---
name: desktop-world
description: Use DTW's native MCP exec tool to inspect and operate desktop apps with JavaScript.
---

# Desktop World

Use the sole MCP tool `exec`. Give each script a short `execution_id` and
`operation:"exec"`. The same tool accepts `status`, `result`, and `cancel` with
that ID. Keep the ID when an outcome is unknown; inspect its original result
and never repeat the action under a new ID.

The script has `dtw`, `state`, and `print`. `state` persists within this MCP
connection. `print` is the model-facing result; native observations and full
receipts remain available with `result` and `detail:"full"`.

```javascript
const app = await dtw.app('Obsidian');
const window = await app.window('exact window title');
print(await window.find({role:'button'}));
```

The printed address, such as `W1/B2`, is directly usable in a later call:

```javascript
await dtw.at('W1/B2').click();
```

Use exact names to select one control with `window.one({name:'...'})`. The
available element methods are shown beside each address. They include `read`,
`invoke`, `setValue`, `setChecked`, `setSelected`, `setExpanded`,
`scrollIntoView`, `focus`, `move`, `click`, `dragTo`, `scroll`, `press`, and
`typeText`. `window.capture()` saves a window image; query `result` with
`include_image:true` to receive it as MCP image content.

Use `dtw.transaction(tx => { ... })` to group predictable input into one plan.
The executor chooses the background route when available and a brief
foreground route otherwise. There is no mode or transport selector. A
`dispatched` receipt says the event was sent; read back app state when the task
requires proof of its effect. An incomplete observation cannot prove absence.
After a window closes or an app restarts, reacquire its address by observing.

DTW refuses actions in terminal applications. OS desktop permissions still
apply; the embedding application controls its own user authorization.
