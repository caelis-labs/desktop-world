---
name: desktop-world
description: Use DTW's one exec tool to inspect and operate local desktop apps through CUA.
---

# Desktop World

Call `exec` with `{"session":"task-name","script":"..."}`. It immediately returns an `execution_id`; query the **same** tool with `{"session":"task-name","operation":"result","execution_id":"..."}` until terminal. `status` and `cancel` use that original id. A cancel request is pending until the terminal receipt says what happened; never replay an unknown action.

Scripts are CUA `run_script` bodies, with `cua` and a JSON `state` object. No filesystem, network, process or Node API is available. Mutations to `state` persist for this Session after successful completion. For an exact target, use `const windows=await cua.listWindows({pid:123}); return windows;`, choose its window ID by title, then `const app=await cua.getApp({pid:123,windowId:456}); const view=await app.getState({max_elements:100}); return {pid:app.pid,window_id:app.windowId,tree:view.tree_markdown,snapshot_id:view.snapshot_id};`. For the next action, bind the same exact PID/window ID and call `await app.click({role:"button",name:"Save"})`. Other native methods include `app.query({role:"button",name:"Save"})`, `app.typeText("text", target)`, `app.pressKey("cmd+s")`, `app.drag([x1,y1],[x2,y2])`, and `app.verify(...)`. `return` the small facts needed for the next decision. `app.getState()` includes a grounding screenshot by default; read the corresponding image only when needed.

Use `{"operation":"permissions"}` for a promptless host permission read. Use `{"session":"task-name","operation":"cursor","enabled":true}` to show that Session's virtual cursor, omit `enabled` to inspect it, and set `false` when done. Cursor operations also return an `execution_id` and use `result` for the terminal receipt.

`result` returns a bounded summary. Request `part:"structured"` or `part:"text"` with `offset` to page the full receipt, or `part:"image"` with `image_index` to view a retained screenshot. Screenshot and large trees are kept out of the ordinary text reply. An input receipt is delivery evidence; read back the changed UI or file before claiming task success. After a stale element, observe again. macOS permissions belong to the actual host; a permission refusal is not an action.

POC limitation: CUA element tokens can cross Sessions inside one MCP process. Do not pass an element token between Sessions; use one MCP process per independent Agent until upstream binding is fixed.

For a canvas editor with no AX text element, get a screenshot in the same Session, inspect its retained image, then use `await cua.call("type_text", {pid,window_id,x,y,text,delivery_mode:"foreground"})` with a point on that editor. The pixel form focuses and types in one native action. If background keys fail, use `await cua.call("hotkey", {pid,window_id,keys:["cmd","s"],delivery_mode:"foreground"})` on the exact window and verify the saved file. A foreground action briefly activates the target, then restores the prior app.
