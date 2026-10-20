---
name: desktop-world
description: Control local macOS and Windows desktop apps through native UI automation and screenshots for tasks that require reading or operating app UI. Prefer purpose-built connectors, APIs, CLIs, or file tools when available.
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

Follow this 4-step loop for every desktop interaction:

### 1. Initialize & Bind Target

Keep discovery scoped to the authorized PID; a global `listWindows()` can expose unrelated titles.

```js
// Bind by PID (follows active front window, including file dialogs)
const app = await cua.getApp({ pid: TARGET_PID });

// Bind by PID and exact title (avoids auxiliary popup / translation panels)
const app = await cua.getApp({ pid: TARGET_PID, title: "Exact Window Title" });
```

### 2. Observe State

Choose the appropriate observation route based on interface type:

```js
// Route A: Semantic AX Tree (preferred for standard controls, forms, and dialogs)
const s = await app.getState({ query: "Save", max_elements: 80, include_screenshot: false });
return { window_id: app.windowId, tree: s.tree_markdown, truncated: s.truncated };

// Route B: Visual-First (for large browser pages, canvases, or text-heavy views)
const s = await app.getState({ include_accessibility_tree: false, include_screenshot: true, max_image_dimension: 1600 });
return { window_id: app.windowId, capture_id: s.capture_id, width: s.screenshot_width, height: s.screenshot_height };
```

*Tip*: Add `include_image: true` to the initial `exec` if you intend to inspect screenshots immediately within `wait_ms`. For ongoing executions, fetch via `{"operation":"result", "id": ORIGINAL_ID, "part":"image"}`.

### 3. Act

- **Semantic Controls**: Use exact element names or tokens (`app.click`, `app.typeText`).
- **Visual Pixels**: Measure coordinates directly on the returned PNG pixel grid and pass the one-use `capture_id`.

```js
// Semantic actions
await app.click({ name: "Save" });
await app.typeText("hello", { name: "SearchField" }); // NOTE: text first, selector second
await app.pressKey("Return");

// Coordinate action bound to current screenshot
await app.click([x, y], { capture_id: CAPTURE_ID });
```

### 4. Verify & Loop

Inspect the updated UI before taking further actions. **Never replay inputs or add automatic fallback clicks without observing first.**

---

## Dual-Track Decision & Platform Matrix

| Scenario | Recommended Route | Primary APIs & Strategy |
| :--- | :--- | :--- |
| **Standard Native Controls** | **Semantic (AX/UIA)** | `app.query({name: "..."})`, `app.click({name: "..."})`. Stable across window resizing and DPI scaling. |
| **Canvas / Rich Editor / Web** | **Visual-First** | `getState({include_accessibility_tree: false, include_screenshot: true})` -> inspect PNG -> `app.click([x, y], {capture_id})`. |
| **Crowded Window Screenshot** | **Region Candidates** | Filter AX controls inside a bounding rectangle. See [Region Candidates Recipe](RECIPES.md#region-candidates-from-a-window-screenshot). |
| **Native Modal / File Chooser** | **Rebind or Desktop Scope** | Rebind by parent PID without `windowId`. If not exposed in AX, use `operation: "desktop", include_image: true`. |
| **macOS Native Application Observation** | **Native `app` Op** | On macOS, `operation: "app", pid: <PID>` returns attributed upper-window images. Click via `operation: "click", observation_id, image_index, x, y`. |
| **macOS Native Menus** | **Native `menu` Op** | Pass-through `operation: "menu", pid, window_id, path: ["File", "Open..."]`. |

### Platform Key Differences
- **macOS**: Actions expose `AXPress`; standard shortcuts use `cmd` (e.g. `cmd+s`, `cmd+shift+g`).
- **Windows**: Actions expose `invoke`; standard shortcuts use `ctrl` (e.g. `ctrl+s`). Windows omits `screenshot_frame_valid`: verify `capture_id && width > 0 && height > 0 && !screenshot_error`, and visually check candidate frames against 125% DPI display scale.

---

## Common Methods & Native Operations

### CUA Script Methods (`app.*` & `cua.*`)

| Method | Signature / Usage | Notes |
| :--- | :--- | :--- |
| `app.query` | `app.query({ name: "Save" })` | Broad queries capped at 50 results. |
| `app.waitFor` | `app.waitFor({ name: "Save" }, { timeout_ms: 3000 })` | Waits for element appearance. |
| `app.click` | `app.click(target, { capture_id })` | Target can be selector, token, or `[x, y]`. |
| `app.typeText` | `app.typeText("text", target)` | **Text is first parameter**. `\n` simulates Return. |
| `app.setValue` | `app.setValue(target, "text")` | **Selector is first parameter**. Writes AX value directly. |
| `app.pressKey` | `app.pressKey("cmd+s")` | Single key or modified key stroke. |
| `app.hotkey` | `app.hotkey(["cmd", "shift", "g"])` | Key sequence combination. |
| `app.scroll` | `app.scroll([x, y], "down", 3)` | Directions: `"up"`, `"down"`, `"left"`, `"right"`. |
| `app.drag` | `app.drag([x1, y1], [x2, y2], { delivery_mode: "foreground" })` | Drag from one coordinate to another. |
| `app.zoom` | `app.zoom([x1, y1, x2, y2])` | Crop view; subsequent click uses `{ from_zoom: true }`. |

### Native Atomic Operations (via `exec` params)

- **Application Observation (macOS)**: `{"operation":"app", "pid": 123, "max_windows": 2}` lists attributed windows and returns up to 2 upper-window images. Click via `{"operation":"click", "observation_id": ID, "image_index": 0, "x": px, "y": py}`.
- **Menu**: `{"operation":"menu", "pid": 123, "window_id": 456, "path": ["File", "Open..."]}` triggers native macOS menus.
- **Desktop Snapshot**: `{"operation":"desktop", "include_image": true, "max_image_dimension": 1600}` captures full primary display.
- **Result & Pagination**: For `status=running` executions, query via `{"operation":"result", "id": ID, "part": "summary"|"text"|"structured"|"image", "offset": 0}`.

---

## Critical Guardrails & Error Recovery

1. **One-Use Capture ID**: `capture_id` binds to a specific screenshot and is consumed by its first pixel action. A second click with the same ID returns `capture_not_found`. Always observe anew before subsequent clicks.
2. **Stale Tokens**: Element tokens expire when pages navigate, windows change, or elements re-render. If CUA rejects a token, re-observe before acting.
3. **Modal Window Closing**: If a script reports `window_not_found` after a click in `calls before the error took effect`, the modal dialog likely closed successfully. Check resulting windows; never replay the button click.
4. **Window Titles vs Headings**: `app.title` is the OS-level window title, not in-page document headings.
5. **No Blind Replay**: On timeouts, `input_outcome_unknown`, or errors, reconcile the actual UI state rather than blindly repeating destructive inputs.

---

## Specialized References

Consult the accompanying reference files as needed for specific task patterns:
- [RECIPES.md](RECIPES.md): Copyable code patterns for file pickers, large web pages, canvas editors, ROI control discovery, and native menus.
- [api.d.ts](api.d.ts): Complete TypeScript signatures and interfaces for `cua`, `app`, and `exec` options.
- [references/safety-policy.md](references/safety-policy.md): User confirmation rules for high-stakes actions, sensitive data, and destructive operations.
