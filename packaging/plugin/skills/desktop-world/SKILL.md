---
name: desktop-world
description: Control local macOS and Windows desktop apps through native UI automation and screenshots for tasks that require reading or operating app UI. When desktop UI operation is not explicitly requested, prefer purpose-built connectors, APIs, CLIs, or file tools.
---

# Desktop World

Use the installed DTW `exec` tool (`desktop_world__exec` in Caelis).
Start with `{"operation":"describe"}`: it returns available operations and
configured browser targets with ready-to-use `next_call` arguments.
Harness policy controls approval. Existing-profile scope is host configuration;
Agent calls cannot grant it. A refusal requiring the host is not a reason to
change authorization mode or try another input route.

## Observe, act, verify

For browser pages, use a configured target's `next_call`. With no configured
target, discover the task's app with `{"operation":"apps"}`, then list its
windows with `{"operation":"windows","arguments":{"pid":PID}}`. If the app
name is already known, use `windows` with `arguments:{"app":"Name"}` directly.
`apps` defaults to running apps; `running:false` includes installed apps. Read
`describe` with `name:"apps"` or `name:"windows"` for filters and pagination.
Follow `next_call` for a new live page; fetch `more` for the original inventory.
Inspect titles, visibility and layer hints, then select an exact window ID.
Layer hints are stacking evidence, not proof of a main window; duplicate titles
remain separate candidates. Do not choose by list order or size alone.
Read [BROWSER.md](BROWSER.md) for exact-tab operations. Browser results include
bindings, tabs, refs and recovery calls directly; normal use needs no separate
structured-result fetch. Use `describe` with `name` when an operation's arguments
are unfamiliar.

For a native observation, prefer the compact exact-window route:

```json
{"operation":"observe","arguments":{"pid":PID,"window_id":WID,"max_elements":80}}
```

The summary retains one tree and snapshot/diagnostic metadata. `more` retrieves
the complete native structure, including element tokens, without a new AX read.
Optional `include_screenshot:true` captures an image; use outer `include_image:true`
to display it inline. A new AX observation can invalidate earlier element tokens.

For native controls, scripts run in CUA QuickJS with `cua` and persistent JSON
`state`; Node, filesystem, network and process APIs are unavailable.

```json
{"script":"const app = await cua.getApp({pid:PID,windowId:WID}); const view = await app.getState({query:'Save',max_elements:80,include_screenshot:false}); return {window_id:app.windowId,tree:view.tree_markdown};"}
```

Replace PID and selectors with observed values. An unpinned app follows its
front window, including pickers; use `{pid:PID,windowId:WID}` for an exact window.

```js
const app = await cua.getApp({pid:PID,windowId:WID});
return await app.click({name:"Save"});
// Text comes first: await app.typeText("search term", {name:"SearchField"});
// Keys: await app.pressKey("Return"); cmd+s on macOS, ctrl+s on Windows.
```

Take one purposeful action, then observe the UI to check its effect. Delivery
receipts and AX value echoes do not prove renderer acceptance. After a timeout,
closed dialog or partial script failure, observe before deciding on another
input; never replay the failed action automatically.

## Screenshots and popups

For macOS popups or canvas controls, `{"operation":"app","pid":PID,
"include_image":true}` returns attributed windows and visible frames. Inspect
`frames`, `occluders` and `omitted_count`. Without `window_id`, capture follows
front-to-back stacking; `max_windows:1` does not mean the main window. Add
`window_id:WID` to capture just that attributable visible window. An unavailable
ID fails without selecting a different window. Then click a visible point:

```json
{"operation":"click","observation_id":"RETURNED_ID","image_index":0,"x":123,"y":45}
```

This route uses ScreenCaptureKit (macOS 14.2+) and the real pointer. It requires a
visible target and is unavailable in bounded mode. For ordinary window capture:

```js
const app = await cua.getApp({pid:PID});
return await app.getState({include_accessibility_tree:false,include_screenshot:true,max_image_dimension:1600});
// In the next call: await app.click([x,y], {capture_id:RETURNED_CAPTURE_ID});
```

Use `include_image:true` on exec, or fetch `result` with `part:"image"` and the
original receipt ID. Measure the exact returned PNG; do not multiply by display
scale or mix these credentials:

| Observation | Click binding | Lifetime |
| --- | --- | --- |
| Native window screenshot | `capture_id`, window PNG pixels | One pixel action |
| macOS `operation:app` | `observation_id` + `image_index`, frame PNG pixels | 30 seconds, one attempt; another execution invalidates it |
| `operation:desktop` | `capture_id`, primary display PNG pixels via `cua.call("click",{target:{kind:"desktop",display_id:"primary"},x,y,capture_id})` | One pixel action |

On Windows, a `capture_id` does not prove pixel alignment. DPI-unaware windows
at 125% scaling can have black padding and UIA frames displaced from the image.
Check alignment before pixel input; stop that route on a mismatch. Use fresh
semantic controls when available, or an authorized desktop observation. Never
repair the coordinates by guessing a scale factor.

For a new modal, rebind by parent PID first, then try PID-scoped `app` if available.
On Windows, `foreground_unavailable` can follow a click that already opened a
picker. Enumerate the parent's windows and observe the dialog before continuing;
do not repeat the opening click. A semantic modal trigger can also remain pending:
poll its original receipt, and if cancelled, restart the Runtime and re-observe.
Use full-desktop scope only when the task authorizes it and app-scoped observation
cannot reveal the target. [RECIPES.md](RECIPES.md) covers pickers, regions, native
menus and advanced click options; [api.d.ts](api.d.ts) has the exact signatures.

## Receipts and recovery

- `status=running`: poll `{"operation":"result","id":"ORIGINAL_ID"}`; do not resend.
- `runtime_busy`: wait for the active receipt.
- `runtime_context_reset`: idle handles and script state were reset; the requested
  action was not dispatched. Discard old bindings and observe again. Native grants
  are not renewed. Continuous use has no DTW absolute-age cutoff.
- A dispatched job's `cancel` ends the Runtime; its input effect is unknown.
  Restart and observe. Cancelling a request before dispatch leaves the Runtime usable.
- `result_expired`: payloads age out after 15 minutes/capacity limits; do not replay.
- `dtw_truncated` with `more`: follow the returned result-page call only if needed.
  `part:"structured"` retains full raw native diagnostics; `part:"image"` returns PNGs.
- Omit `session`; the Runtime owns it. Ref and capture IDs come from fresh observations.

`permissions` checks OS readiness; `health_report` returns native diagnostics.
`get_guide` is upstream reference material and may describe CUA-only entrypoints;
DTW `describe` and returned `next_call` define this adapter's callable interface.
