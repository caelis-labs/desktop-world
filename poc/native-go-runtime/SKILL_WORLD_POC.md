---
name: desktop-world-object-poc
description: Isolated DTW world-object and text-result experiment; not the installed product Skill.
---

# DTW object POC

Use the sole MCP tool **`exec`** with `operation:"exec"`, a caller-chosen short
`execution_id`, and JavaScript `code`. The result begins with that same ID and
contains one concise text block. Query `status`, `result`, or `cancel` with the
same ID. `result` with `detail:"full"` exposes original native receipts;
unknown delivery must be reconciled there, never retried under a new ID.

The desktop is `App → Window → Element`. Use exact names, and keep the returned
address unchanged across calls:

```javascript
const app = await dtw.app('Obsidian');
const win = await app.window('exact window title');
const button = await win.one({name:'exact control name'});
print(button);
```

When a title or control name is unknown, use `print(await app.windows())`
or `print(await win.find({role:'button'}))`. Results are bounded, readable
lines; narrow a query if the native scan reports incomplete coverage.

The text line includes the address and available semantic methods, such as
`W1/R1 新建笔记 · container · invoke, scrollIntoView`. The next script can call
`await dtw.at('W1/R1').scrollIntoView()`. A same-script `button.invoke()` also
works. `state.button = button` persists in this Session. Exact lookup refuses
ambiguous or incomplete observations; zero on an incomplete read is unknown.

`Element` methods: `read('value'|'name'|'role'|'states'|'checked'|'selected'|'expanded')`, `invoke`,
`setValue(text)`, `setChecked(bool)`, `setSelected(bool)`, `setExpanded(bool)`,
`scrollIntoView`, `focus`, `move`, `click({button,count})`, `dragTo(other)`,
`scroll({dx,dy})`, `press(key,modifiers)`, `typeText(text)`. `Window.capture()`
captures on demand. For a focus/key sequence use one native plan:

```javascript
await dtw.transaction(tx => {
  tx.focus(field);
  tx.press(field, 'A', ['primary']);
  tx.typeText(field, 'example');
});
```

The executor selects a proven background or short coordinated foreground route;
there is no transport/mode argument. An action receipt reports delivery and
verification. Physical delivery does not prove the App changed; read back when
the task needs that claim. Permission and grant setup stays with the trusted
host. This experimental Skill is usable only with `DTW_POC_TEXT_OUTPUT=1` and
the isolated POC runtime. macOS live acceptance and Windows remain separate
from this script contract.
