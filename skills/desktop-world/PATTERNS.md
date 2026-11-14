# Interaction patterns

These patterns apply to both surfaces. Discover/list tools first: if `exec` is
present, use the JavaScript examples; otherwise call the identically named listed
tool with the same object fields plus `app`. For example,
`await app.readText({target:'w1.e12'})` becomes
`readText({app,target:'w1.e12'})`. Atomic tools execute no JavaScript. Each call
has its own settlement boundary; related focus/menu steps may require one script.

## Observe when needed; batch known actions

A script does not need an initial look when its next action is already known:

```javascript
await app.click({selector:{role:'button',text:'Continue'}});
```

Use look/capture to decide unknown actions or verify task effects. App context
survives between exec calls, and input returns the current observation automatically.

Each action is sent once. DTW handles short native readiness checks and routes
within the authorized app; script completion returns the latest view. UI work
can settle asynchronously: perform another look when the expected change has
not appeared. A successful native dispatch alone does not prove the result.
If `observation.status` is `partial` or `failed`, the automatic follow-up read
was incomplete. Check `effects.dispatch`, then use a read-only observation to
verify what happened before choosing new input.

`click({target:id})` can focus an editable text control when it advertises `press`.
On Windows a field with `focused:true` can take keyboard input. To focus a field
without `press`, use a coordinate click from the current capture.
A text field without an advertised `press` action needs a visual click before
`typeText`. A proven undelivered semantic rejection can return an image for this
fallback; it never repeats a potentially delivered action.

## Visual fallback

```javascript
await app.capture({});
// Inspect the returned PNG before choosing coordinates in the next exec.
```

```javascript
await app.click({x:420,y:260});
```

Use image dimensions returned by DTW, even if the viewer resizes the preview.
DTW maps the app canvas to the topmost authorized surface internally. A stale
frame or unrelated occluder can reject input. Capture again and inspect before
new work; never repeat a possibly delivered action to wait for its effect.
Known pixel steps may open and dismiss same-app popups without inserting a new
capture between every step. Points hit the current top surface of their original
app instance while the canvas mapping remains valid. Coordinates do not identify
a control: inspect newly revealed UI when the next action is not already known.
Semantic IDs still identify their original object and never become a popup target.

Semantic failure can trigger a screenshot fallback. An image error is explicit;
a partially observed semantic tree cannot prove absence or unique targeting.

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

## App menus

```javascript
await app.look({scope:'menu',text:'File'});
```

Menu inspection uses the native app-menu scope without preparing input. Filter
by menu/command name, then click a freshly returned exact ID. Normal actions
prepare the authorized destination automatically. A command still needs a live,
enabled, app-owned target; incomplete discovery does not prove uniqueness.
Windowless menu extras and Dock roots are separate capabilities.

## Dialogs and application lifecycle

A look starts a stopped installed app on supported providers and waits briefly
for a window. A running app can legitimately have no visible window. This is an
observable state, not proof of a ready editing target. Look again if startup is
still settling; avoid repeated typing into unproven targets.

Popovers, menus and save sheets are app scene layers. Use their current semantic
target or capture coordinates. Normal quit can leave a save sheet; inspect that
fresh view. Closing a native window is distinct from quitting an app; use its
ordinary close UI or the platform's close-window chord when appropriate. Use an observed close control or the standard close-window chord.
Do not repeat an action with unknown effects.

## Input readiness

Keep related input in one script. Normal actions prepare the authorized
app/window automatically. Use `app.focus()` only when explicit focus is part of
the task, with follow-up input in the same `exec`. On Windows, opening a native
menu and choosing its item must also share one `exec`: foreground restoration
at settlement can dismiss the menu. Separate atomic calls do not share this scope.

On Windows, after a click opens a form, observe the intended field and pass its
ID as `target` in `typeText({target,text})`. The script stops on a focus mismatch
before it can submit to the wrong field.

DTW checks identity, destination and input cleanup internally. If the user
switches the selected app/window during a script, further input stops. Inspect
the new state in a later execution; never replay an unknown input. A confirmed
quit or current-window closure ends that script's input scope; an unresolved
save prompt does not establish termination.

## Errors and interruption

There is no public Session or receipt-query tool. Native references and history
belong to DTW. Standard MCP cancellation preserves its original execution's
identity. Identical request retransmission does not replay retained work; new
requests are new work. After an unknown effect, inspect current state before
new input. Missing history never proves an action had no effect. A native
cleanup failure closes desktop dispatch in that connection; reconnect only
after the host has settled it, then perform a fresh look rather than replaying.
On macOS, semantic `AXPress` allows up to 2 seconds for its reply within the
remaining request deadline. An AX error or timeout after dispatch has unknown
effect; a positive acknowledgement alone does not prove the UI change.

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
await app.keypress({keys:['META','N']});
await app.waitForNewWindow({});
await app.typeText({text:'New document text'});
```

The wait also works in the next exec. A timeout stops with the latest view;
it does not resend the shortcut.
