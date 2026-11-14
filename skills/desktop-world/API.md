# DTW app script reference

## Tools

The default `dtw mcp` surface remains `apps` + `exec`. The experimental
`dtw mcp --toolset atomic` exposes `apps` plus one MCP tool per public method;
`--toolset all` exposes both. Atomic calls require `app` beside the method's
object arguments. Tool names use snake case (`read_text`, `set_value`,
`type_text`, `wait_for_new_window`); other names match the table below.
`keypress` uses `{app,keys:[...]}` and numeric `wait` uses `{app,ms:...}`.
Consult `tools/list` for the selected schemas. Each atomic call has the same
execution/cleanup boundary as one `exec`; keep related focus/menu sequences in
`exec` when they must share that boundary. See the source-tree PoC notes at
`poc/mcp-toolset/README.md` for validation and limits.

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
- Windows reads user/common Start Menu links, App Paths and AppsFolder package
  identities, and merges running user applications. Argument-bearing shortcuts,
  unsupported values and native read gaps remain explicit in coverage. Executable
  paths identify Win32 apps; `win:<AUMID>` identifies proven packaged apps.
- Experimental X11 reports running-only discovery with `installed:false`.
- Provider identity is sent once as `serverInfo.description` at MCP initialization.
  The platform sections below describe native support.

`exec({app, source})`

- `app`: the exact authorization identity returned by discovery; macOS also
  accepts an existing absolute `.app` path, and Windows accepts an existing
  absolute executable path. Windows package IDs must come from AppsFolder discovery.
  The authorization identity cannot change within a script.
- `source`: an async JavaScript function body, at most 64 KiB UTF-8. Await calls
  sequentially. No custom session, request ID, timeout or delivery arguments.
- A missing `app` or malformed argument object returns `invalid_arguments` before
  provider connection; a syntactically invalid app identifier returns `invalid_app`; a valid identity
  that cannot be resolved returns `app_unavailable`.
- Normal output is `{app, execution_id, status, view?, result?, output?, effects?}`.
  `status:completed` means execution has settled; inspect MCP `isError` / `error`
  for success. App-operation failures expose their business code in `error.code`
  and retain operation/source details in `failure`. Invalid public arguments use
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
  On Windows, optional focus preparation and return are handled by DTW.
  An action that cannot establish its required receiver is refused before its
  payload. Inspect the current state before deciding how to continue.
  If the app exits, a separately proved final `running:false` view concludes the
  old instance without relaunching it. A later explicit `look()` can launch it.
  When DTW automatically observes the UI after input, `observation.status` is
  `observed`, `partial` (a view with an image error), or `failed`. Its error is
  separate from action dispatch and the script result. If that read fails,
  inspect the UI again with `look()` or `capture()` before choosing another
  action. Do not repeat an action to recover the missing observation. An
  explicit read failure, interruption or unconfirmed cleanup remains an error.
  A readback mismatch or native cleanup failure is retained on the affected
  execution. Later requests can observe fresh state and issue new actions.
  DTW replaces unavailable workers and checks the current target and input
  preconditions for each action. It does not replay earlier input.
  Return a task-specific JSON value to get just `result` alongside applicable
  receipts. Return the final snapshot or omit the return value to publish `view`.
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

