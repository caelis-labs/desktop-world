---
name: desktop-world-native-go-poc
description: Use the isolated native Go Desktop World POC through its one MCP exec tool.
---

# Desktop World native Go POC

This file describes the isolated POC surface. It is not installed as the product Skill.
The trusted host starts the native Go MCP server, its native helper, and exact
per-Session grants. No Node/npm process or model-selected transport is involved.

Use the sole MCP tool, **`exec`**. Its `operation` is `exec`, `status`, `result`,
or `cancel`; every call carries the original `execution_id`. `exec` also takes
JavaScript `code`. Query or cancel the same ID while a script is running. A
finished `result` can expose the original native receipt and, with
`include_image:true`, a requested PNG as MCP `ImageContent`.
Printed facts live in `structuredContent.print`; text carries the short
execution/status/error summary and does not copy printed AX nodes.

JavaScript has persistent `state`, `print`, and `dtw`. `dtw` supplies async
`observe`, `read`, `sync`, `act`, `capture`, `get`, and `cancel`; `dtw.sleep` is a
bounded scheduling primitive. `dtw.grants()` reads this Session's grants and
`dtw.revokeGrant({grant_id})` can revoke one of them; scripts cannot grant
themselves authority. Use `await`, loops, filters, and exceptions to
batch narrow work, keep full observations in `state`, and `print` only the facts
needed for the next decision. Start with an existing exact grant when one is
available, then search inside that App for one exact window:

```javascript
const title = 'exact approved window title';
const grants = await dtw.grants();
const active = grants.grants.filter(g => g.window_title === title &&
  g.state === 'active' && g.application);
if (active.length !== 1) throw Error('exact grant unresolved');
const app = active[0].application;
const ob = await dtw.observe({
  scope: {refs: [app]}, projection: 'outline', fields: ['name', 'role', 'app'],
  match: {within: app, name_equals: title},
  budget: {max_results: 16, max_visited_nodes: 512, max_depth: 12}
});
const matches = ob.objects.filter(o => o.kind === 'window' &&
  o.name?.status === 'known' && o.name.value === title);
if (matches.length !== 1 || ob.coverage?.complete !== true || ob.coverage?.dirty)
  throw Error('window is not uniquely proven');
state.window = matches[0];
const shown = dtw.disclose(ob, {fields: ['kind', 'role', 'name']});
const windowItem = shown.items.find(i => i.kind === 'window' &&
  i.name?.value === title);
if (!windowItem) throw Error('selected window was not disclosed');
state.windowId = windowItem.id;
print(JSON.stringify(shown));
```

The `name` field is a fact object; compare `name.status` and `name.value`, not
`name` directly to a string. `complete:false` and zero matches mean unknown,
not absent. If no grant is available for a read-only task, find the selected App
in a bounded desktop summary, then narrow to its windows. Follow continuation
only when needed to establish the declared scope; do not send unrelated tree
pages to the model. An absent, ambiguous, or pending grant does not authorize
an action. If a read-only
observation is dirty or partial, at most one fresh bounded read may resolve it;
act only on clean, complete coverage and never replay an uncertain write.
The supported projections are `summary`, `outline`, `detail`, and
`capture_windows`. `summary` and `detail` can return only the scoped root;
use `outline` to find descendants. `value` is not an observation field; use
`value_preview` or `dtw.read` for long text. A narrow control lookup after
proving an exact window is:

```javascript
const ob = await dtw.observe({
  scope: {ids: [state.windowId]}, projection: 'outline',
  fields: ['name', 'role', 'capabilities', 'value_preview'],
  match: {within_id: state.windowId, name_equals: 'POC text'},
  budget: {max_results: 16, max_visited_nodes: 512, max_depth: 12,
           read_deadline_ms: 3000}
});
const fields = ob.objects.filter(o => o.name?.status === 'known' &&
  o.name.value === 'POC text');
if (ob.coverage?.complete !== true || ob.coverage?.dirty || fields.length !== 1)
  throw Error('field is not uniquely proven');
state.field = fields[0];
const shown = dtw.disclose(ob, {
  fields: ['kind', 'role', 'name', 'capabilities']
});
const fieldItem = shown.items.find(i => i.name?.value === 'POC text');
if (!fieldItem) throw Error('selected field was not disclosed');
state.fieldId = fieldItem.id;
print(JSON.stringify(shown));
```

Find another named control with the same scoped `outline` pattern and its
own exact `name_equals` predicate. `dtw.disclose(ob, {fields, max_items})` is
the POC's progressive presentation helper: it keeps a per-Session, bounded
Ref/field cache and returns only new or changed fields. Repeating an unchanged
target returns `items:[]`; asking for `capabilities` later adds just that field.
Use `refresh:true` when the model explicitly needs an unchanged fact again.
The model-facing `id` is a short display alias, such as `W2/R1`, scoped to a
window. `dtw.index(windowObservation)` can register window bounds without
printing them. Use `dtw.observe({scope:{ids:[id]},match:{within_id:id}})`,
`dtw.read({target_id:id})`,
`dtw.capture({kind:'window_content',target_id:windowId})`, and
`dtw.act({steps:[{id:'one',op:'invoke',target:{id}}]})` to use it. An action ID
must first have appeared in a clean, complete disclosure. The POC still
supports `dtw.ref(id)` for old scripts; it resolves the full native Ref inside
the same Session but gives no authority, and stale Refs still fail native
action checks. Alias maps clear on native epoch change and are never shared
between Sessions. When a selected window and target expose usable
bounds, `area_hint` reports a rough relative band (`top`, `left`, `main`,
`right`, `bottom`); it is navigation context, not a semantic role or action
target. Windows receive distinct `W` aliases; main/popup relationships remain
unknown unless the native provider supplies trustworthy structure. Full native
Refs and sampling times stay in the internal observation and original receipt.
Its `coverage` is the original native coverage; `omitted` is a separate
presentation limit. Neither `items:[]` nor an incomplete native scan proves
absence. Use the original `ob.objects` for action decisions, keep full objects
in `state`, and print only the disclosure or even smaller task facts. A large
unfiltered `outline` may hit the native output cap; refine by exact window,
name, role, and depth rather than sending a long tree or all its pages.
Preserve current native observations inside the script and read a control's
capabilities before an action. For `set_checked`, `set_selected`, and
`set_expanded`, give an explicit desired boolean. The native receipt states the
selected semantic or foreground channel, delivery, verification, and
restoration. A physical dispatch receipt does not by itself prove that an App
handled the action; verify the business result independently. The execution
layer chooses a background or short foreground route; scripts do not set mode.

On cancellation, partial completion, unknown delivery, a late native reply, or
`state_lost`, query `result` for the same `execution_id` and retain its original
native request IDs. Never generate a new ID to replay an uncertain effect.
An unverified provider route or an unknown outcome cannot fall back to
foreground input. Never extend grants through script arguments.

Current POC limits: 64 KiB script, 8 KiB printed text, at most 32 native calls
and 128 retained executions per Session. See `RESULTS.md` for failed and
unverified acceptance rows. Do not treat this POC as a product runtime.
