---
name: desktop-world
description: Control one authorized desktop application through DTW using compact semantic observations, app screenshots, mouse and keyboard scripts.
---

# Desktop World

DTW exposes two MCP tools: `apps` and `exec`. Use `apps({query})` to find an
installed or running app and its authorization identity. Run an async JavaScript
body with `exec({app, source})`; `app` is always required and fixes the entire
script's authorization scope.

Start with `await app.look()`. DTW starts a stopped app where supported and
returns a compact baseline; subsequent looks return changes. Use semantic target
IDs from the latest observation or a unique selector to click known controls.
Use `await app.capture()` when the UI needs visual understanding or semantics
are unavailable, then click coordinates in the returned image. After input DTW
returns a fresh observation automatically. Use `move({x,y})` for hover and
`drag({path:[{x,y},...]})` for one content drag within one window on one display;
the runtime pairs releases. Group known sequential steps in one script. Await every app operation.

```javascript
await app.look();
await app.click({selector:{role:'button',text:'Continue'}});
```

The public methods are `look`, `capture`, `click`, `move`, `drag`, `typeText`,
`keypress`, `scroll` and `quit`. Read [API.md](API.md) for exact arguments and
output semantics;
[PATTERNS.md](PATTERNS.md) covers visual fallback, dialogs and recovery.
There is no Agent-managed Session, window binding, cursor or native reference.
Use optional `print(...)` or return a small JSON value for task-specific output.
Ordinary variables last for one execution. Imports, filesystem, network, process
APIs and timers are unavailable; scripts have a 30-second deadline.

A completed script or dispatched action does not establish the requested app
effect. Inspect the returned changes, read back, or capture when necessary.
Unknown/interrupted input may already have effects: inspect before new input;
never replay a script merely because its response was lost. Native cleanup
failure blocks further input in that connection. Standard MCP cancellation stops
its matching call; quitting never force-kills or discards documents.

`permission_required` identifies missing OS access. Grant the host named by the
OS; Screen Recording changes may require restarting that host and reconnecting.
Discovery reads no window contents and does not itself authorize an app.
