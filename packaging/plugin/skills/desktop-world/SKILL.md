---
name: desktop-world
description: Control local macOS and Windows desktop apps through native UI automation and screenshots for tasks that require reading or operating app UI. When desktop UI operation is not explicitly requested, prefer purpose-built connectors, APIs, CLIs, or file tools.
---

# Desktop World

Control and inspect desktop applications on macOS and Windows through native accessibility trees (AX/UIA) and visual screenshots.

## Execution Environment & Rules

- **Tool Entrypoint**: Invoke the installed DTW exec tool (named `desktop-world_dtw/exec` in Antigravity) with `{"script":"..."}` or native atomic operations (`app`, `click`, `menu`, `desktop`, `focus`).
- **Sandbox**: Scripts run inside upstream CUA `run_script` via native QuickJS, with `cua` plus a JSON `state` object persisted across successful calls.
- **No Host APIs**: Node.js, `npm`, filesystem (`fs`), network, and process APIs are **NOT** available inside scripts. Use `exec` to inspect app content and verify UI effects; do not switch to a host file tool to inspect an active document. (A test harness may independently verify disk files outside the Agent turn).
- **Verification Mandate**: Action receipts prove delivery only, not UI success. **Always observe the resulting UI after an action before deciding the next step.**

---

## Standard Workflow (SOP)

Follow this 4-step loop for every desktop interaction. Each snippet is an independent, runnable script:

### 1. Initialize & Bind Target

Keep window discovery strictly scoped to the authorized PID; a global `listWindows()` can expose unrelated titles.

```js
// Option A: Bind by PID (follows the application's active front window, including pickers)
const app = await cua.getApp({ pid: TARGET_PID });
return { pid: app.pid, window_id: app.windowId, title: app.title };
```

```js
// Option B: Bind by PID and exact title (avoids auxiliary popup or translation panels)
const app = await cua.getApp({ pid: TARGET_PID, title: "Exact Window Title" });
return { pid: app.pid, window_id: app.windowId, title: app.title };
```

### 2. Observe State

Select the observation route based on interface type:

```js
// Route A: Semantic AX Tree (preferred for standard controls, forms, and dialogs)
const app = await cua.getApp({ pid: TARGET_PID });
const state = await app.getState({ query: "Save", max_elements: 80, include_screenshot: false });
return { window_id: app.windowId, tree: state.tree_markdown, truncated: state.truncated };
```

```js
// Route B: Visual-First (for large browser pages, canvases, or text-heavy views)
const app = await cua.getApp({ pid: TARGET_PID });
const state = await app.getState({ include_accessibility_tree: false, include_screenshot: true, max_image_dimension: 1600 });
return { window_id: app.windowId, capture_id: state.capture_id, width: state.screenshot_width, height: state.screenshot_height };
```

*Tip*: Add `include_image: true` to the initial `exec` parameters if you intend to inspect screenshots immediately within `wait_ms`. For ongoing executions, fetch via `{"operation":"result", "id": ORIGINAL_ID, "part":"image"}`.

### 3. Act

Execute one purposeful action at a time:

```js
// Action A: Semantic click
const app = await cua.getApp({ pid: TARGET_PID });
return await app.click({ name: "Save" });
```

```js
// Action B: Text input and single key (NOTE: text first, selector second)
const app = await cua.getApp({ pid: TARGET_PID });
await app.typeText("search term", { name: "SearchField" });
return await app.pressKey("Return");
```

```js
// Action C: Coordinate click bound to a fresh screenshot
const app = await cua.getApp({ pid: TARGET_PID });
return await app.click([x, y], { capture_id: CAPTURE_ID });
```

Click options follow the pinned CUA 0.38.0 component. DTW accepts either
`modifiers: ["ctrl"]` or native `modifier: ["ctrl"]`; a string such as
`modifier: "ctrl"`, an unknown key name, or both spellings is rejected before input.

| Method | Options beyond delivery_mode | Screenshot binding |
| --- | --- | --- |
| `click` | `button`, `modifiers` / `modifier`, `from_zoom`; `count` of 1, 2, or 3 with a Point (route limits below) | `capture_id` with a Point; cannot combine with `from_zoom` |
| `rightClick` | `modifiers` / `modifier` with a Point; `from_zoom` on Windows only | None; use `click(point, {button: "right", capture_id})` |
| `doubleClick` | `from_zoom` on Windows only | None |
| `typeText` | `delay_ms` (0–200; default 30, synthesis routes only) | None |
| `scroll` | `by: "line"` or `by: "page"` | None |
| `drag` | `duration_ms`, `steps`, `button`, `from_zoom` | None |

