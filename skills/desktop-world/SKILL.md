---
name: desktop-world
description: Operate desktop apps through DTW MCP with app-scoped JavaScript, filtered native discovery, screenshots, text input and shortcuts. Use for GUI tasks through DTW; batch known actions and verify their effects inside each script.
---

# Desktop World

Use the configured DTW MCP server's `apps`, `api`, `exec`, `result` tools (the client may prefix their names). Start with `apps` for stable app IDs, method signatures and platform conditions. Execute with exactly `{app, source}`. `source` is an async JavaScript function body with `app`, `state`, `await` and `return`. Send the original source string serialized once; no filename or double JSON encoding.

**Work in complete UI steps.** Return to the model only when new labels, state or an image require a decision. Once the next operations are known, run them sequentially in the same script, including verification. For repeated forms, discover the labels once, then process the requested records in one loop. Do not rediscover the surrounding table between records. Input methods focus internally; a separate focus call is usually unnecessary.

**Use labels supplied by the task.** If the user already names the fields to edit, do not open a form just to list those labels again. The batch's exact-name queries validate them against the live UI before acting. Usually select the window and discover its search field in one exec, then run the whole edit/save/readback loop in the next. Use the optional form-discovery example only for labels or control types that are still unknown.

**Finish every query before deciding.** This includes filtered queries for a known, exact label. `limit` limits returned matches; it does not guarantee that the native tree was fully scanned. While `page.next` exists, collect the page and call `app.observe({cursor:page.next})` inside the same script. Only after the last page should you check `coverage.complete` and whether there is exactly one match. A first page with zero or one matches and a continuation is neither missing nor ambiguous. Use the paginated helper in section 2 for every control lookup.

Results and committed state must be JSON. Project documented element fields: `role`, `name`, `value`, `enabled`, `actions` and refs. There is no `description` field. Omit absent fields or use `?? null`; returning `undefined` inside an object fails serialization. Continue through the requested edits and saves: an intermediate discovery or screenshot is not task completion.

## 1. Discover the relevant controls

For a browser table/form, start with **window selection plus `role:'text_field'` discovery in the same exec**. Find the search/filter field before searching the entire tree for a record keyword. Do not start by dumping an unfiltered browser window or all matching table rows. Adapt this example to the task:

```javascript
const matches = (await app.windows()).windows.filter(w => w.title === 'Task window');
if (matches.length !== 1) throw Error('Identify the intended window');
state.window = matches[0].window_ref;
let p = await app.observe({window_ref:state.window, role:'text_field', limit:12});
const fields = [];
for (;;) {
  fields.push(...p.elements.map(e => ({name:e.name, value:e.value})));
  if (!p.next) break;
  p = await app.observe({cursor:p.next});
}
return {fields, complete:p.coverage.complete};
```

If the task already provides the form labels, go directly to section 2. Otherwise combine filtering, identifying the edit control, opening the form, discovering its fields and closing it without saving in one script. This optional example uses an observed search label and requires an actually discovered, exact edit label before pressing it. If that label is absent or ambiguous, return compact choices for a decision; do not guess a different action.

```javascript
const window_ref = state.window, key = 'Contact A';
async function query(role, text) {
  let p = await app.observe({window_ref, role, ...(text ? {text} : {}), limit:12});
  const elements = [];
  for (;;) {
    elements.push(...p.elements);
    if (!p.next) break;
    p = await app.observe({cursor:p.next});
  }
  if (!p.coverage.complete) throw Error('Incomplete control discovery');
  return {elements, observation_ref:p.observation_ref};
}
function ref(q,e) {
  return {window_ref, observation_ref:q.observation_ref, element_ref:e.element_ref};
}
let q = await query('text_field','Search contacts');
let found = q.elements.filter(e => e.name === 'Search contacts' && e.actions.includes('setValue'));
if (found.length !== 1) throw Error('Identify the search control');
await app.setValue({...ref(q,found[0]), value:key});
q = await query('button',key);
found = q.elements.filter(e => e.name === 'Edit '+key && e.actions.includes('press'));
if (found.length !== 1) return {choices:q.elements.map(e=>e.name)};
await app.press(ref(q,found[0]));
const form = {};
for (const role of ['text_field','text_area','checkbox']) {
  const page = await query(role);
  form[role] = page.elements.map(e=>({name:e.name,value:e.value,actions:e.actions}));
}
form.saveButtons = (await query('button','save')).elements.map(e=>e.name);
q = await query('button','Cancel');
found = q.elements.filter(e=>e.name === 'Cancel' && e.actions.includes('press'));
if (found.length !== 1) return {form,needsCloseDecision:true};
await app.press(ref(q,found[0]));
return {form,closedWithoutSaving:true};
```

