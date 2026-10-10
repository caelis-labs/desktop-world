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

JavaScript has persistent `state`, `print`, and `dtw`. `dtw` supplies async
`observe`, `read`, `sync`, `act`, `capture`, `get`, and `cancel`; `dtw.sleep` is a
bounded scheduling primitive. `dtw.grants()` reads this Session's grants and
`dtw.revokeGrant({grant_id})` can revoke one of them; scripts cannot grant
themselves authority. Use `await`, loops, filters, and exceptions to
batch narrow work, keep full observations in `state`, and `print` only the facts
needed for the next decision. For example:

```javascript
const title = 'exact owned window title';
let ob = await dtw.observe({
  scope: {desktop: true}, projection: 'summary', fields: ['name', 'role', 'app'],
  budget: {max_results: 64, max_visited_nodes: 512, max_depth: 12}
});
const matches = [];
for (let page = 0; page < 8; page++) {
  matches.push(...ob.objects.filter(o => o.kind === 'window' &&
    o.name?.status === 'known' && o.name.value === title));
  if (matches.length > 1 || !ob.coverage?.continuation) break;
  ob = await dtw.observe({
    scope: {desktop: true}, projection: 'summary', fields: ['name', 'role', 'app'],
    budget: {max_results: 64, max_visited_nodes: 512, max_depth: 12},
    continuation: ob.coverage.continuation
  });
}
if (matches.length !== 1 || ob.coverage?.complete !== true)
  throw Error('window is not uniquely proven');
state.window = matches[0];
print(JSON.stringify({window_found: true, coverage_complete: ob.coverage?.complete}));
```

The `name` field is a fact object; compare `name.status` and `name.value`, not
`name` directly to a string. `complete:false` and zero matches mean unknown,
not absent. Follow a bounded continuation or narrow the scope. Do not print
other windows or the full observation. For a previously approved exact window,
first inspect `await dtw.grants()`. If exactly one active grant has that
`window_title` and an `application` Ref, observe with
`scope:{refs:[grant.application]}` and find the exact window there. An absent,
ambiguous, or pending grant does not authorize an action. If a read-only
observation is dirty or partial, at most one fresh bounded read may resolve it;
act only on clean, complete coverage and never replay an uncertain write.
The supported projections are `summary`, `outline`, `detail`, and
`capture_windows`. `summary` and `detail` can return only the scoped root;
use `outline` to find descendants. `value` is not an observation field; use
`value_preview` or `dtw.read` for long text. A narrow control lookup after
proving an exact window is:

```javascript
const ob = await dtw.observe({
  scope: {refs: [state.window.ref]}, projection: 'outline',
  fields: ['name', 'role', 'capabilities', 'value_preview'],
  match: {within: state.window.ref, name_equals: 'POC text'},
  budget: {max_results: 16, max_visited_nodes: 512, max_depth: 12,
           read_deadline_ms: 3000}
});
const fields = ob.objects.filter(o => o.name?.status === 'known' &&
  o.name.value === 'POC text');
if (ob.coverage?.complete !== true || ob.coverage?.dirty || fields.length !== 1)
  throw Error('field is not uniquely proven');
state.field = fields[0];
print(JSON.stringify({field_found: true, coverage_complete: true}));
```

Find another named control with the same scoped `outline` pattern and its
own exact `name_equals` predicate. Keep full objects in `state`; print only
the facts needed for the task.
Preserve current Refs and read a control's
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
