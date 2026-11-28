---
name: dtw
description: Use when a task requires inspecting or operating an app through its desktop interface.
---

# DTW

Use the `dtw` MCP tools from the `desktop-world` plugin. Find the target with
`apps({query})` and copy the exact `app` value into `exec({app,source})`.
`source` is an async JavaScript function body. Await each `app` operation.

## Choose an observation

- **Known action.** Start directly with a unique locator or a known keyboard
  shortcut. DTW prepares the receiver. Group known steps in one `exec`.
- **Controls or text.** Use `look({role:'button',text:'Save'})` to narrow the
  native query. Use an observed target ID or a unique locator such as
  `app.getByRole('button',{name:'Save',exact:true})`. For full text on a
  supported control, return `await app.readText({target})`.
- **Pixels.** Use `capture({max_dimension:800})` for a smaller overview.
  `capture()` skips native element discovery by default. If `look()` already
  supplied a usable image, work from it.

For a small control, use `capture({region:{x:0.6,y:0.75,width:0.4,height:0.25}})`
to inspect the bottom-right area. `region` is measured against the full app
image from its top-left. Inspect the returned PNG, then choose the point in a
later `exec`. With `unit:'fraction'`, x/y are measured within the returned
image or crop, in `[0,1)`. DTW converts image coordinates to desktop positions. Use the
new image when `image.coordinate_reset:true` appears.
[Visual examples and recovery](PATTERNS.md#visual-fallback)

## Verify the result

Inspect the outer `view`, `effects`, and any `error`. A dispatch receipt says
the input was sent. Verify the resulting field, window, confirmation or saved
content. Leave the script return empty for the normal view, or return a small
value when the task needs specific data.

Wait for the expected control or value when UI updates are delayed. Use
`waitForNewWindow()` after New/Open before sending input into the new window.
After an interrupted or unknown dispatch, inspect the current UI before
sending more input. If typing is unconfirmed, inspect the field before typing
again or submitting.

Use the [API reference](API.md#methods) for exact arguments and the
[interaction patterns](PATTERNS.md) for editing, dialogs, and recovery.