Once these form labels are known, run the batch below. A full tree, another row scan or a screenshot is unnecessary when the controls are already identified. If a visible transition is asynchronous, make bounded read-only checks for the expected control inside the script; never repeat the action to wait for it.

Native names/values are bounded excerpts; use field equality checks for short values, not as proof of a whole long document. Keep pages in JavaScript. Follow `next` with `app.observe({cursor:page.next})`; a page can be empty and still have a continuation. Return compact names/values/status, not full trees or arrays of unrelated row buttons. `coverage.complete=false` is incomplete discovery even if `next` is null. Use a screenshot when an unavailable node prevents semantic discovery; another unfiltered tree read adds no proof.

## 2. Execute the known workflow and verify it

Adapt this complete batch to labels supplied by the task or discovered in the app. Each exact-name query obtains a fresh real control; it never fabricates a ref. It starts from a list, edits and saves each record once, then reopens it to verify persisted values before moving on. The checkbox example uses observed `'0'`/`'1'` values; adapt those to the app's actual representation. Helpers are local to each script; functions cannot be stored in JSON state.

```javascript
const window_ref = state.window;
async function one(role, name) {
  let p = await app.observe({window_ref, role, text:name, limit:5});
  const found = [];
  for (;;) {
    found.push(...p.elements.filter(e => e.name === name && e.enabled !== false));
    if (!p.next) break;
    p = await app.observe({cursor:p.next});
  }
  if (!p.coverage.complete || found.length !== 1)
    throw Error('Need one identified control in complete discovery: '+name);
  return {ref:{window_ref, observation_ref:p.observation_ref,
               element_ref:found[0].element_ref}, element:found[0]};
}
async function press(name) { await app.press((await one('button',name)).ref); }
async function set(role,name,value) {
  await app.setValue({...(await one(role,name)).ref, value});
}
const records = [
  {key:'Contact A', name:'Alex', note:'Reviewed\nReady'},
  {key:'Contact B', name:'Sam', note:'Reviewed\nFollow up'}
];
const verified = [];
for (const row of records) {
  await set('text_field','Search contacts',row.key);
  await press('Edit '+row.key);
  await set('text_field','Display name',row.name);
  await set('text_area','Notes',row.note);
  const approved = await one('checkbox','Approved');
  if (!['0','1'].includes(approved.element.value)) throw Error('Interpret checkbox state');
  if (approved.element.value !== '1') await app.press(approved.ref);
  await press('Save changes');
  await press('Edit '+row.key);
  const savedName = (await one('text_field','Display name')).element.value;
  const savedNote = (await one('text_area','Notes')).element.value;
  const savedApproved = (await one('checkbox','Approved')).element.value;
  await press('Cancel');
  if (savedName !== row.name || savedNote !== row.note || savedApproved !== '1')
    throw Error('Persisted value differs for '+row.key);
  verified.push({key:row.key, verified:true});
}
return verified;
```

For a checkbox, read its current value and press only when it differs from the requested state. Interpret values from that app's observed controls; do not use the text setter for booleans. If a save completes asynchronously, observe its status before reopening. Stop on uncertainty; do not rerun the batch to recover a missing receipt.

## 3. Switch apps with a new script

For full replacement of a named, already-open plain-text document with known editing focus, select its window, edit, save and capture in one exec. Substitute the task's title/text and platform shortcut. If editing focus is unclear, return a screenshot first and identify the intended control.

Do not spend one exec returning all window titles and a second selecting the already-named document. Select and inspect it in the same script. If a short read-only preflight is useful, include it in the edit script and proceed when it confirms the expected document; return early only when there is an actual decision to make.

```javascript
const matches = (await app.windows()).windows.filter(w => w.title === 'Task document.txt');
if (matches.length !== 1) throw Error('Identify the intended document window');
const window_ref = matches[0].window_ref;
await app.keypress({window_ref, keys:['META','A']});
await app.typeText({window_ref, text:'Line one\nLine two'});
await app.keypress({window_ref, keys:['META','S']});
return await app.capture({window_ref});
```

