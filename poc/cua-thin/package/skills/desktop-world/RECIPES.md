# Short CUA scripts for DTW

Each JavaScript block is the `script` string of `desktop-world_dtw/exec` with one stable `session`. The native code is upstream CUA `run_script`; DTW does not evaluate another JS language. Replace example PIDs, window IDs, labels and coordinates with values observed in this run. A script may branch between reads and actions. Return only facts the Agent needs; the complete native result and up to two images stay retrievable through the original `execution_id`.

## A chooser or newly opened window

Bind only the app PID after opening a dialog. An unpinned CUA `App` follows the app's current front window, including its picker; an exact `windowId` stays pinned to the old window. The user's persistent Obsidian picker was selected by PID-only binding and exposed `AXWindow "打开本地仓库" [id=open-panel]`. Confirm the title and controls before input.

```js
const app = await cua.getApp({pid: PARENT_PID});
const view = await app.getState({query:"打开",max_elements:100,include_screenshot:true});
return {pid:app.pid,window_id:app.windowId,title:app.title,
        tree:view.tree_markdown,snapshot_id:view.snapshot_id,
        has_image:view.screenshot_frame_valid};
```

If PID-only binding misses the chooser, compare `cua.listWindows({pid:PARENT_PID})` before and after. Keep discovery scoped to the authorized process; a global window listing can expose unrelated titles. A macOS Open/Save service window may be invisible while the parent app has a visible AX proxy. Use the parent-owned `(pid,window_id)` if its `getState` has the real controls. A returned ID that fails on `getApp` does **not** prove the picker closed: in real Obsidian acceptance it remained visibly open for minutes. Do not repeatedly bind that ID or click its stale element frame. Capture the primary display instead:

```json
{"session":"task","operation":"desktop","include_image":true,"max_image_dimension":1600}
```

Inspect the image in that response (or `result,part:image` if still running). Confirm the intended temporary picker by its visible title/path and buttons. Measure `x,y` in the **returned PNG pixel grid** (`capture.screenshot_width/height`), not in a resized chat preview, macOS screen points, or an earlier screenshot. Aim at the center of a large visible control. CUA maps a capped screenshot back to native screen points when the matching `capture_id` is supplied; do not multiply by display scale yourself. Then send exactly one pixel action with its one-use capture ID:

```js
return await cua.call("click",{target:{kind:"desktop",display_id:"primary"},
                               x:IMAGE_X,y:IMAGE_Y,capture_id:"CAPTURE_ID"});
```

Recapture the desktop and check the picker or parent form changed before another action. In the isolated Obsidian trial, two delivered desktop clicks near the picker’s Open button did not close it; a `press_key` Return while that same picker retained focus did. Prefer the focused default button via Return when visually confirmed. A delivered click with `effect:"unverifiable"` is not proof of activation. The desktop is shared: stop if another window covers the target or its identity is unclear. For Go to Folder **only while that picker has visible focus**, use `cua.call("hotkey",{target:{kind:"desktop",display_id:"primary"},keys:["cmd","shift","g"]})`; inspect the new sheet, enter the authorized path with `cua.call("type_text",{target:{kind:"desktop",display_id:"primary"},text:"/authorized/path"})`, then `cua.call("press_key",{target:{kind:"desktop",display_id:"primary"},key:"return"})`. A one-key `hotkey` is invalid. On macOS an action receipt only proves delivery; read the resulting visible path and UI. Do not send a global shortcut to the old main window or to uncertain focus.

After pressing `创建` or `打开`, the old modal window can close before the next line binds its pinned ID. A script can therefore return `window_not_found` **after** a successful click. Inspect `calls before the error took effect`, then use a fresh PID-only bind or desktop screenshot to look for the new window and verify the requested effect. Never replay the button just because the stale window read failed.

## Large browser page: visual first

Set `include_image:true` on the screenshot `exec` to receive the first image in that call when it finishes within `wait_ms`. If it is still running, fetch `operation:"result",part:"image"` using the original execution ID.

Page content with hundreds of elements need not be dumped into the model. A native screenshot can be the first observation, with a narrow AX query only when text or an exact control matters. This works without CDP, DOM injection, browser setup or a second MCP tool. It may require foreground delivery for a canvas/video control; read back the effect before changing route. If PID-only binding selects a Chrome translation or extension popup, use `cua.getApp({pid:CHROME_PID,title:"Expected window title"})` or locate the exact title in `cua.listWindows({pid:CHROME_PID})` and pin its `window_id`.

```js
const app = await cua.getApp({pid: CHROME_PID,windowId: WINDOW_ID});
const view = await app.getState({include_accessibility_tree:false,
                                 include_screenshot:true,max_image_dimension:1600});
return {window_id:app.windowId,capture_id:view.capture_id,
        width:view.screenshot_width,height:view.screenshot_height,
        has_image:view.screenshot_frame_valid};
```

