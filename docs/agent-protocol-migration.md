# Current Agent protocol

Start `dtw mcp` to expose `apps` and `exec({app,source})`. The
[bundled Skill API](../skills/dtw/API.md) defines the supported methods and limits.
Scripts use ordinary async JavaScript, `print` and `return`. DTW owns window
routing, observations, execution history and session state.

`look`/`capture` return complete snapshots inside scripts. Published MCP views
are incremental; `full:true` requests a baseline. Observe to choose unknown actions
or verify effects; known actions need no redundant initial look. Use screenshot
fallback when semantics are incomplete. Unknown input
effects require fresh observation before further input.

Installation uses [immutable complete packages and host/profile pins](package-management.md).

If a saved stdio configuration uses `--atomic`, update its arguments to
`["mcp"]` and migrate the caller to `apps` and `exec({app,source})`. On macOS,
DTW uses background input where supported. Windows and Linux X11 prepare the foreground for
interaction. Keep related focus and input in one `exec`. Consumers should read `view.kind` and use
`effects.acknowledgements` for specific postconditions when present.
