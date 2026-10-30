---
name: desktop-world
description: Operate desktop apps through DTW MCP with app-scoped JavaScript, filtered native discovery, screenshots, text input and shortcuts. Use for GUI tasks through DTW; batch known actions and verify their effects inside each script.
---

# Desktop World

Use the configured DTW MCP server's `apps`, `api`, `exec`, `result` tools (the client may prefix their names). Start with `apps` for stable app IDs, method signatures and platform conditions. Execute with exactly `{app, source}`. `source` is an async JavaScript function body with `app`, `state`, `await` and `return`. Send the original source string serialized once; no filename or double JSON encoding.

**Work in complete UI steps.** Return to the model only when new labels, state or an image require a decision. Once the next operations are known, run them sequentially in the same script, including verification. For repeated forms, discover the labels once, then process the requested records in one loop. Do not rediscover the surrounding table between records. Input methods target the selected window; macOS prefers background delivery and temporarily focuses it when needed. A separate focus call is usually unnecessary.

**Confirm form actions before changing values.** Task-supplied field labels do not establish save, submit or close button labels. Before the first field or checkbox mutation, discover and validate the relevant action names in the actual form, once per form layout. Do not turn a task verb into a guessed exact label. If interpretation is needed, return compact action choices before editing. Then batch records with fresh exact-name lookups; do not repeat action-label discovery for every record or scan a full tree.

**Finish every query before deciding.** This includes filtered queries for a known, exact label. `limit` limits returned matches; it does not guarantee that the native tree was fully scanned. While `page.next` exists, collect the page and call `app.observe({cursor:page.next})` inside the same script. Only after the last page should you check `coverage.complete` and whether there is exactly one match. A first page with zero or one matches and a continuation is neither missing nor ambiguous. Use the query helper in section 2 for every control lookup.

**After a known UI transition, retry only the read.** Search setters, opening a form and saving can finish before their controls settle. Use bounded fresh queries for the expected next control, not another setter, press or save. Keep incomplete coverage, missing and disabled results distinct. A complete ambiguous match stops immediately; provider, permission and identity errors propagate without a retry.

Optional host-argument object properties whose value is `undefined` are omitted; required fields are still validated, and array entries cannot be `undefined`. Results and committed state must be JSON. Project documented element fields: `role`, `name`, `value`, `enabled`, `actions` and refs. There is no `description` field. Omit absent fields or use `?? null`; returning `undefined` inside an object fails serialization. Continue through the requested edits and saves: an intermediate discovery or screenshot is not task completion.

## 1. Discover the relevant controls

For a browser table/form, start with **window selection plus `role:'text_field'` discovery in the same exec**. Find the search/filter field before searching the entire tree for a record keyword. On a first inspection of an unfamiliar window, include its capture in that same return: a cold browser can expose only its toolbar, so this gives the next decision both the visible page and available controls without another round trip. Omit the image when existing context already identifies the page. Do not dump an unfiltered browser window or all table rows. Adapt this example to the task:

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
return {fields, coverage:p.coverage, capture:await app.capture({window_ref:state.window})};
```

If the task provides field labels, skip only their repeated discovery. Identify any unknown form actions before proceeding to section 2. Combine filtering, identifying the edit control, opening the form and discovering the relevant controls in one script. This example uses an observed search label and requires an actually discovered, exact edit label before pressing it. Adapt the action-search keywords to the task; keywords find candidate names and are not exact action labels. Return compact choices when their meaning or uniqueness needs a decision, before changing form values.

Copy the local helpers from section 2 into this script. Discover the first form’s action labels and any still-unknown field roles together, in this same exec. Return only relevant names, roles, values and advertised actions. Once those are known, skip this inspection for later records and batch their edits, saves and readback.

```javascript
const window_ref = state.window, key = 'Contact A';
const search = await one('text_field','Search contacts','setValue');
await app.setValue({...search.ref, value:key});
const edit = await one('button','Edit '+key,'press',true);
await app.press(edit.ref);
const saveChoices = (await query('button','save',{afterTransition:true})).elements
  .map(e=>({name:e.name,enabled:e.enabled,actions:e.actions}));
const closeName = state.formActions?.close;
const q = await query('button',closeName ?? 'cancel',
  closeName ? {name:closeName,action:'press',afterTransition:true} : {afterTransition:true});
const actionChoices = {save:saveChoices,
  close:q.elements.map(e=>({name:e.name,enabled:e.enabled,actions:e.actions}))};
