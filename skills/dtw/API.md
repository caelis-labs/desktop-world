# DTW app API reference

[Tools](#tools) · [Methods](#methods) · [Locators](#locators) ·
[Views and deltas](#view) · [Screenshot coordinates](#screenshot-coordinates) ·
[Provider boundaries](#provider-boundaries)

## Tools

`dtw mcp` exposes `apps` and `exec`. `exec` runs the methods below in an
app-scoped JavaScript execution.

### Discover an app

`apps({query?, state?, offset?, limit?})` accepts only these optional fields.

- `query`: case-insensitive name/identifier substring, up to 256 characters.
- `state`: `all` (default), `running` or `stopped`.
- `offset`: nonnegative integer (default 0); `limit`: 1–100 (default 30).
- Returns `{apps:[{app,name,running,instances?}], next, coverage}`. Pass `next`
  as the next offset with the same query/state. Coverage describes searched
  locations; an empty result does not prove the app is not installed elsewhere.
- macOS searches standard application folders, including stopped bundles, and
  running user bundles elsewhere. Nested helpers and background-only bundles
  are excluded. Duplicate installed bundle identifiers use explicit bundle paths.
- Windows reads user/common Start Menu links, App Paths and AppsFolder package
  identities, and merges running user applications. Argument-bearing shortcuts,
  unsupported values and native read gaps remain explicit in coverage. Executable
  paths identify Win32 apps; `win:<AUMID>` identifies proven packaged apps.
- Linux X11 reports running windows and eligible top-level XDG desktop entries.
  Coverage is partial and `installed:false` indicates running-only results.
  Use the returned canonical executable path. `look()` can start a stopped
  entry if it declares a single unquoted, absolute ELF executable with no
  arguments or field codes. Terminal and D-Bus activated entries are excluded.
- Provider identity is sent once as `serverInfo.description` at MCP initialization.
  The platform sections below describe native support.

### Run app JavaScript

`exec({app,source})` runs one async JavaScript function body for the chosen app.

- `app`: the exact authorization identity returned by discovery (1–4096 UTF-8
  bytes, without surrounding whitespace, control characters or dot path components). macOS also
  accepts an existing absolute `.app` path, and Windows accepts an existing
  absolute executable path. Windows package IDs must come from AppsFolder discovery.
  On Linux use the canonical executable path from `apps` for a running app or
  an eligible installed desktop entry. The app identity is fixed for the script.
- `source`: a nonempty async JavaScript function body, at most 64 KiB UTF-8.
  The MCP arguments contain exactly `app` and `source`. Await operations sequentially.
- A missing `app` or malformed argument object returns `invalid_arguments` before
  provider connection. A syntactically invalid identity returns `invalid_app`.
  An unresolved identity can return `app_unavailable` or, on Linux, `app_not_found`.
- Normal output is `{app, execution_id, status, view?, result?, output?, effects?}`.
  `status:completed` means execution has settled; inspect MCP `isError` / `error`
  for success. App-operation failures expose their business code in `error.code`
  and retain operation/source details in `failure`. When the failing call has a
  recorded receipt, `failure.dispatch` describes **that call**, not an earlier
  success in the batch. `failure.diagnostics` can include bounded selector or
  preparation evidence. Absence of a dispatch field is not proof that nothing
  happened. Inspect the returned view before deciding on further input.
  Invalid public arguments use
  `invalid_arguments`. `effects`
  separates input dispatch from any known UI effect. `effects.dispatch` counts
  `dispatched`, `not_dispatched` and `unknown`. Optional
  `effects.acknowledgements` identify the call, operation and confirmed
  postcondition or specific readback error. An omitted acknowledgement gives no
  proof of app consumption.
  DTW handles input readiness internally. Verify the requested effect from the
  returned view or an explicit readback; dispatch alone is not effect confirmation.
  If automatic input preparation fails before payload delivery, the requested
  input is `not_dispatched`; preparation may still have changed focus. Its error
  identifies that stage. Cleanup failures remain explicit errors.
  Windows defaults to foreground interaction; DTW prepares the exact receiver.
  macOS prefers effective background capabilities and directly uses foreground
  or the real pointer when needed. Changes remain in place after settlement.
  Public `effects` reports dispatch and optional interruptions/acknowledgements.
  Delivery `route` / `reason` stay in the internal diagnostic ledger; the public
  response has no `effects.delivery` field.
  An action that cannot establish its required receiver is refused before its
  payload. Inspect the current state before deciding how to continue.
  If the app exits, a separately proved final `running:false` view concludes the
  old instance without relaunching it. A later explicit `look()` can launch it.
  When DTW automatically observes the UI after input, `observation.status` is
  `observed`, `partial` (a view with masked surfaces or an image error), or
  `failed`. Its error is
  separate from action dispatch and the script result. If that read fails,
  inspect the UI again with `look()` or `capture()` before choosing another
  action. Do not repeat an action to recover the missing observation. An
  explicit read failure, interruption or unconfirmed cleanup remains an error.
  A readback mismatch or native cleanup failure is retained on the affected
  execution. Cleanup failure stops that execution and affected native context.
  Later new requests let the service retire/recover the context and check
  admission before observing fresh state or issuing new actions.
  DTW replaces unavailable workers and checks the current target and input
  preconditions for each action. It does not replay earlier input.
  Return task-specific JSON to publish `result` alongside applicable receipts.
  Omit the return value or return the final scene snapshot to publish `view`.
  Errors retain any available view. A PNG is a separate MCP image block with
  geometry in `view.image` or top-level `image` for a custom result.
- An incomplete default semantic view can include `semantics.hint` suggesting a
  `role` or `text` filter. This preserves the original read reason and performs
  no extra observation or input; use freshly returned target IDs.
- Output is bounded. `print` has 200 lines / 32 KiB aggregate, 2000 characters per
  line, and explicit truncation. Return JSON has a separate 64 KiB budget.
- A retransmitted identical MCP request ID refers to its retained execution;
  a new request ID executes new work. There is no public `api` or `result` tool.

## Methods

The arguments column defines the canonical object fields. JavaScript also accepts
the shorthand forms noted below.

| Method | Arguments | Behavior |
| --- | --- | --- |
| `await app.look(options?)` | `{full?:boolean,role?:string,text?:string,scope?:'window'\|'menu',target?:string}` | Start/reuse the app where supported (nonactivating by default on macOS); observe visible window layering and up to 80 semantic elements in the top surface. Optional role (1–64 characters) and text (1–256) filter the native traversal, so relevant content beyond the default preview can be reached. `scope:'menu'` reads the selected window's app menu through the native menu scope without activating it; the default is window content. It requires a proven app window and does not expose Dock roots or menu extras. `target` observes the exact retained subtree of a previously observed ID (for example, File from a menu look); it cannot be combined with `scope`. Linux supports window scope alone, without subtree queries. It does not activate or expand menus. Unsupported/stale subtree reads fail without broadening to the window or a screenshot. `semantics.query` identifies the filtered scope; completeness and absence apply only within it. JavaScript always receives the full snapshot. Tool output compares with the previous published view. Window metadata is bounded to 32 entries. `full:true` forces a tool-output baseline, not continuation. |
| `await app.wait(options)` | `{ms:number}` (1–10000) OR `{window?:'stable',title?:string,role?:string,text?:string,timeout_ms?:number}` | `{ms}` pauses without reading, launching or focusing; do not combine it with condition fields or `timeout_ms`. A numeric JS argument is supported shorthand. A title (case-insensitive substring) selects a visible window. The matching window is first in `windows`, and `elements` comes from that window. Role/text also requires a matching element. `window:'stable'` requires the same window metadata on consecutive samples at least 150 ms apart. Supply at least one condition. Default timeout is 2000 ms, maximum 10000 ms. Use the expected destination title or control after navigation. |
| `await app.waitForNewWindow(options?)` | `{timeout_ms?:number}` (default 2000, max 10000) | Wait for a newly observed content window or menu surface of the same app instance, relative to a baseline taken before the last dispatched or possibly dispatched input, even from the preceding exec. A new surface does not prove the input caused it. This includes `quit` opening a save-confirmation dialog. Returns a full snapshot when that new surface appears; throws `wait_timeout` if it does not. A timeout retains that baseline for another wait without repeating input; success consumes it. `input_required` means there is no usable input baseline. Waiting never resends the triggering action or upgrades an unknown receipt to success. Use after New/Open or opening a popup. After Save/Cancel/Close dismisses a surface, use `look()` to verify the resulting view instead. |
| `await app.capture(options?)` | `{full?:boolean,semantics?:boolean,region?:{x:number,y:number,width:number,height:number}}` | Return a PNG and its coordinate metadata. Windows captures the foreground window's on-screen region. Use `look()` for native elements, or opt into both with `semantics:true`. Windows chooses a size based on local image detail. Use `region` to inspect a small area. Pointer coordinates are relative to the resulting PNG. |
| `await app.readText(options)` | `{target:string,offset?:number,limit?:number}` (JS also accepts an observed ID string) | Read native textual content of an observed element. Returns `{text,offset,next_offset,total_length,unit:'utf16'}` inside JavaScript; use `return` or `print` for requested tool output. Offset is an integer from 0 to 4294967295 (default 0). Limit is an integer from 2 to 16384 UTF-16 units (default 4096). `next_offset:null` marks the end; stop there. Otherwise reuse `next_offset` and the same target without a new look to continue one retained text revision. This read does not refresh the scene or request a PNG. A fresh look/input can retire its source; stale reads fail instead of joining revisions. Titles/descriptions are not substituted for document text. Unsupported native text remains explicit. |
| `app.getByRole(role, options?)` | Native role string and `{name?:string,exact?:boolean}` | Builds a locator without starting or reading the app. Resolve it with `await locator.click()`, `await locator.setValue(value)` or `await locator.waitFor({state?,value?,timeout?})`. See below. |
| `await app.invokeMenu(options)` | `{path:string[]}` | macOS native menu command, e.g. `{path:['File','Export','PDF']}`. Provide 2–16 exact path segments (each 1–256 characters), not a global text search. Only paths the native menu exposes and whose document destination is proved can be invoked; dynamically hidden branches can be refused. A command's dispatch does not prove that it saved or exported a file. |
| `await app.showMenu(options)` | `{target:string}` | macOS: request the context menu of the observed native element if it advertises `showMenu`. Not a coordinate right-click. Observe the resulting menu before selecting a command. |
| `await app.selectOption(options)` | `{target:string,option:string}` | macOS: select the exact named option of an observed native popup control advertising `selectOption`, in its focused document window. Not a setter for arbitrary text or a DOM select API. |
| `await app.click(options)` | `{target:string}` OR `{selector:{role?:string,text?:string,scope?:'window'\|'menu'}}` OR `{x:number,y:number,unit?:'fraction',button?:'left'\|'right'\|'middle',count?:1\|2,modifiers?:string[]}` | Semantic target ID from the latest look or a unique selector, for controls advertising `press`. Editable text controls can advertise this action to focus the control without changing its text. Unsupported controls use screenshot coordinates. Use `app.click({target:'w1.e12'})` for an ID or `{selector:{text:'Continue'}}` for text. JS string shorthand accepts IDs or text selectors. Never mix target forms. Mouse options apply only to coordinates. Middle clicks require macOS. A single middle click can be delivered directly to a verified ordinary AppKit window without moving the real pointer or activating the app. Otherwise the visible-point route requires an already-foreground app and may move the real pointer. `modifiers: []` is equivalent to omitting the field. A nonempty array of unique `cmd`, `shift`, `option`, or `ctrl` values returns `unsupported_capability` before input. macOS can retain modifier flags in shared session state after a modified mouse event. |
| `await app.move(point)` | `{x:number,y:number,unit?:'fraction'}` | Hover over a visible point in the current app image, enabling hover UI. |
| `await app.drag(options)` | `{path:[{x:number,y:number},...],unit?:'fraction'}` | Content drag uses one left-button press, bounded path and paired release. 2–64 supplied points with movement, internally at most 256 samples. The entire path must stay within one observed window. A proven standard macOS or Windows title bar moves its window via native position, with the destination fully on one display. Other paths drag content. Unsupported provider/display boundaries are explicit. No held button survives a call. |
| `await app.setValue(options)` | `{target:string,value:string}` OR `{selector:{role?:string,text?:string,scope?:'window'\|'menu'},value:string}` | Replace the complete native value of an editable text control advertising `setValue`, including clearing with an empty string. At most 16 KiB UTF-8 and 48 KiB after JSON encoding, with no NUL. Uses the existing native setter without clicking or selecting text first. DTW prepares and verifies the intended control internally; an extra `click` or `focus` is not required. It may not emit keyboard/DOM input events; use `typeText` for application typing behavior. `element_value_set` confirms immediate native readback when available. Verify saves and submissions separately. |
| `await app.typeText(options)` | `{text:string,target?:string}` (JS also accepts a string) | Send text into the active editing control. Text must be nonempty, at most 16 KiB UTF-8 and 48 KiB after JSON encoding, with no NUL. macOS and Windows support Unicode keyboard input. Linux can type only characters mapped by the active group-zero keymap. Windows alone accepts `target` to require an observed editable field to remain focused. Typing may invoke application editing behavior. Use `setValue` for complete native replacement where advertised. |
| `await app.keypress(options)` | `{keys:string[]}` (JS also accepts a chord string or string array) | One press/release chord with 1–8 key names, at most 32 printable ASCII characters per name; on macOS this also works for a running app with no window (e.g. New). Native app/window focus is verified before key downs. E.g. `{keys:['META','A']}` or `{keys:['ESC']}`. JS shorthand also accepts `'META+A'` and arrays. Use an array/object for key names containing a literal `+`. `META` maps to Command on macOS, the Windows key on Windows and Super on X11. Choose the app’s shortcut for that OS. |
| `await app.scroll(options)` | `{delta_y:number,delta_x?:number,target?:string,x?:number,y?:number,unit?:'fraction'}` | Integer wheel steps in [-100,100] per axis, positive down/right. At least one delta must be nonzero; `delta_y` is required. Choose one anchor: an observed element ID (macOS/Windows), both x/y from the current screenshot, or neither for the active surface's center. An element anchors a wheel event at its verified geometry; this does not mean scroll-into-view or guarantee a changed offset. Do not mix `target` with x/y or `unit`. |
| `await app.focus(options?)` | `{}` | Request focus for the authorized app/current window. Windows skips the request if it cannot safely prepare the window. A supported windowless app can create its first window within that script. Normal input prepares its destination automatically. |
| `await app.quit(options?)` | `{}` | On macOS and Windows, request normal quit of the active app instance. A save dialog remains observable. Linux returns `unsupported_capability`. |

### Locators

A locator stores a query, not an observed element ID. Each operation resolves it
in the selected app instance. Use DTW native roles such as `button` or `text_field`.
`name` matches the native name, not its editable value or the broader `look({text})`
search. The default is a case-insensitive substring; `exact:true` compares the
whole name case-sensitively. This is not an ARIA name computation. Multiple
matches do not select the first one. Use an observed ID instead when you need
continuity with the same object rather than a newly resolved match.

`locator.waitFor` accepts `state:'attached'|'detached'|'enabled'|'disabled'`
and/or an exact string `value`; a value alone implies `attached`. Do not combine
`detached` with `value`. `timeout` is in milliseconds (default 10000, range 0–30000); `0` samples once.
Success returns `{state,element?}` without a native handle. A complete sample that
does not satisfy the predicate by the deadline yields `wait_timeout`; missing proof
can yield `selector_unproven` instead. A stopped app is not launched by this wait. Incomplete enumeration
cannot prove absence or uniqueness; an unreadable value or unknown enablement
does not satisfy the corresponding predicate. This wait issues no input. It does
not implement DOM visibility, stability or a generic selected-state predicate.
`setValue` is the same native setter as `app.setValue`, not Playwright `fill`.

### Observations and receipts

`look` and `capture` route within the selected app.
The wait condition budget stops new probes. An in-flight bounded read
can finish within the script deadline. A condition timeout does not retire the connection.
Control values, including sliders and checkboxes, are in `look().elements[].value`.
When Windows confirms keyboard focus on a returned editable element it adds
`focused:true`. This is a fact about the sampled scene.
`readText()` reads native text content, not numeric control values or element labels.

Scene reads and successful condition waits return full snapshots to the method
caller; a duration wait returns `null`.
After a click or keypress, the next scene read or untargeted typing allows up to
1.2 seconds for UI updates, including time already spent in the script. When
the destination is known, use a condition wait. A numeric wait can give slower
UI more time.
Semantic completeness applies to the queried tree. A stable window sample covers
visible window metadata, so prefer a destination title or control for async loads.
For a new surface use `waitForNewWindow()`. A condition wait times out with
`wait_timeout` and sends no input. `readText()` returns
text pages. Input methods (`click`, `move`, `drag`, `scroll`, `setValue`, `typeText`, `keypress`,
`focus`, `quit`, `invokeMenu`, `showMenu`, `selectOption`) return action receipts with `outcome.dispatch` and
`outcome.evidence`, not snapshots. Inspect the outer tool `view` for the current
UI; call `look()` inside a script when the script itself needs its contents.
For example: `await app.click({target:id}); const view=await app.look({}); return view.elements;`.
`dispatched` means the action was sent; `evidence.kind:'unverified'` means its UI
effect has not been established. Dispatch itself is not unknown in this case.
Known selectors, keyboard input and default scrolling can begin an `exec`
directly. DTW prepares the app/current receiver on demand and returns observation
after input; no initial or per-script `look` is required. Context persists within
the MCP connection. Elements with a stable native identity keep their IDs across
filtered views of the same window. Windows controls with a proven native window
identity can reuse an ID after a different filter. Other Windows controls may
require a current observation before input. Coordinates
require a returned capture; new unknown controls require inspection. Known actions
can run sequentially without an explicit observation after each step. DTW renews
single-use native bindings before the next requested action.
Renewing a semantic target requires a provider-retained object credential plus
the same role, name, enabled state and actions. View comparison IDs alone do not
prove object continuity: Windows UIA runtime IDs can be reused, so a consumed
Windows target requires a fresh observation. Its exact retained original binding
can still be used once. Labels and positions never retarget an ID. A missing or changed target requires inspection
of the returned view before choosing new input. Dispatched/unknown input is never
replayed to renew a binding.

Input uses the current authorized app/window and prepares its destination
internally. Use an observed editable control before typing. After an action that
creates a window or popup, use `waitForNewWindow()` before acting in it; after
Save/Cancel/Close dismisses a surface, inspect `look()` instead.

Keep known related input in one `exec`. Verify the active menu and receiver before
new input.
If the user changes the selected app/window during execution, DTW stops further
input. Inspect the new state in a later execution; never replay an unknown input.
Confirmed quit or closure of the current window ends its input scope. A save
prompt or unconfirmed close is not proof of termination.

## View

Script `look()` and `capture()` always return `{running,windows,elements,coverage,...}`, even when unchanged. `windows` includes bounds and front-to-back order.

The tool's `view.kind` is `full`, `delta` or `unchanged`. Full views replace
the baseline and carry arrays. A delta applies to the last published view for
the app and carries per-collection `added`, `changed`, `removed` and optional
`order`. Missing fields retain their previous values. DTW sends a full view if
it costs less than a delta. Use `look({full:true})` when a fresh baseline is needed.
Truncated observations cannot prove absence. Even `complete` with `elements:[]`
only describes the queried AX/native tree, not all visible content. Capture
self-drawn or low-AX interfaces before concluding the screen is empty.
Intermediate and custom-result
reads leave the published baseline in place.

### Screenshot coordinates

Use the PNG attached to the current response to choose an unknown point. The
script receives image metadata, and the model sees the PNG after the call
returns. While its anchor stays in place, the canvas retains its viewport as
overlays disappear. `image.coordinate_reset:true` means to take coordinates
from the newly returned image.
Default input coordinates are pixels in the returned PNG. When the host scales
the image preview, pass `unit:'fraction'` with `x` and `y` as fractions of the
visible image width and height in [0, 1). Measure inside the image content,
excluding viewer margins. For drag the unit applies to every path point.
Coordinates use the latest captured canvas and must still pass current hit and
ownership checks.

`capture()` defaults to pixels. Its script snapshot includes `windows`,
image geometry, `elements:[]` and `semantics.reason:"skipped"`. Use
`capture({semantics:true})` to include native elements. Windows sizes each
PNG automatically, including during visual readback. DTW maps input coordinates
from the returned image to the desktop. Visual readback retains the semantic
preference. A window `look()` can fall
back to a PNG when native semantics are unavailable or the unfiltered tree
has no usable elements. The fallback PNG can also ground coordinates.

For small or densely packed controls, use `capture({region:{x:0.6,y:0.75,width:0.4,height:0.25}})`
to inspect the bottom-right part of the full app canvas. The origin is the
top-left of the app image. The four fractions describe left offset, top offset,
width and height. Both edges must fit in the canvas. Windows crops first and
can resize the result. Read `image.width` and `image.height` for its pixel dimensions.
For a crop, `image.bounds` describes the crop's desktop geometry, `image.canvas_bounds`
describes the full canvas, and `image.region` echoes the requested fractions.
All pointer methods accept crop-local pixels or `unit:'fraction'` measured within
the crop. Automatic visual readback retains the crop while the full canvas
geometry stays stable and shows the full canvas if that geometry changes.
`capture()` returns to the full canvas. Always inspect the new image when
`image.coordinate_reset:true` is present.

On macOS and Windows, composed images preserve window positions, z-order and alpha, including
same-app overlays and proven associated surfaces. Empty space is transparent and
cannot receive input. Coordinate hit-testing selects the top nontransparent app
surface. macOS addressed input ignores unrelated-app coverage; same-app layers still block it.
DTW prepares required visibility before pointer input; a point must still belong to the current authorized surface.
Windows input can be blocked by unrelated-app coverage. Linux X11 uses a single
unobstructed client window. If geometry or layer membership changes during capture,
no coordinate binding is published. A transient capture/layer disappearance gets
one bounded read-only refresh, including during coordinate preflight before input;
input is never replayed. Read errors/partial semantics remain explicit.

### Native elements and text

DTW prioritizes actionable controls, readable values and named content over empty
structural nodes, with bounded internal continuation. Stable native identities
keep target IDs across consecutive reads. A bounded cache keeps exact native object handles usable across filtered reads. Before input or a historical subtree read, DTW obtains fresh native evidence for the same app, window, object and relevant state; cached IDs alone never authorize input. Removed/replaced objects or changed role/name/actions/enabled state fail before payload, without a label-based fallback. Weak identities require continuous observation. Text pagination stays on its original read-only content observation and may expire; labels and traversal positions never establish identity. Changing the observation query publishes a new compact baseline instead of a large removed-ID list.
Selectors still require proven uniqueness; incomplete discovery can reject them.
On macOS, text filtering searches native fields beyond their printed previews with
a shared search budget. An unsearched suffix leaves `text_limit` coverage incomplete;
it cannot prove absence or uniqueness. Narrow a query by role when appropriate.
Names/values longer than 240 Unicode characters carry `<field>_truncated:true`,
`<field>_length` (Unicode character count) and `<field>_revision` (SHA-256 of the
provider's full observed field). Revisions signal omitted-content changes, not
object identity or full text readback. These fields appear only when needed;
`semantics.text_truncated` marks the incomplete preview. Read the actual native
text with `readText` to verify omitted content. The provider itself can have
bounded read coverage, so a field revision does not establish document completeness.

Semantic output is bounded to 80 elements / 24 KiB; window metadata to 32 rows,
with titles bounded to 80 characters. Coverage marks omissions. Optional native
read gaps include bounded `coverage.read_error_codes`; a gap never authorizes an
unproven surface. `kind:"unchanged"`
compares these observed fields; visual-only changes require capture.
An explicit `look()` uses semantics when available, ending automatic visual
follow-ups while retaining a previously returned image's coordinate mapping.
Within one script, a successful `look()` also preserves an earlier requested PNG
when the window layout is unchanged; `view.image.captured_at_call` identifies its
capture call. Input, changed surfaces and failed reads discard that pending PNG.
Keep the `capture()` result when script logic needs its image geometry.
A coordinate gesture resumes visual follow-ups after validating current pixels.
If any semantic continuation fails, its earlier pages
are discarded and semantics stay explicitly incomplete; a fresh image may still
be used. A failed refresh retires previous element and pixel input bindings;
script settlement does not repeat the failed read. A failed later read also clears
the current execution's pending view/snapshot and PNG. Arguments rejected locally
before a read reaches the host do not cancel a preceding action's automatic readback.

### Pixel availability and pointer preflight

The longest image edge is at most 1600 pixels. Windows crops first and chooses
the smallest candidate that retains local high-contrast detail and spacing up
to what the largest candidate can show. Each side of the source window is
bounded to 8192 pixels and the total to 16,777,216 pixels. The worker encodes
the selected image once. Recoverable capture errors
appear in `view.image.error` and provide no image or coordinate binding.
Windows captures the foreground window's region of the composed desktop,
including visible menus and tooltips within those bounds. Regions covered by
other processes and areas outside all displays appear transparent.
On macOS, app composition requires provider layout metadata and supports up to
16 visible surfaces.
Permission, cancellation, deadline, process/provider identity and invalid-provider
errors remain operation failures, including when they occur during capture.
Multi-monitor space keeps its relative geometry.
Fully offscreen and minimized surfaces are listed where available without pixels.

`move` and `drag` are implemented on macOS, Windows and native Linux X11. They reject stale geometry, unavailable
images and paths across windows/transparent gaps before input. DTW selects the
native delivery automatically; scripts do not select or manage input routes.
During a drag, a changed/occluded application surface or cancellation
stops the sequence and releases its admitted button. Partial effects remain
unknown; never repeat a failed drag automatically.

Native pointer bindings remain single-use. Windows renews the binding from the
last returned image's physical bounds and uses a native hit test at each requested
point. Other providers privately refresh the app pixels that could receive the
gesture. Known coordinate actions can run sequentially without explicit recapture.
Private refreshes authorize one dispatch and preserve the last image's mapping.
A change away from the requested point can leave it usable. Moved receiver
geometry can return `stale_scene`, and a point with no authorized app pixel
returns `outside_app`. A coordinate addresses a location. Use a semantic target
when the action needs object identity.
The final tool view supplies a fresh image while in visual mode. Cross-window/app
drag, window resizing, off-canvas drag endpoints, held
buttons/keys and clipboard are not public capabilities. On X11, drag covers client
content only.

## Provider boundaries

On macOS, editable semantic clicks confirm the exact retained native text receiver
before sequential script input continues. Web text controls can require direct
foreground preparation before dispatch. Receiver readback failure preserves a
known dispatch together with its error; inspect before new input. Compatibility
still requires evidence for each application/control.

On macOS, AX tree availability is optional for app capture and coordinate input.
An unreadable or partial tree does not block proven screenshot coordinates, drag
or wheel input. Keyboard input still needs its exact editing destination; unknown
AX/modal state does not prove that destination or authorize input. Semantic actions still need
their own live target proof; a rejected semantic action is never replayed as a
mouse click. Process identity, visible-hit, geometry and input-release checks
continue to apply when no semantic elements are available.

macOS input readiness is handled internally, including app-menu commands. A menu
action needs a live enabled target and a proven destination in the authorized
app. Reading menus does not itself prepare input. Models need no delivery-mode
arguments or separate focus sequence for ordinary actions.

Native AX actions use the remaining caller deadline for one provider reply.
After dispatch, a timeout or AX error means unknown effect, never success or
permission to retry. A positive AX acknowledgement is dispatch evidence, not
proof of the UI effect; inspect the returned view before choosing a new action.

Windows records physical DWM-visible bounds and reads the composed screen
inside the selected foreground window. Explicit capture can prepare focus
when needed. Automatic readback uses the current foreground. An owned popup
within those bounds appears in the same image as its owner. An active dialog
can become the capture window and change the image bounds. Visible pixels can
contain the desktop background through translucent windows. Native menu scope
stays within its selected window. A popup's keyboard receiver requires
native owner proof. Normal quit requests close on the selected process instance,
leaving save prompts observable. Pointer paths remain within one visible window
and display, with paired release on cancellation/failure. Windows GUI, application
and DPI conditions require their own acceptance.

Windows prepares the exact foreground receiver for interaction. Semantic actions
use UIA. Screen coordinates require the captured window to remain foreground
with the same bounds, DPI, and backing dimensions. DTW maps each PNG pixel to
the physical point sampled to produce it and verifies the actual receiver, including
the receiver again after pointer placement. Activity elsewhere on screen
does not invalidate the point. Related actions in one script share its checked
input scope. A visible user handoff stops
input for that execution. Windows defaults to foreground input and uses normal
activation behavior; it does not use background-directed mouse delivery.
Settlement releases held keys/buttons and leaves foreground, focus, stacking
and physical pointer changes in place. No original-state restoration is attempted.

On Windows, a readable focused edit can confirm `text_present_in_focused_control`
for ASCII and Unicode. A failure to verify accepted text stops the script at
`typeText` with `text_delivery_unconfirmed` when exact readback differs or does
not settle. Inspect the field before any further input. Some controls support
typing without native text readback, so use the next relevant UI state to check
the effect. Confirmation describes the edit at readback time.

For native Unicode Win32/WinForms Edit controls, Windows `readText` reads the
exact UIA-advertised child HWND with bounded read-only system messages. This
avoids truncated UIA proxy text being reported as a complete document. Ownership,
root, native handle, password state, length and retained revision are checked;
the read does not focus the editor. Other controls use native UIA text/value
patterns. A short provider preview is not proof of full document content.
Native multiline Win32/WinForms Edit setters use CRLF line breaks so their visible
document agrees with the logical text; other UIA providers keep their own values.

Linux requires a local native X11 server with an EWMH window manager, XRes 1.2,
XInput 2 and XTest. Wayland and XWayland return `wayland_unsupported`.

`look` reads window metadata and, where available, the AT-SPI tree. The AT-SPI
application must match the XRes owner by D-Bus PID and expose one visible window
matching the managed X11 window by title and bounds. Linux supports window scope
only. Subtree queries (`look({target})`) and menu scope are unavailable. Text
filters search accessible names and labels. `readText` reads bounded
plain AT-SPI content using UTF-16 pages. Password controls cannot use `readText`
or `setValue`. `setValue` uses the native editable-text interface and confirms
the value when readback matches. Native `click` invokes a proven, enabled press
action. Each new semantic observation issues fresh element IDs, so select from
the latest observation. Multiple windows, missing AT-SPI or an unproven match
leave semantics unavailable. Continue with a capture if pixels can be proven.

`capture` requires unobstructed client pixels. X11 provides foreground focus,
coordinate click, hover, same-window content drag, wheel and mapped keyboard
input. XTest uses the shared pointer and keyboard and requires idle input before
dispatch. An active group switch or text-altering modifier can block typing.
`look` can start an eligible desktop entry and confirms the exact spawned
process through its XRes-owned window.
Normal application quit is unavailable. Verify input effects using the returned
view or independent app state.
