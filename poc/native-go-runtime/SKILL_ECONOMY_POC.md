---
name: desktop-world-economy-poc
description: Use the isolated DTW Go POC to inspect an explicitly selected desktop App through its sole exec tool.
---

# Desktop World economy POC

Use only `dtw_poc.exec` for desktop work. Every call has `operation:"exec"`, a unique short `execution_id`, and JavaScript `code`. `state` persists across calls. `dtw.observe`, `dtw.grants`, `dtw.act` and `dtw.capture` are async. `print` only the facts needed for the next decision; full native observations and receipts stay inside the execution record. `operation:"result"` queries the original ID if an outcome is partial, unknown, cancelled or failed. Never replay an uncertain action.

For an explicitly approved window, read its active exact grant and keep `grant.application` inside JavaScript. Query only that App for the exact window title. A `name` is `{status,value}`. Require one matching `kind:"window"` and `coverage.complete===true`, without dirty or truncated coverage. If the first lookup is incomplete, try **one** narrower `summary` lookup; if still unresolved, stop. Then query that window with `projection:"outline"`, `match:{within:window.ref,name_equals:targetName}`, and bounded `max_results`, `max_visited_nodes`, `max_depth`, `read_deadline_ms`. Require one exact control and clean complete coverage. Query `capabilities` only when needed. `dtw.index(windowObservation)` then `dtw.disclose(controlObservation,{fields:["kind","role","name","capabilities"]})` gives a short Session-local control `id`; the role letter in this experimental ID is only a hint. Print the ID, role, exact target name, and supported **available** actions in one line. Never print unrelated AX nodes, the native Ref, timestamps or the full grant record.

Use this shape for one batched lookup; replace only the title and target name:

```javascript
const title = 'exact approved window title', targetName = 'exact target name';
const grants = (await dtw.grants()).grants ?? [];
const active = grants.filter(g => g.window_title === title &&
  g.state === 'active' && g.application);
if (active.length !== 1) throw Error('grant unresolved');
const app = active[0].application;
let win = await dtw.observe({scope:{refs:[app]}, projection:'outline',
  fields:['name','role','app'], match:{within:app,name_equals:title},
  budget:{max_results:16,max_visited_nodes:512,max_depth:12,read_deadline_ms:4000}});
if (win.coverage?.complete !== true || win.coverage?.dirty)
  win = await dtw.observe({scope:{refs:[app]}, projection:'summary',
    fields:['name','role','app'], match:{within:app,name_equals:title},
    budget:{max_results:16,max_visited_nodes:128,max_depth:4,read_deadline_ms:4000}});
const windows = win.objects.filter(o => o.kind === 'window' &&
  o.name?.status === 'known' && o.name.value === title);
if (win.coverage?.complete !== true || win.coverage?.dirty ||
    win.coverage?.truncated || windows.length !== 1) throw Error('window unresolved');
dtw.index(win);
const target = await dtw.observe({scope:{refs:[windows[0].ref]},
  projection:'outline',fields:['name','role','capabilities'],
  match:{within:windows[0].ref,name_equals:targetName},
  budget:{max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000}});
const hits = target.objects.filter(o => o.name?.status === 'known' &&
  o.name.value === targetName);
if (target.coverage?.complete !== true || target.coverage?.dirty ||
    target.coverage?.truncated || hits.length !== 1) throw Error('target unresolved');
const id = dtw.disclose(target,{fields:['kind','role','name','capabilities']}).items[0].id;
const usable = (hits[0].capabilities ?? []).filter(c =>
  c.support === 'supported' && c.availability === 'available').map(c => c.name);
print(`${id} ${hits[0].role} "${targetName}" ${usable.join(',')}`);
```

If a task requires an action, use the short disclosed ID as `target:{id:"W1/R1"}` only after clean complete observation. The execution layer selects background or a short coordinated foreground route. An unsupported or unknown route never silently falls back to foreground. Check the authoritative native action summary; physical delivery alone does not verify App effect. Use a separate scoped readback when the task needs proof of effect. Window capture uses a `capture_windows` candidate, which may differ from the AX action Ref; request the image only through `result` with `include_image:true`.

The server returns task facts in short `text` and routine `structuredContent` contains only state in this experiment. A clean result can omit the original execution ID because it is already in the tool arguments; incomplete/unknown results include it. Full original details stay under that ID. This POC is macOS-only and does not pass the product phase gate.
