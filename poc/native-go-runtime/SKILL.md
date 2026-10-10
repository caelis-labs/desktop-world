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
const ob = await dtw.observe({
  scope: {desktop: true}, projection: 'summary', fields: ['name', 'role'],
  budget: {max_results: 32}
});
state.windows = ob.objects.filter(o => o.kind === 'window');
print(JSON.stringify({count: state.windows.length,
  complete: ob.coverage?.complete, more: !!ob.coverage?.continuation}));
```

`complete:false` and zero matches mean unknown, not absent. Follow a bounded
continuation or narrow the scope. Preserve current Refs and read a control's
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
