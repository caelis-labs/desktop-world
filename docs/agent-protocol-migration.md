# Current Agent protocol

Desktop World exposes only `apps` and `exec({app,source})`. `app` is required.
The supported script methods, input/output and limits are defined in the
[bundled Skill API](../skills/desktop-world/API.md). Scripts use ordinary async
JavaScript, `print` and `return`; window routing, observations, execution history
and session state remain internal.

`look`/`capture` return complete snapshots inside scripts. Published MCP views
are incremental; `full:true` requests a baseline. Normal tasks are look → input
→ look, with screenshot fallback when semantics are incomplete. Unknown input
effects require fresh observation before further input.

Installation uses [immutable complete packages and host/profile pins](package-management.md).