// Include task-relevant field metadata here when its role is still unknown.
const fields = [];
for (const name of ['Display name','Approved','Notes']) {
  const controls = await query(undefined,name,{afterTransition:true});
  fields.push(...controls.elements.filter(e=>e.name===name).map(e=>({
    name:e.name,role:e.role,value:e.value ?? null,enabled:e.enabled,actions:e.actions
  })));
}
// A keyword match alone does not establish what pressing it will do.
if (!closeName) return {actionChoices,fields,formOpen:true};
// Field discovery replaced the earlier query: obtain a fresh close ref.
await press(closeName);
return {actionChoices,fields,closedWithoutSaving:true};
```

Choose the relevant exact action names from the observed controls and store them as `state.formActions = {save: observedSaveName, close: observedCloseName}`; these are names, not reusable refs. A save action may be disabled until values change, so distinguish an observed disabled control from an absent label. If the actions are already clear, continue with the batch in the same script instead of returning only discovery. If a decision was needed, retain whether the form was left open and resume there. A full tree, another row scan or a screenshot is unnecessary when the controls are already identified. If a visible transition is asynchronous, make bounded read-only checks for the expected control inside the script; never repeat the action to wait for it.

Native names/values are bounded excerpts; use field equality checks for short values, not as proof of a whole long document. Keep pages in JavaScript. Follow `next` with `app.observe({cursor:page.next})`; a page can be empty and still have a continuation. Return compact names/values/status, not full trees or arrays of unrelated row buttons. `coverage.complete=false` is incomplete discovery even if `next` is null. Use a screenshot when an unavailable node prevents semantic discovery; another unfiltered tree read adds no proof.

## 2. Execute the known workflow and verify it

Adapt this complete batch to task-supplied field labels and `state.formActions` confirmed in the first form. Each exact-name query obtains a fresh real control; it never fabricates a ref. The example starts from a list, edits and saves each record once, then reopens it to verify persisted values before moving on. If discovery left the first form open, begin with its field edits instead of opening it again. Do not assume a multiline field has role `text_area`: the targeted `Notes` query below selects an exact-name editable control, captures its observed role and reuses that role with fresh refs. Only recognized static labels without an advertised setter are excluded; unknown roles and disabled controls remain candidates, so multiple potential controls remain ambiguous. The checkbox example uses observed `'0'`/`'1'` values; adapt those to the app's actual representation. Helpers are local to each script; functions cannot be stored in JSON state.

```javascript
const window_ref = state.window;
const actionNames = state.formActions;
if (!actionNames?.save || !actionNames?.close)
  return {needsActionDiscovery:true}; // Identify actions before any form mutation.