On macOS, modified `click` requires an exact window binding
(`cua.getApp({pid: PID, windowId: WID})`) and `delivery_mode: "foreground"`.
DTW never changes delivery mode automatically. Windows preserves the native `from_zoom` conversion for pixel `rightClick` and
`doubleClick` after `zoom`; macOS rejects that option. Native `rightClick` modifiers and
Windows modifier delivery have upstream limitations; verify the resulting UI.
Windows accepts double-click modifiers without applying them; DTW rejects that
option on all platforms. `cua.call` applies the same click argument checks.
Native `operation=click` remains a single unmodified left click with its own
`observation_id`; it does not accept these JS options.

For a screenshot-bound double click, use
`app.click([x,y], {capture_id: CAPTURE_ID, count: 2})`. This is one native
gesture consuming one capture, not two attempts with the same credential.
The native schema accepts `count:3`, but macOS Chromium background left clicks
are capped at two; macOS background right/middle clicks ignore `count`.
Do not infer a triple click or repeated right click from an accepted option.
Native `drag` supports background delivery on some controls; macOS drag-and-drop
sessions and some Windows controls still require explicit foreground delivery.
Inspect the result and UI before changing route. `getState({verbose:true})`
exposes the native macOS `background_input` report when troubleshooting;
`full_output:true` expands tree metadata and should not be combined with `since`.

### 4. Verify & Loop

Inspect the updated UI before taking further actions. **Never replay inputs or add automatic fallback clicks without observing first.**

---

## Observation Hierarchy for Modals & Pickers

When dealing with native file pickers, modal alerts, or newly opened panels, **never jump directly to full-desktop capture on the first AX failure**. Follow this strict priority ladder:

