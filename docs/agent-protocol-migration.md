# Agent protocol migration

The next protocol is a breaking change from the released 0.2.x script surface.
Keep the executable and its entire `skills/desktop-world` directory together.
Current MCP definitions and the packaged Skill are the contract; cached tool
schemas and old installation prompts must be refreshed together.

MCP exposes only `apps` and `exec({app,source})`. The authorization app is required
for every execution and cannot change within its script. Discover a running or
installed target, then submit an async JavaScript body:

```javascript
await app.look();
await app.click({selector:{role:'button',text:'Continue'}});
```

DTW owns window routing, startup where supported and observation baselines.
Subsequent calls receive changes relative to the previous returned tool view.
Input returns a fresh observation; capture enables image-coordinate operation
and visual follow-ups. Use print or small JSON returns for task-specific output.
Returning the final view directly coalesces it into `view`, saving a duplicate.

Removed public interfaces include api/result tools, app-less execution, explicit
Session/state management and native/window-reference bindings. The `dtw api`
CLI and distributed atomic TypeScript declarations are also retired. Their
fixtures may remain in internal tests; they are not a compatibility protocol.

For an interrupted or missing response, issue a fresh ordinary look/capture when
the call settles. Unknown actions may already have effects. Read back their UI
or independently saved effect before deciding what to do; never replay them to
obtain a receipt. A new request ID is new work, not an idempotency key.

macOS supports installed discovery/startup and normal quit; Windows/X11 currently
report running-only discovery. Windows has source support for opaque app-window
composition using physical bounds and z-order; layered alpha and new gestures
remain unsupported. Platform build, GUI, model and installed-asset evidence must
be recorded separately. See [the API](../skills/desktop-world/API.md),
[installation](install.md) and the source checkout's capability/check documentation.

The current lifecycle/LSUIElement and Windows protocol probes accept an optional
`--installed-package` to bind the executable and Skill hashes to the same package.
They recheck those bytes after cleanup. Without it their result is development
binary evidence. A protocol migration smoke in Caelis/DSH/agy should load that
same Skill and inspect the actual two-tool schema before task execution; no
model/Harness A/B is needed to verify that contract.

Release Please owns release version/changelog updates. Preserve the breaking
change marker and inspect its generated version PR before merging. Use the
existing development-candidate route for prepublication validation. Publication
is automatic after the version PR; final downloaded-byte installation/upgrade
acceptance remains distinct from development-candidate evidence.
