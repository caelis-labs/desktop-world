# Current Agent protocol

Desktop World defaults to `apps` and `exec({app,source})`. Start `dtw mcp --atomic`
for `apps` plus fourteen flat camelCase app operations without JavaScript.
Discover/list actual tools: use scripts if `exec` is present; otherwise invoke the
listed operation directly. Both use the same object fields; atomic calls add
required `app`, while `exec` binds `app` once for its script. The supported
methods, input/output and limits are defined in the
[bundled Skill API](../skills/dtw/API.md). Scripts use ordinary async
JavaScript, `print` and `return`; window routing, observations, execution history
and session state remain internal.

`look`/`capture` return complete snapshots inside scripts. Published MCP views
are incremental; `full:true` requests a baseline. Observe to choose unknown actions
or verify effects; known actions need no redundant initial look. Use screenshot
fallback when semantics are incomplete. Unknown input
effects require fresh observation before further input.

Installation uses [immutable complete packages and host/profile pins](package-management.md).

Current hosts launch `dtw mcp` by default or opt in with `dtw mcp --atomic`; see
[MCP toolsets](mcp-toolsets.md). Remove the retired
`--input-policy` flag from saved commands. Default macOS routing still chooses
its native background path when supported. Windows prepares the foreground for
interaction. Each tool call has its own settlement boundary; keep related
focus/input and Windows menu-open/item selection in one `exec`. Separate atomic
calls do not share that scope. Consumers should read `view.kind` and use
`effects.acknowledgements` for specific postconditions when present.