Input methods focus their target window internally. Typing inserts into its focused control; it does not choose a field. After any workflow, inspect resulting content or visible state. Native `dispatched` and script `completed` alone do not prove task success. Keep readback in the action script; if a semantic readback is incomplete, return its fallback capture from that same script.

## Method contract

Arguments are exact objects; text and text-setter values are strings. Keep element metadata separate from the action `ref`, as above; extra `name`, `value` or `actions` fields cause `invalid_args`. Use the signatures from `apps` and this table. Ask `api` for specific methods only when a required argument is still unclear; do not fetch every schema to confirm already-known methods.

| Method | Contract |
| --- | --- |
| `app.windows()` | All instances' windows and explicit inventory coverage. |
| `app.observe({window_ref,role?,text?,limit?})` | Native role and case-insensitive name/value filtering; default 40, max 100 matches. |
| `app.observe({cursor})` | Continue a query without other arguments. |
| `app.capture({window_ref})` | Exact-window PNG returned as MCP image content outside JavaScript. |
| `app.press({window_ref,observation_ref,element_ref})` | Fresh element must advertise `press`. |
| `app.setValue({window_ref,observation_ref,element_ref,value})` | Editable text control must advertise `setValue`; verify resulting value/events. |
| `app.click({window_ref,observation_ref,x,y,button?,count?})` | Referenced image coordinates; left/right, count 1/2. |
| `app.typeText({window_ref,text})` | Literal Unicode insertion; no clipboard. |
| `app.keypress({window_ref,keys})` | Complete chord, e.g. `['META','A']`, `['CTRL','S']`, `['RETURN']`. |
| `app.scroll({window_ref,delta_x?,delta_y?})` | Wheel steps −100…100; positive x right/y down. |
| `app.focus({window_ref})` | Bring the selected window forward. |

Each window retains one semantic query and one capture independently; capture preserves semantic refs. All query pages share an observation ref. A new same-window query replaces its semantic query. **Every admitted action consumes that window's observations**, including focus/keyboard/text. Query fresh before each semantic action, capture fresh before each coordinate action. Other windows remain independent. Observations expire after 15 seconds; observe and act in the same script. Never invent refs or borrow coordinates from another image.

## Identity, state and recovery

Pass the stable `app` from `apps` unchanged (e.g. `com.google.Chrome`). No PID, generation, request ID, grant or custom Host is needed; the Harness handles approvals. All instances of the ID are grouped, and each `window_ref` routes to its exact native instance. For matching window titles, inspect candidate contents to choose; do not pick the first match.

One exec operates one app. Switching apps preserves each app's committed JSON state and refs; separate MCP sessions are independent. Joining/closing one instance preserves siblings' refs/state. `state_reset` means all previous instances were replaced; `context_reset` retires native refs but preserves JSON state. Rediscover after stale refs/`target_changed`; stored state cannot make refs valid.

On `focus_changed`, `focus_unproven`, `stale_observation` or `target_changed`, stop the current sequence and refresh the affected window: list windows if its identity changed, observe for semantics, or capture for coordinates. Inspect the actual result before continuing only the unfinished steps. Input may already have partial effects; preserve its original receipt and never replay the whole script/text or repeatedly steal focus back. Other windows and app contexts do not need a reset.

`exec` waits and returns a compact summary. No polling or `result` query is needed after ordinary success. Inspect summary issues even if the script succeeded: `incomplete_reads` means a window inventory was not fully proven, so absence from that list does not prove a window disappeared. For uncertainty, `result({app})` reads this connection's latest accepted execution, or use a **returned** `execution_id` for that original ledger. An unreceived/rejected request may leave an older latest record. Never resubmit an action merely to recover its receipt; retain uncertainty and inspect the target if its original record is unavailable.

DTW has a 30-second execution deadline without a native-call quota. MCP cancellation stops work; dispatched input may still have effects. Scripts have no filesystem, network, process, timers or arbitrary native-tool APIs. JSON state commits only on successful execution and is not durable. Report permission/unsupported-surface conditions without repeated identical attempts. Foreground input can change focus and coordinate clicks move the system pointer; independent background/lock-screen modes are not provided.