| Method | Arguments | Behavior |
| --- | --- | --- |
| `await app.look(options?)` | `{full?:boolean,role?:string,text?:string,scope?:'window'\|'menu',target?:string}` | Start/reuse the app where supported (nonactivating by default on macOS); observe visible window layering and up to 80 semantic elements in the top surface. Optional role (1–64 characters) and text (1–256) filter the native traversal, so relevant content beyond the default preview can be reached. `scope:'menu'` reads the selected window's app menu through the native menu scope without activating it; the default is window content. It requires a proven app window and does not expose Dock roots or menu extras. `target` observes the exact retained subtree of a previously observed ID (for example, File from a menu look); it cannot be combined with `scope`. It does not activate or expand menus. Unsupported/stale subtree reads fail without broadening to the window or a screenshot. `semantics.query` identifies the filtered scope; completeness and absence apply only within it. JavaScript always receives the full snapshot. Tool output compares with the previous published view. Window metadata is bounded to 32 entries. `full:true` forces a tool-output baseline, not continuation. |
| `await app.wait(msOrCondition)` | milliseconds (1–10000) OR `{window?:'stable',title?:string,role?:string,text?:string,timeout_ms?:number}` | A number pauses without reading, launching or focusing. A title (case-insensitive substring) selects a visible window. The matching window is first in `windows`, and `elements` comes from that window. Role/text also requires a matching element. `window:'stable'` requires the same window metadata on consecutive samples at least 150 ms apart. Supply at least one condition. Default timeout is 2000 ms, maximum 10000 ms. Use the expected destination title or control after navigation. |
| `await app.waitForNewWindow(options?)` | `{timeout_ms?:number}` (default 2000, max 10000) | Wait for a new content window or menu surface after the latest dispatched input, even from the preceding exec. This includes `quit` opening a save-confirmation dialog. Returns a full snapshot when that new surface appears; throws `wait_timeout` if it does not, or `input_required` when no dispatched input precedes it. Read-only; never repeats input. Use after New/Open or opening a popup. After Save/Cancel/Close dismisses a surface, use `look()` to verify the resulting view instead. |
| `await app.capture(options?)` | `{full?:boolean}` | Return a full snapshot, including `windows`, and a PNG of the authorized app's visible stacked surfaces. Establishes the internal coordinate binding; later input returns an updated visual observation while in visual mode. |
| `await app.readText(target)` | observed ID string OR `{target:string,offset?:number,limit?:number}` | Read native textual content of an observed element. Returns `{text,offset,next_offset,total_length,unit:'utf16'}` inside JavaScript; use `return` or `print` for requested tool output. Offset defaults to 0; limit defaults to 4096, from 2–16384 UTF-16 units. `next_offset:null` marks the end; stop there. Otherwise reuse `next_offset` and the same target without a new look to continue one retained text revision. This read does not refresh the scene or request a PNG. A fresh look/input can retire its source; stale reads fail instead of joining revisions. Titles/descriptions are not substituted for document text. Unsupported native text remains explicit. |
| `await app.click(target)` | string OR `{target:string}` OR `{selector:{role?:string,text?:string,scope?:'window'\|'menu'}}` OR `{x:number,y:number,unit?:'fraction',button?:'left'\|'right',count?:1\|2}` | Semantic target ID from the latest look or a unique selector, for controls advertising `press`. Editable text controls can advertise this action to focus the control without changing its text. Unsupported controls use screenshot coordinates. `app.click('w1.e12')` uses a target ID; other strings such as `app.click('Continue')` are text selectors. Never mix target forms. Mouse options apply only to coordinates. |
| `await app.move(point)` | `{x:number,y:number,unit?:'fraction'}` | Hover over a visible point in the current app image, enabling hover UI. |
| `await app.drag(options)` | `{path:[{x:number,y:number},...],unit?:'fraction'}` | Content drag uses one left-button press, bounded path and paired release. 2–64 supplied points with movement, internally at most 256 samples. The entire path must stay within one observed window. A proven standard macOS or Windows title bar moves its window via native position, with the destination fully on one display. Other paths drag content. Unsupported provider/display boundaries are explicit. No held button survives a call. |
| `await app.setValue(options)` | `{target:string,value:string}` OR `{selector:{role?:string,text?:string,scope?:'window'\|'menu'},value:string}` | Replace the complete native value of an editable text control advertising `setValue`, including clearing with an empty string. At most 16 KiB UTF-8, no NUL. Uses the existing native setter without clicking or selecting text first. DTW prepares and verifies the intended control internally; an extra `click` or `focus` is not required. It may not emit keyboard/DOM input events; use `typeText` for application typing behavior. `element_value_set` confirms immediate native readback when available. Verify saves and submissions separately. |
| `await app.typeText(text)` | string, or `{text:string,target?:string}` | Unicode keyboard input into the active editing control. On Windows, pass an observed editable field ID as `target` to require that field to hold focus before and during typing. A mismatch stops input and reports the focused control when available. Typing may invoke application editing behavior and does not implicitly replace the field value. |
| `await app.keypress(keys)` | chord string, string array, or `{keys:string[]}` | One press/release chord; on macOS this also works for a running app with no window (e.g. New). Native app/window focus is verified before key downs. E.g. `'META+A'`, `'ENTER'`, `['META','A']` or `['ESC']`. Use an array/object for key names containing a literal `+`. `META` means Command on macOS and Windows key on Windows. |
| `await app.scroll(options)` | `{delta_y:number,delta_x?:number,x?:number,y?:number,unit?:'fraction'}` | Integer wheel steps in [-100,100] per axis, positive down/right; these are not pixels. Supply both x/y from the current screenshot to target a pane. Without x/y DTW captures the active surface and anchors at its center. |
| `await app.focus()` | none | Request focus for the authorized app/current window. Windows skips the request if it cannot safely prepare the window. A supported windowless app can create its first window within that script. Normal input prepares its destination automatically. |
| `await app.quit()` | none | Request normal quit of the active app instance. A save dialog remains observable. No forced termination. |