async function query(role,text,{name,action,afterTransition=false}={}) {
  const until = Date.now()+2000, attempts = [];
  for (let n=0; n<(afterTransition ? 4 : 1); n++) {
    if (n && Date.now() >= until) break;
    let p = await app.observe({window_ref,...(role === undefined ? {} : {role}),
      ...(text === undefined ? {} : {text}),limit:12});
    const elements = []; // Fresh collection and refs for this whole attempt.
    for (;;) {
      elements.push(...p.elements);
      if (!p.next) break;
      p = await app.observe({cursor:p.next});
    }
    const named = name === undefined ? elements : elements.filter(e=>e.name === name);
    // Exclude only recognized static labels without a setter. Unknown roles
    // and disabled controls may still be the intended field; keep them.
    const found = role === undefined && action === 'setValue' ? named.filter(e=>
      e.actions.includes('setValue') || !['static_text','text'].includes(e.role)) : named;
    const status = !p.coverage.complete ? 'incomplete' :
      name !== undefined && found.length > 1 ? 'ambiguous' :
      name !== undefined && !found.length ? (named.length ? 'unsupported_action' : 'missing') :
      action && found[0].enabled === false ? 'disabled' :
      action && !found[0].actions.includes(action) ? 'unsupported_action' : 'ready';
    attempts.push({status,count:found.length,named_count:named.length,coverage:p.coverage});
    if (status === 'ready')
      return {elements:found,observation_ref:p.observation_ref,attempts};
    if (status === 'ambiguous' || status === 'unsupported_action') break;
  }
  throw Error(JSON.stringify({query:{role,text,name:name ?? null,action:action ?? null},attempts}));
}
function ref(q,e) {
  return {window_ref,observation_ref:q.observation_ref,element_ref:e.element_ref};
}
async function one(role,name,action,afterTransition=false) {
  const q = await query(role,name,{name,action,afterTransition});
  return {ref:ref(q,q.elements[0]),element:q.elements[0]};
}
async function press(name,afterTransition=false) {
  await app.press((await one('button',name,'press',afterTransition)).ref);
}
async function set(role,name,value,afterTransition=false) {
  const control = await one(role,name,'setValue',afterTransition);
  await app.setValue({...control.ref,value});
  return control.element.role;
}
const records = [
  {key:'Contact A', name:'Alex', note:'Reviewed\nReady'},
  {key:'Contact B', name:'Sam', note:'Reviewed\nFollow up'}
];
const verified = [];
let notesRole; // The first targeted setter query discovers the actual native role.
for (const [index,row] of records.entries()) {
  await set('text_field','Search contacts',row.key,index > 0);
  await press('Edit '+row.key,true);
  await set('text_field','Display name',row.name,true);
  notesRole = await set(notesRole,'Notes',row.note,true);
  const approved = await one('checkbox','Approved',undefined,true);
  if (!['0','1'].includes(approved.element.value)) throw Error('Interpret checkbox state');
  if (approved.element.value !== '1') {
    if (approved.element.enabled === false || !approved.element.actions.includes('press'))
      throw Error('Checkbox is disabled or cannot press');
    await app.press(approved.ref);
  }
  await press(actionNames.save,true);
  await press('Edit '+row.key,true);
  const savedName = (await one('text_field','Display name',undefined,true)).element.value;
  const savedNote = (await one(notesRole,'Notes')).element.value;
  const savedApproved = (await one('checkbox','Approved')).element.value;
  await press(actionNames.close);
  if (savedName !== row.name || savedNote !== row.note || savedApproved !== '1')
    throw Error('Persisted value differs for '+row.key);
  verified.push({key:row.key, verified:true});
}
return verified;
```

The helper normally makes one whole query. `afterTransition:true` permits at most four fresh whole queries, starting retries only within two seconds; each started query drains its cursors under DTW's overall execution deadline. There are no timer APIs to interrupt a pending native call. It never merges attempts, catches native errors or repeats an action. Exhaustion reports each attempt's original coverage and distinguishes missing, disabled and unsupported actions; do not discard these diagnostics. Discovery without an exact `name` may return a complete empty set: choose another relevant keyword instead of treating an unproven label as required. Disabled controls remain visible in action-name discovery, while an action requires an enabled control that advertises that exact operation.

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

Input methods select delivery internally. Unrelated pointer movements and work in other apps do not invalidate keyboard input; refresh when the actual target or required input state changes. Typing inserts into the selected window's focused control; it does not choose a field. On macOS, foreground fallback temporarily borrows focus and returns it when DTW still owns it. Explicit focus is released when the script finishes. After any workflow, inspect resulting content or visible state. Native `dispatched` and script `completed` alone do not prove task success. Keep readback in the action script; if a semantic readback is incomplete, return its fallback capture from that same script.

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
| `app.focus({window_ref})` | Bring the selected window forward; macOS returns borrowed focus after the script. |

Each window retains one semantic query and one capture independently; capture preserves semantic refs. All query pages share an observation ref. A new same-window query replaces its semantic query. **Every admitted action consumes that window's observations**, including focus/keyboard/text. Query fresh before each semantic action, capture fresh before each coordinate action. Other windows remain independent. Observations expire after 15 seconds; observe and act in the same script. Never invent refs or borrow coordinates from another image.

## Identity, state and recovery

Pass the stable `app` from `apps` unchanged (e.g. `com.google.Chrome`). No PID, generation, request ID, grant or custom Host is needed; the Harness handles approvals. All instances of the ID are grouped, and each `window_ref` routes to its exact native instance. For matching window titles, inspect candidate contents to choose; do not pick the first match.

One exec operates one app. Switching apps preserves each app's committed JSON state and refs; separate MCP sessions are independent. Joining/closing one instance preserves siblings' refs/state. `state_reset` means all previous instances were replaced; `context_reset` retires native refs but preserves JSON state. Rediscover after stale refs/`target_changed`; stored state cannot make refs valid.

On `focus_changed`, `focus_unproven`, `stale_observation` or `target_changed`, stop the current sequence and refresh the affected window: list windows if its identity changed, observe for semantics, or capture for coordinates. Inspect the actual result before continuing only the unfinished steps. Input may already have partial effects; preserve its original receipt and never replay the whole script/text or repeatedly steal focus back. Other windows and app contexts do not need a reset.

`exec` waits and returns a compact summary. No polling or `result` query is needed after ordinary success. `incomplete_reads` preserves partial window inventories and terminal incomplete semantic queries, even when JS ignores the result. Missing from a partial inventory does not prove disappearance. Optional `recovered_incomplete_reads` counts semantic reads superseded by a new complete observation of the same window before any action/error intervened; the original warning remains. This proves read-only refresh, not business success or identical query filters. Unrecovered reads require fresh discovery before acting. For uncertainty, `result({app})` reads this connection's latest accepted execution, or use a **returned** `execution_id` for that original ledger. An unreceived/rejected request may leave an older latest record. Never resubmit an action merely to recover its receipt; retain uncertainty and inspect the target if its original record is unavailable.

DTW has a 30-second execution deadline without a native-call quota. History is a sliding window: expired receipts do not reset app state or permit replay. MCP cancellation stops work; dispatched input may still have effects. Scripts have no filesystem, network, process, timers or arbitrary native-tool APIs. JSON state commits only on successful execution and is not durable. Report permission/unsupported-surface conditions without repeated identical attempts. On macOS, eligible input goes directly to the selected app/window without moving the system pointer. When background delivery is unavailable before any side effects, DTW can temporarily focus the window and use shared input. Do not assume every app/action supports background delivery; verify the actual result. Locked-session operation is not yet supported.