Then request `{"operation":"result","execution_id":"ORIGINAL_ID","part":"image"}` in the **same** Session, inspect that original image, and click a point measured in its window-local pixels. If it was downscaled, pass the returned `capture_id` so CUA can reject a stale or mismatched frame and map coordinates. `app.click([x,y],{capture_id:CAPTURE_ID})` works when background delivery lands; use `{delivery_mode:"foreground",capture_id:CAPTURE_ID}` only for an authorized visible target. A capture ID is consumed by its first pixel action, even if a later line in that script fails. A second click with the same ID returns `capture_not_found` before dispatch. Read the actual page heading or control state after the first click; `app.title` only reports the window title. If another action is needed, take a fresh screenshot. After navigation or resizing, never reuse the old point. `app.zoom([x1,y1,x2,y2])` gives a cropped image. A following `app.click([x,y],{from_zoom:true})` uses **zoom-image** coordinates, not the full screenshot's. A screenshot-only read with `display_only:true` is a preview and cannot ground a click.

For a text-heavy target, prefer `app.getState({query:"target label",max_elements:100,include_screenshot:false})` or `app.query({name:"exact label"})`. `max_elements` truncation is not absence. `since:"latest"` returns a small diff after a prior read with the same options. Upstream's typed browser/CDP tools can be faster for permitted page-aware tasks, but `run_script` cannot call them; attaching a personal Chrome profile also has a separate authorization/setup boundary. This POC's one-tool path uses native screenshot/AX input.

## Canvas document editor

Some editors expose toolbar AX controls but draw the document text without AX text nodes. The WPS acceptance used one exact temporary window: screenshot the document, foreground-click a visually confirmed text position, move to the intended line end with an app/window hotkey, insert a line break, and use `app.zoom(rect)` to confirm the caret before typing. `app.typeText(text,undefined,{delivery_mode:"foreground"})` then typed into the focused canvas; a new screenshot showed the marker. An exact-window `app.pressKey("cmd+s",undefined,{delivery_mode:"foreground"})` saved it. Verify the app's visible saved state, and when the task requires disk proof use an independent file readback outside the desktop script. Coordinates, caret placement and keyboard behavior must come from the current document; do not reuse the fixture's points or infer success from `effect:"unverifiable"`.

## Native menu and system-wide controls

The upstream QuickJS allowlist excludes `invoke_menu` and `get_desktop_state`. The same DTW `exec` therefore has two narrow pass-through operations:

```json
{"session":"task","operation":"menu","pid":123,"window_id":456,"path":["File","Open..."]}
{"session":"task","operation":"desktop","max_image_dimension":1600}
```

`menu` delegates to CUA's exact native menu-path resolver. It may temporarily activate the target and is suitable only for an authorized target; read the resulting window state afterward. `desktop` captures the whole primary display with no AX walk, and its image is retrieved with `operation=result,part=image`. Use it only when the task requires the shared visible desktop. Pixel input on that image uses `cua.call("click",{target:{kind:"desktop",display_id:"primary"},x,y,capture_id})` in a later script; the screenshot's capture ID belongs to that Session. A global keyboard shortcut can use `cua.call("hotkey",{target:{kind:"desktop",display_id:"primary"},keys:["cmd","space"]})`, followed by a new `desktop` read. Such a shortcut acts on the **current global focus**, so do not send text until the intended field is visibly focused. Prefer an exact app/window hotkey when one exists.

To discover a localized menu path, first query the exact command label in one window read, for example `const s=await app.getState({query:"重新加载",max_elements:80,include_screenshot:false}); return s.tree_markdown;`. The result can show its enclosing `AXMenuBarItem` and `AXMenu`. Pass those observed labels to `operation:"menu"`; do not translate an English example path by guesswork.

## Fixed actions, verification and failures

```js
const app = await cua.getApp({pid: PID,windowId: WINDOW_ID});
const before = await app.getState({query:"Save",max_elements:80,include_screenshot:false});
const matches = await app.query({name:"Save"});
if (matches.length !== 1 || matches[0].enabled === false)
  return {acted:false,reason:"Save is absent or ambiguous",snapshot_id:before.snapshot_id};
const action = await app.click({name:"Save"});
const after = await app.getState({query:"Saved",max_elements:80,include_screenshot:false});
return {acted:true,delivery:action,after:after.tree_markdown,
        snapshot_id:after.snapshot_id};
```

This checks the target before acting, then reads once at the meaningful checkpoint. CUA's `effect:"unverifiable"` is not a success verdict. On stale references, permission refusal or `unknown`, observe/reconcile the original receipt; do not replay the action. Upstream `run_steps` does its own stale-token re-find/retry, so this POC does **not** surface it as an `exec` operation. Keep logic in `run_script`, where branches and the point of action remain explicit.