`look`/`capture` accept no native refs. Input routes within the selected app.
The wait condition budget stops new probes. An in-flight bounded read
can finish within the script deadline. A condition timeout does not retire the connection.
Control values, including sliders and checkboxes, are in `look().elements[].value`.
When Windows confirms keyboard focus on a returned editable element it adds
`focused:true`. This is a fact about the sampled scene.
`readText()` reads native text content, not numeric control values or element labels.

Scene reads and successful condition waits return full JavaScript snapshots.
After a click or keypress, the next scene read or untargeted typing allows up to
1.2 seconds for UI updates, including time already spent in the script. When
the destination is known, use a condition wait. A numeric wait can give slower
UI more time.
Semantic completeness applies to the queried tree. A stable window sample covers
visible window metadata, so prefer a destination title or control for async loads.
For a new surface use `waitForNewWindow()`. A condition wait times out with
`wait_timeout` and sends no input. `readText()` returns
text pages. Input methods (`click`, `move`, `drag`, `scroll`, `setValue`, `typeText`, `keypress`,
`focus`, `quit`) return action receipts with `outcome.dispatch` and
`outcome.evidence`, not snapshots. Inspect the outer tool `view` for the current
UI; call `look()` inside a script when the script itself needs its contents.
For example: `await app.click(id); const view=await app.look(); return view.elements;`.
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

Keep related input in one `exec`, including input following an explicit `focus()`.
On Windows, open a native menu and choose its item in that same `exec`: returning
the original foreground at the script boundary can normally dismiss the menu.
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
Truncated observations cannot prove absence. Intermediate and custom-result
reads leave the published baseline in place.

While its anchor window stays in place, a previously captured viewport is retained
when overlays disappear, preserving origin and scale. A new canvas adds
`image.coordinate_reset:true`; use the new image before choosing coordinates.
Default input coordinates are pixels in the original PNG. When the host scales
the image preview, pass `unit:'fraction'` with `x` and `y` as fractions of the
visible image width and height in [0, 1). Measure inside the image content,
excluding viewer margins. For drag the unit applies to every path point.
Coordinates use the latest captured canvas and must still pass current hit and
ownership checks.

Images preserve authorized windows' positions, z-order and alpha, including
same-app overlays and proven associated surfaces. Empty space is transparent and
cannot receive input. Coordinate hit-testing selects the top nontransparent app
surface. macOS addressed input ignores unrelated-app coverage; same-app layers still block it.
DTW prepares required visibility before pointer input; a point must still belong to the current authorized surface.
Windows input can still be blocked by unrelated-app coverage. If geometry or layer membership changes during capture,
no coordinate binding is published. A transient capture/layer disappearance gets
one bounded read-only refresh, including during coordinate preflight before input;
input is never replayed. Read errors/partial semantics remain explicit.

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
This retention belongs to the tool response and image attachment. The script's
semantic `look()` snapshot still has `image:null`; keep the `capture()` result
when the script needs image geometry.
The next coordinate gesture privately refreshes pixels and verifies its original
canvas geometry and application instance before input, then resumes visual
follow-ups. Without a prior capture, coordinates still require `capture()`;
unavailable semantics still
trigger an image fallback. If any semantic continuation fails, its earlier pages
are discarded and semantics stay explicitly incomplete; a fresh image may still
be used. A failed refresh retires previous element and pixel input bindings;
script settlement does not repeat the failed read. A failed later read also clears
the current execution's pending view/snapshot and PNG. Arguments rejected locally
before a read reaches the host do not cancel a preceding action's automatic readback.

