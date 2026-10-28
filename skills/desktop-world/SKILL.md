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

Call `exec` with only the required `app` and JavaScript `source`. DTW assigns execution identity internally; do not generate or supply a request ID. The source is an async function body; use `await` and `return`. `exec` waits for the bounded result, so no submit/poll loop is needed.

Supply script text, not a filename or a JSON-encoded string inside `source`. When constructing requests programmatically, pass the original text to the JSON serializer exactly once; do not pre-escape it. When writing raw tool JSON, account for its string escaping, especially backslashes in JavaScript strings and regular expressions.

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

The normal exec result retains `value.result`, console output and the last capture. `script.summary` provides step and action counts instead of the full step list. Its dispatch/evidence counts and `last_issue` are diagnostics, not proof of success. A caught action error can leave the script successful while an action remains unknown; inspect the summary even when `isError` is false. `last_issue` is only the last diagnostic sample. Use `result({app})`, or a returned `execution_id` when selecting history, only when you need the original `script.steps` ledger. Do not add a result query to every successful call.

After an action, inspect an independent observation to verify the requested effect. `dispatched` means input was dispatched, and `completed` means execution ended; neither proves the business task succeeded. Return the relevant result rather than an entire tree when a narrower answer is sufficient.

The exec input contains exactly `app` and `source`. DTW currently applies a 30-second execution deadline internally, with no call-count quota; use standard MCP cancellation to stop a call. VM memory and retained receipt bytes still bound resource use; a receipt limit stops before the next native dispatch and preserves prior effects. There are no filesystem, network, process, timer or arbitrary native-tool APIs. JSON `state` persists within the same app context after successful scripts. A conversation can alternate between multiple applications without losing each app’s committed state or window refs. Each execution still names exactly one app. Independent MCP sessions have independent state and receipts. Original receipts remain readable. State is not durable storage and cannot make stale refs valid.

## Interruption and recovery

Standard execution needs no bookkeeping ID. Retain a returned `execution_id` only when you need to look up that specific historical execution. Standard MCP cancellation or connection closure stops work and releases resources; already dispatched input may still have effects.

After an uncertain response, use `result({app})` for this MCP connection’s latest accepted execution; use the returned `execution_id` only to select a specific older record. A rejected or unreceived call may leave an older record as latest. Do not resubmit to recover a result: each new exec is new work, even with identical source. If the connection or receipt is gone, keep the outcome unknown and independently observe before deciding on another action.

`target_changed` means the running instance or window changed. Observe again before deciding on a new action; never reuse the old instance’s references or assume its state survived a restart. `context_reset` means native refs must be rediscovered; committed JSON state survives, including any now-stale ref strings in it. `state_reset` means the app instance changed and its state was cleared. OS permission and unsupported-surface errors are execution conditions: report the structured cause instead of repeatedly retrying the same action or switching input routes.