1. **Tier 1 — Rebind Parent PID**: Rebind using `cua.getApp({ pid: PARENT_PID })` without `windowId`. Unpinned CUA apps follow the frontmost window, exposing the picker's real controls.
2. **Tier 2 — macOS PID-Scoped App Observation**: On macOS, call native `{"operation":"app", "pid": PARENT_PID}`. It discovers all attributed visible windows, checks for occluders, and returns upper-window images within the authorized process.
3. **Tier 3 — Window Region Candidates**: If a window binds but has a crowded tree, mark a bounding box in its PNG and query [Region Candidates](RECIPES.md#region-candidates-from-a-window-screenshot) to discover AX controls from image pixels.
4. **Tier 4 — Authorized Desktop Fallback**: Only if the above application-scoped routes cannot reveal the picker AND the task explicitly authorizes desktop scope, call `{"operation":"desktop", "include_image": true}`. Visually verify the intended panel before dispatching any coordinate action.

---

## Three Screenshot Input Routes & Lifecycles

Different screenshot operations bind to different input mechanisms and lifecycles. **Never mix coordinates or credentials across routes:**

| Route | Observation Source | Binding Credential | Action Dispatch | Coordinate Grid | Lifecycle & Invalidation Rules |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **CUA Window** | `app.getState({include_screenshot: true})` | `capture_id` | `app.click([x, y], { capture_id })` | Window-local PNG pixels | **One-use**: consumed by the first pixel action. Subsequent clicks return `capture_not_found`. Re-observe for new clicks. |
| **macOS Native App** | `{"operation":"app", "pid": PID}` | `observation_id` + `image_index` | `{"operation":"click", "observation_id": ID, "image_index": 0, "x": px, "y": py}` | Frame PNG pixels | **30s TTL & One-use**: consumed by one click attempt. Inspect `frames`, `occluders`, `omitted_count` first. Any new script or native operation invalidates it (reads do not). Do not fabricate CUA capture IDs for SCK frames. |
| **Desktop Snapshot** | `{"operation":"desktop", "include_image": true}` | `capture_id` | `cua.call("click", { target: { kind: "desktop", display_id: "primary" }, x, y, capture_id })` | Primary display PNG pixels | **One-use**: consumed by first pixel action. CUA maps to screen points automatically; **do not multiply by display scale**. |

> **Warning on Coordinate Grids**: Measuring coordinates on a resized chat preview, a different window, or mixing window-local coordinates with desktop coordinates will cause misclicks. Always measure against the exact PNG returned by that specific observation receipt.

The macOS application route requires macOS 14.2+. Every frame uses an exact
ScreenCaptureKit window with child composition disabled. Its click moves the
real pointer and posts one left click at a verified visible point; it does not
activate a hidden window. This differs from native CUA background delivery.
Read the resulting UI after it settles; a posted event is not verified effect.

## Native Browser and Diagnostics

CUA's typed browser tools are available as `exec` operations, outside scripts.
Start with `{"operation":"describe","name":"get_browser_state"}` for the
worker's current schema, then use `{"operation":"get_browser_state",
"arguments":{"pid":PID,"window_id":WID}}`. Preserve the returned opaque
`target_id` and `tab_id`. Use `snapshot_format:"semantic_v2"` for actionable
refs and optional exact-tab screenshots. A newer snapshot invalidates old refs.

Read [BROWSER.md](BROWSER.md) for preparation, input, dialogs and files. Existing
profile attachment uses a host-reviewed native capability manifest approved at
DTW startup. Agents cannot grant or widen it through exec. Downloads require a
fresh MCP host form confirmation for the exact request and destination; CUA still
checks the live ref and native file scope. Unsupported hosts fail closed. See
BROWSER.md for launcher configuration and the remaining dynamic-callback limitation.
Do not switch to unrestricted mode to satisfy a refusal. An explicitly requested
isolated profile can be prepared with `browser_prepare`.

Use `{"operation":"health_report","arguments":{}}` for read-only runtime
diagnostics; `permissions` remains the short OS-permission check. Use
`{"operation":"get_guide","arguments":{"topic":"browser"}}` for the
pinned upstream guide. Guides may mention tools DTW intentionally does not expose;
`describe` identifies the available native contract.

---

## Execution Lifecycle & Recovery Protocol

- **`status=running`**: The task is still active. Do NOT re-send the script. Query the original execution ID using `{"operation":"result", "id": ID, "part": "text"|"image"|"structured"}`.
- **`runtime_busy`**: Another execution is currently running. Inspect its receipt and wait; do not issue concurrent executions.
- **`runtime_context_reset`**: State and native handles have expired or reset. Discard old window IDs, tokens, and capture IDs; re-observe target application from scratch.
- **`cancel`**: Canceling terminates the worker and makes the Runtime unusable. Dispatched inputs are in an `unknown` state (never assumed undone). Restart the Runtime and observe the physical UI before deciding on new actions.
- **`result_expired`**: Completed results expire after 15 minutes or capacity limits. The receipt status is retained, but payloads are pruned. This is not permission to replay.
- **No `session` Parameter**: Do not pass a `session` argument. One MCP process owns one CUA session and JSON state; deprecated session parameters are rejected.
- **`window_not_found` after modal clicks**: If a script reports `window_not_found` after a successful `click` in its `calls before the error took effect`, the modal dialog likely closed as expected. Rebind by PID or observe to confirm; do NOT replay that click.
- **Stale Tokens & Timeouts**: If CUA rejects an element token or an input times out (`input_outcome_unknown`), reconcile the actual UI state through observation rather than blindly re-dispatching input.

---

## Platform Nuances

- **macOS**: Button actions expose `AXPress`. Shortcuts use `cmd` (e.g. `cmd+s`, `cmd+shift+g`). Native menus are accessible via `{"operation":"menu", "pid": PID, "window_id": WID, "path": ["File", "Save"]}`.
- **Windows**: Button actions expose `invoke`. Shortcuts use `ctrl` (e.g. `ctrl+s`). Windows omits `screenshot_frame_valid`: manually verify `capture_id && screenshot_width > 0 && screenshot_height > 0 && !screenshot_error`. Check candidate alignment visually on 125% DPI displays before pixel interaction.
- **Web text**: Native CUA receipts may suggest `page` or `browser_*`. Use the
  typed `browser_*` exec operations; legacy `page` is not exposed. Reobserve the field
  to establish whether text landed before changing input routes.
  Do not repeatedly call an unavailable tool or replay unverified text. AX/UIA
  value echoes in Chromium/Electron do not establish renderer acceptance.

---

## Specialized References

Consult accompanying reference documents as needed:
- [RECIPES.md](RECIPES.md): Concrete implementations for modal choosers, large browser pages, region candidates ROI, canvas editors, and native menus.
- [BROWSER.md](BROWSER.md): Exact-tab browser tools, native authorization and diagnostic workflows.
- [api.d.ts](api.d.ts): Authoritative TypeScript signatures and type definitions for `cua`, `app`, and `exec` options.
- [references/safety-policy.md](references/safety-policy.md): Host policy precedence, confirmation modes, and sensitive data protection.
