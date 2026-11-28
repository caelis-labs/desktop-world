# Interaction patterns

Use `exec({app,source})` to run the JavaScript examples. Copy `app` from `apps`
and pass the example’s JavaScript body as `source`.
Settlement leaves foreground, focus and pointer changes in place.

[Visual fallback](#visual-fallback) · [Editing](#replace-a-value-or-trigger-typing-behavior) ·
[Reading text](#relevant-content-and-full-text) · [Menus](#native-app-menus-on-macos-and-windows) ·
[New windows](#new-document-windows) · [Recovery](#errors-and-interruption)

## Observe when needed and batch known actions

A script does not need an initial look when its next action is already known:

```javascript
await app.click({selector:{role:'button',text:'Continue'}});
```

Use `look()` for native elements or `capture()` for pixels when choosing unknown
actions or verifying effects. App context persists across exec calls. Input
normally gets an automatic readback in the outer `view`.

Each action is sent once. DTW handles short native readiness checks and routes
within the authorized app; script completion returns the latest view. UI work
can settle asynchronously: perform another look when the expected change has
not appeared. A successful native dispatch alone does not prove the result.
If `observation.status` is `partial` or `failed`, the automatic follow-up read
was incomplete. Check `effects.dispatch` and the readable parts of the view.
Observe again if the requested effect is still unclear.

`click({target:id})` can focus an editable text control when it advertises `press`.
On Windows a field with `focused:true` can take keyboard input. To focus a field
without `press`, use a coordinate click from the current capture.
A proven undelivered semantic rejection can return an image for this fallback.
Check dispatch before choosing another action.

## Match a native name and wait for state

```javascript
const note = app.getByRole('text_field',{name:'Note',exact:true});
await note.setValue('Ready');
await note.waitFor({value:'Ready',timeout:2000});
const save = app.getByRole('button',{name:'Save',exact:true});
await save.waitFor({state:'enabled',timeout:2000});
await save.click();
```

Each use resolves the query again. `Save` will not match `Save As` here. Two
buttons both named `Save` are still ambiguous; inspect the returned view rather
than taking the first. The field readback verifies the local value, not the save.
Check the application's confirmation or persisted result without repeating the click.

## Replace a value or trigger typing behavior

Identify the field and its actions with `look()`. For a field advertising
`setValue`, replace its native value and read it back.

```javascript
const field = {role:'text_field',text:'Name'};
await app.setValue({selector:field,value:'Updated name'});
await app.look(field);
```

Compare the control's `value` in the outer `view` with the requested value.
Verify a save from the application's confirmation or persisted content.

For a search field, Go to Folder path completion, or a Web form whose Submit
button reacts to input events, click the proven editable receiver and type:

```javascript
await app.click({target:'w1.e12'}); // Requires advertised press; otherwise capture/click.
await app.typeText({text:'example'});
```

Inspect completion/results or the Submit state before continuing. A setter can
read back the correct value while those application behaviors never ran.
Targets in examples stand for IDs from the actual observation. Build the
`exec` argument object with a JSON serializer when embedding user text in
`source`; do not interpolate unescaped text into JavaScript.

## Visual fallback

If `look()` already attached a clear screenshot, use it for the next point.
Otherwise use a pixel-only capture. A smaller overview is useful for layout.

```javascript
await app.capture({max_dimension:800});
```

For a small icon, ask for the relevant part of the full app image. This
example crops the bottom-right quarter.

```javascript
await app.capture({region:{x:0.5,y:0.5,width:0.5,height:0.5}});
```

Inspect the returned PNG, then choose a point in the next `exec`. If the
target is at the center of that crop, use this call.

```javascript
await app.click({x:0.5,y:0.5,unit:'fraction'});
```

Fractional points use the returned image or crop, excluding margins around a
preview. For raw pixels use `image.width` and `image.height`. DTW maps the point
through crop and scaling. Each new crop uses the full canvas as its reference.
Use `capture()` for a full image again, or `look()` to switch the automatic
readback back to native elements when available.

Inspect the automatic image after input before choosing a newly revealed
control. Use the new mapping when `image.coordinate_reset:true` appears.
Unrelated changes, such as new messages away from a fixed composer button, do
not require a new point. If the control itself moves within the app, locate it
again from the updated image. A complete semantic query can miss self-drawn
controls. Use a visual read to check them.

## Relevant content and full text

On a crowded page, narrow the native traversal instead of repeating the same
bounded preview:

```javascript
await app.look({role:'static_text',text:'order number'});
```

Inspect the returned targets, then request only the text the task needs:

```javascript
return await app.readText({target:'w1.e12'});
```

A page returns `text`, `next_offset`, `total_length` and `unit:'utf16'`. Continue
with `{target:'w1.e12',offset:page.next_offset}` while it is non-null; do not insert
a new look between pages of one revision. A fresh look establishes new content.
Keep reads bounded by the task and return/print the relevant part rather than
collecting a whole document into the tool output. A field's truncation marker or
revision is not proof of its omitted text. Unsupported text can still use images;
permission/identity failures require recovery before more work.

## Native app menus on macOS and Windows

```javascript
await app.look({scope:'menu',text:'File'});
```

On macOS and Windows, menu inspection reads native menus without preparing input. Filter
by menu/command name, then click a freshly returned exact ID. Normal actions
prepare the authorized destination automatically. A command still needs a live,
enabled, app-owned target; incomplete discovery does not prove uniqueness.
Windowless menu extras and Dock roots are separate capabilities.

For a known native menu path on macOS, call e.g.
`await app.invokeMenu({path:['File','Export','PDF']})`. This distinguishes two
commands named `PDF` under different parents. To open a control's context menu,
use `await app.showMenu({target:id})` only on a control advertising `showMenu`;
inspect the actual menu before choosing an item. For a native popup use
`await app.selectOption({target:id,option:'Finance'})` and verify the new choice.
Neither action substitutes a coordinate gesture for a stale or unsupported control.

## Dialogs and application lifecycle

A look starts a stopped installed app on supported providers and waits briefly
for a window. A running app can legitimately have no visible window. This is an
observable state, not proof of a ready editing target. Look again if startup is
still settling; avoid repeated typing into unproven targets.

Popovers, menus and save sheets may appear as app scene layers. Use their current
semantic target or capture coordinates. On macOS and Windows, normal quit can
leave a save sheet. On Linux X11, use the app’s ordinary close UI or its
close-window chord when appropriate.
Do not repeat an action with unknown effects.

## Input readiness

Keep related input in one script. Normal actions prepare the authorized
app/window automatically. Use `app.focus()` only when explicit focus is part of
the task. macOS prefers working background capabilities; when needed it uses
the foreground/real mouse directly. Windows and X11 use foreground input.
Foreground, focus and pointer changes stay in place. Verify menu and receiver state
before further input.

On Windows, after a click opens a form, observe the intended field and pass its
ID as `target` in `typeText({target,text})`. The script stops on a focus mismatch
before it can submit to the wrong field.

DTW checks identity, destination and input cleanup internally. If the user
switches the selected app/window during a script, further input stops. Inspect
the new state in a later execution; never replay an unknown input. A confirmed
quit or current-window closure ends that script's input scope; an unresolved
save prompt does not establish termination.

## Errors and interruption

Read `failure.call`, `failure.operation` and `failure.dispatch` when present.
Aggregate `effects.dispatch` can also count earlier actions in the same script.
After an interruption or an unknown dispatch, observe the UI before issuing a
new action. A new MCP request executes new work.

| Result | Next observation |
| --- | --- |
| `capture_required`, `stale_scene`, `outside_app` | Inspect the attached image if available. Otherwise capture again. Choose a point from the current image. |
| `view.image.error` | The read produced no usable coordinate binding. Resolve the reported capture problem, or use a proven native control from `look()`. |
| `target_changed`, `selector_ambiguous`, `selector_unproven` | Use a targeted `look()` to find the current control. Inspect role, name, and scope before choosing an ID or locator. |
| `text_delivery_unconfirmed` | Inspect the intended field by value or screenshot before typing again or submitting. |
| `wait_timeout` after New/Open | Continue observing for the destination without repeating the action. |
| `observation.status` is `partial` or `failed` | Check dispatch evidence. Read again if the returned view cannot verify the requested effect. |

See [execution results](API.md#run-app-javascript) for receipt and cancellation
details, and [provider boundaries](API.md#provider-boundaries) for permissions
or platform-specific capture and input errors.

## Hover and content drag

Use `app.move({x,y})` with current image pixels to reveal hover UI; inspect its
returned observation before choosing another action. A drag is one transaction:

```javascript
await app.drag({path:[{x:120,y:180},{x:360,y:240}]});
```

No down/up state is managed by the script. The current path must stay within one
window on one display, avoiding overlays and transparent gaps. Cancellation or a new obstruction
releases the button and reports uncertainty; inspect before new input. A second
coordinate action can use the same image mapping when its target remains valid;
DTW refreshes and checks the relevant pixels internally. Inspect a new capture
when hover reveals a control you have not yet seen or the returned view reports
changed targets/geometry.

## Script snapshots and scrolling

```javascript
const view = await app.capture({});
print(view.windows.map(w => ({id:w.id, bounds:w.bounds})));
```

`look()` also returns all current metadata inside the script, even when its tool
message is `{kind:"unchanged"}`. Request `full:true` only to republish a tool baseline.

```javascript
await app.scroll({delta_y:80});
```

DTW captures and uses the active surface's center when no anchor is supplied.
For a nested scroll pane, first inspect a capture and provide its x/y. Deltas
are integer wheel steps between -100 and 100, not pixel distances.

A standard macOS/Windows title-bar drag uses native window position when proven and
writable. Use a current image, start in empty title-bar space away from buttons,
and keep the supplied path inside that window. The destination must fit one
display. Inspect fresh bounds/image afterward; moving a window resets coordinates.

## New document windows

Wait for a new window after an input that should create one. A delivered chord
alone does not prove that the app has finished creating the window.

```javascript
// Use the application’s New shortcut (CTRL+N here, commonly META+N on macOS).
await app.keypress({keys:['CTRL','N']});
await app.waitForNewWindow({});
await app.typeText({text:'New document text'});
```

The wait also works in the next exec. A timeout stops with the latest view;
it does not resend the shortcut.
