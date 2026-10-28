---
name: desktop-world
description: Inspect and operate running macOS applications with Desktop World's MCP tools, including accessibility observations, window screenshots and supported controls. Use for desktop GUI tasks through DTW; browser DOM, keyboard and text input are not yet implemented.
---

# Desktop World

Use the configured Desktop World MCP server. Tool names may have a client-added namespace. The tools are `apps`, `exec` and `result`.

## Discover and bind the application

Call `apps` to find the requested running application and read the advertised script methods, limits and support conditions. Choose its returned `app` string, such as `com.apple.TextEdit`, and pass that string unchanged in `exec.app`. The app name is for display. DTW resolves process instances internally; do not supply a PID, process generation, executable path or composite handle.

Every execution requires an explicit stable app ID so the Harness can inspect ordinary tool arguments. DTW does not manage approval. Use the Agent's normal tool policy; no grant file, approval flag or custom Host wiring is needed. `apps` deduplicates by app ID, and multiple independent usable instances of the same ID cause `ambiguous_app`; do not guess a process.

## Execute and verify

Call `exec` with `app`, a fresh descriptive `request_id`, and JavaScript `source`. The source is an async function body; use `await` and `return`. `exec` waits for the bounded result, so no submit/poll loop is needed.

Start with a small observation, for example:

```javascript
const inventory = await app.windows();
return inventory;
```

Available methods return Promises:

- `app.windows()` returns the proven window inventory and its coverage.
- `app.observe({window_ref})` returns semantic elements and an observation reference.
- `app.capture({window_ref})` returns image metadata; the MCP response carries the image.
- `app.press({window_ref, observation_ref, element_ref})` acts on a currently pressable semantic element.
- `app.click({window_ref, observation_ref, x, y})` uses pixels in the referenced image.

Choose an unambiguous window and element from current observations. Read structured roles, names, values, enabled state and actions where available; `label` is display text. Never invent refs or use coordinates from another image. `windows`, new observations and admitted actions retire the prior observation; refs also expire. A stale reference requires a fresh observation.

After an action, inspect an independent observation to verify the requested effect. `dispatched` means input was dispatched, and `completed` means execution ended; neither proves the business task succeeded. Return the relevant result rather than an entire tree when a narrower answer is sufficient.

Scripts have a default maximum of 5 seconds and 32 native bridge attempts. Budget options can only reduce those limits. Split longer workflows into bounded executions. There are no filesystem, network, process, timer or arbitrary native-tool APIs. JSON `state` persists within the same app context after successful scripts. Switching applications closes the previous context and releases its native resources; returning to it reports `context_reset` and starts with empty state and new refs. Original receipts remain readable. State is not durable storage and cannot make stale refs valid.

## Interruption and recovery

Keep the original app ID, request ID and receipts. Standard MCP cancellation or connection closure stops work and releases resources; already dispatched input may still have effects.

Use `result({app, request_id})` to read the original record after interruption or an uncertain response. Do not submit the script under a new ID to recover a missing result. If the connection or receipt is gone, keep the outcome unknown and independently observe before deciding on another action.

`target_changed` means the running instance or window changed. Observe again before deciding on a new action; never reuse the old instance’s references or assume its state survived a restart. `context_reset` means saved state and old refs are gone. OS permission and unsupported-surface errors are execution conditions: report the structured cause instead of repeatedly retrying the same action or switching input routes.