The compositor currently requires provider layout metadata for multiple
surfaces (implemented on macOS and Windows), supports at most 16 visible surfaces and a
1600-pixel maximum canvas dimension. Recoverable capture errors appear in
`view.image.error`; there is no image block or coordinate binding on that failure.
Permission, cancellation, deadline, process/provider identity and invalid-provider
errors remain operation failures, including when they occur during capture. Multi-monitor
space keeps its relative geometry. Offscreen/minimized surfaces are listed where
available but are not invented in the visible canvas.

`move` and `drag` are implemented on macOS and Windows. They reject stale geometry, unavailable
images and paths across windows/transparent gaps before input. DTW selects the
native delivery automatically; scripts do not select or manage input routes.
During a drag, a changed/occluded application surface or cancellation
stops the sequence and releases its admitted button. Partial effects remain
unknown; never repeat a failed drag automatically.

Native pointer bindings remain single-use. Before each coordinate gesture DTW
privately captures fresh app pixels and checks the current receiver, canvas geometry
and process ownership at each supplied point. Known coordinate actions can
run sequentially without explicit recapture. They hit the current top nontransparent
surface within the original point's app process instance: a popup appearing or
disappearing does not alone invalidate a point. Each original layer retains its
process incarnation, so a sibling process cannot acquire that point. Private
refreshes authorize only their current dispatch and never replace the script-visible
image baseline for later points. A changed canvas anchor, image mapping or
process instance returns `stale_scene`; a point with no authorized app pixel returns
`outside_app`. Both refuse before dispatch; inspect the returned image to choose new coordinates. Content changes such as animation, typing and scrolling do not
invalidate a coordinate mapping. A point does not establish semantic identity or
prove that app content is unchanged; inspect new views when the task needs it.
The final tool view supplies a fresh image while in visual mode. Cross-window/app
drag, window resizing, off-canvas drag endpoints, held
buttons/keys and clipboard are not public capabilities. Experimental X11 rejects
these gestures. Each platform requires its own native/GUI evidence; macOS
acceptance does not establish Windows parity.

## Provider boundaries

On macOS, AX tree availability is optional for app capture and coordinate input.
An unreadable or partial tree does not block proven screenshot coordinates, drag
or wheel input. Keyboard input still needs its exact editing destination; unknown
AX/modal state is a refusal, not permission to activate the app. Semantic actions still need
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

Windows records physical DWM-visible bounds and global window order, and renders
exact HWNDs for app composition. Covered reads do not relax input visibility or
receiver checks. Readable global alpha/color-key layered windows are supported;
per-pixel layered alpha without an independent proof and blank/unsupported renders
remain explicit failures and do not acquire a coordinate binding. Native menu
scope stays within its selected window; a popup's keyboard receiver requires
native owner proof. Normal quit requests close on the selected process instance,
leaving save prompts observable. Pointer paths remain within one visible window
and display, with paired release on cancellation/failure. Windows GUI, application
and DPI conditions require their own acceptance. Experimental X11 still requires
an unobstructed window and does not claim multi-surface capture.

Windows prepares the exact foreground receiver for interaction. Semantic actions
use UIA; pointer actions require a visible, unobstructed hit. If the target
remains covered after preparation, uncover it and capture it again. Related
actions in one script reuse its foreground scope. A visible user handoff stops
input for that script. Settlement returns proven original focus and the owned
physical pointer. Windows controls window stacking through its normal activation
behavior. If the original foreground is an unreadable hidden service, the
prepared visible target can remain active.

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
