# DTW app script reference

## Tools

`apps({query?, state?, offset?, limit?})`

- `query`: case-insensitive name/identifier substring, up to 256 characters.
- `state`: `all` (default), `running` or `stopped`.
- `offset`: nonnegative integer; `limit`: 1–100 (default 30).
- Returns `{apps:[{app,name,running,instances?}], next, coverage}`. Pass `next`
  as the next offset with the same query/state. Coverage describes searched
  locations; an empty result does not prove the app is not installed elsewhere.
- macOS searches standard application folders, including stopped bundles, and
  running user bundles elsewhere. Nested helpers and background-only bundles
  are excluded. Duplicate installed bundle identifiers use explicit bundle paths.
- Other providers currently report running-only discovery with `installed:false`;
  stopped-app discovery/startup is not claimed on those providers.

`exec({app, source})`

- `app`: the exact authorization identity returned by discovery; macOS also
  accepts an existing absolute `.app` path. It cannot change within a script.
- `source`: an async JavaScript function body, at most 64 KiB UTF-8. Await calls
  sequentially. No custom session, request ID, timeout or delivery arguments.
- A missing `app` or malformed argument object returns `invalid_arguments` before
  provider connection; an invalid app identifier returns `invalid_app`.
- Normal output is `{app, execution_id, status, view?, result?, output?, effects?}`.
  Errors retain their code, operation and source line when available. `effects`
  preserves dispatch/effect uncertainty; it does not prove the task succeeded.
  A returned value identical to the final view is coalesced into `view`, rather
  than repeated in `result`. Task-specific JSON values remain in `result`.
  The last current explicit/fallback PNG is a separate MCP image block.
- Output is bounded. `print` has 200 lines / 32 KiB aggregate, 2000 characters per
  line, and explicit truncation. Return JSON has a separate 64 KiB budget.
- A retransmitted identical MCP request ID refers to its retained execution;
  a new request ID executes new work. There is no public `api` or `result` tool.

## Methods

| Method | Arguments | Behavior |
| --- | --- | --- |
| `await app.look(options?)` | `{full?:boolean,screenshot?:boolean}` | Start/reuse the app where supported; observe visible window layering and up to 80 semantic elements in the top surface. First response is a baseline; later views compare with the previous returned tool view. Window metadata is bounded to 32 entries with truncation coverage. `full:true` requests a baseline. |
| `await app.capture(options?)` | `{full?:boolean}` | Look with a PNG of the authorized app's visible stacked surfaces. Establishes the internal coordinate binding; later input returns an updated visual observation while in visual mode. |
| `await app.click(target)` | `{target:string}` OR `{selector:{role?:string,text?:string,scope?:'window'\|'menu'}}` OR `{x:number,y:number,button?:'left'\|'right',count?:1\|2}` | Semantic target ID from the latest look or a unique selector, for controls advertising `press`; otherwise use the returned screenshot fallback; or image-pixel coordinates from the last capture. The string shortcut `app.click('Continue')` is a text selector. Never mix target forms. Mouse options apply only to coordinates. On macOS, coordinates use the visible physical pointer and require foreground delivery; `background-only` refuses them before input. |
| `await app.move(point)` | `{x:number,y:number}` | Move the real pointer to a visible point in the current app image, enabling hover UI. Requires foreground delivery on macOS. |
| `await app.drag(options)` | `{path:[{x:number,y:number},...]}` | One left-button press, bounded path and paired release. 2–64 supplied points with movement, internally at most 256 samples. Currently the entire path must stay within one observed window on one display; no held button survives a call. |
| `await app.typeText(text)` | string, or `{text:string}` | Unicode keyboard input into the active editing control. It may invoke application editing behavior. It does not replace a field value implicitly. |
| `await app.keypress(keys)` | string array, or `{keys:string[]}` | One press/release chord; on macOS this also works for a running app with no window (e.g. New). Foreground app identity is verified before key downs. E.g. `['META','A']` or `['ESC']`. `META` means Command on macOS and Windows key on Windows. |
| `await app.scroll(options)` | `{delta_y:number,delta_x?:number,x?:number,y?:number}` | Wheel steps, positive down/right. Optional x/y anchor uses the current screenshot; without it uses the active window. |
| `await app.quit()` | none | Request normal quit of the active app instance. A save dialog remains observable. No forced termination. |

`look`/`capture` accept no native refs. Input routes within the selected app.
Target IDs belong to the latest observation and should be used promptly. A
changed/expired binding rejects input; DTW never silently rebinds and repeats it.
Use `look` or `capture` to inspect the new state before deciding on new input.

## View

A baseline has `reset:true`, windows in front-to-back order, semantic elements,
and explicit coverage/semantic completeness. Semantic element names/values are
bounded; truncated observations cannot prove absence. Later views use
`{changed:false}` or added/changed/removed windows/elements and changed coverage.
Window order is meaningful. Intermediate reads inside one script do not consume
the baseline before its tool response. A context/process replacement resets it.

Images preserve authorized windows' positions, z-order and alpha, including
same-app overlays and proven associated surfaces. Empty space is transparent and
cannot receive input. Coordinate hit-testing selects the top nontransparent app
surface. Unrelated-app occlusion may still prevent input even though a read can
show the authorized app. If geometry or layer membership changes during capture,
no coordinate binding is published. Read errors/partial semantics remain explicit.

Semantic output is bounded to 80 elements / 24 KiB; window metadata to 32 rows,
with titles bounded to 80 characters. Coverage marks omissions. `changed:false`
compares these observed fields; visual-only changes require capture.
An explicit `look()` uses semantics when available, ending automatic visual
follow-ups and retiring the previous image binding; unavailable semantics still
trigger an image fallback.

The compositor currently requires provider layout metadata for multiple
surfaces (implemented on macOS and Windows), supports at most 16 visible surfaces and a
1600-pixel maximum canvas dimension. Capture errors appear in `view.image.error`;
there is no image block or coordinate binding on that failure. Multi-monitor
space keeps its relative geometry. Offscreen/minimized surfaces are listed where
available but are not invented in the visible canvas.

`move` and `drag` are implemented on macOS. They reject stale geometry, unavailable
images, paths across windows/transparent gaps, and `background-only` delivery
before pointer dispatch. Spanning-window physical input is refused before pointer dispatch: rectangular
window geometry does not prove per-display/Space hit ownership. Capture remains
available. During a drag, a changed/occluded surface or cancellation
stops the sequence and releases its admitted button. Partial effects remain
unknown; never repeat a failed drag automatically. Drag relative motion fields
and absolute positions are both supplied.

After any pointer action, its binding is consumed. The returned tool view supplies
a fresh image while in visual mode. Within one script, capture again before a
second coordinate gesture. Cross-window/app drag, window resizing/moving, held
buttons/keys and clipboard are not public capabilities. Other providers explicitly
reject unsupported gestures; macOS acceptance does not establish platform parity.

## Provider boundaries

Windows records physical window bounds and front-to-back order and renders each
exact HWND for app composition. Covered reads do not relax input visibility or
receiver checks. Layered/transparent HWNDs are explicitly unsupported by this
renderer, and blank/unsupported renders do not acquire a coordinate binding.
macOS capture does not establish Windows GUI or DPI acceptance. Experimental X11
still requires an unobstructed window and does not claim multi-surface capture.
