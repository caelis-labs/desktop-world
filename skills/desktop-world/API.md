# DTW app script reference

## Tools

`apps({query?, state?, offset?, limit?})`

- `query`: case-insensitive name/identifier substring, up to 256 characters.
- `state`: `all` (default), `running` or `stopped`.
- `offset`: nonnegative integer; `limit`: 1–100 (default 30).
- Returns `{apps:[{app,name,running,instances?}], next, coverage, provider}`. Pass `next`
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
- `provider` describes the native provider, capabilities, observe scopes and conditions
  before input; a public method's presence alone does not prove platform support.

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
  preserves dispatch/effect uncertainty; it does not prove the task succeeded.
  Selector focus preparation has a separate `effects.focus_preparation` summary
  and is not counted again as target dispatch.
  A returned value identical to the final script snapshot is coalesced into `view`, rather
  than repeated in `result`. Task-specific JSON values remain in `result`.
  The last current explicit/fallback PNG is a separate MCP image block.
- Output is bounded. `print` has 200 lines / 32 KiB aggregate, 2000 characters per
  line, and explicit truncation. Return JSON has a separate 64 KiB budget.
- A retransmitted identical MCP request ID refers to its retained execution;
  a new request ID executes new work. There is no public `api` or `result` tool.

## Methods

| Method | Arguments | Behavior |
| --- | --- | --- |
| `await app.look(options?)` | `{full?:boolean,role?:string,text?:string,scope?:'window'\|'menu'}` | Start/reuse the app where supported (nonactivating by default on macOS); observe visible window layering and up to 80 semantic elements in the top surface. Optional role (1–64 characters) and text (1–256) filter the native traversal, so relevant content beyond the default preview can be reached. `scope:'menu'` reads the selected window's app menu through the native menu scope without activating it; the default is window content. It requires a proven app window and does not expose Dock roots or menu extras. `semantics.query` identifies the filtered scope; completeness and absence apply only within it. JavaScript always receives the full snapshot. Tool output compares with the previous published view. Window metadata is bounded to 32 entries. `full:true` forces a tool-output baseline, not continuation. |
| `await app.capture(options?)` | `{full?:boolean}` | Return a full snapshot, including `windows`, and a PNG of the authorized app's visible stacked surfaces. Establishes the internal coordinate binding; later input returns an updated visual observation while in visual mode. |
| `await app.readText(target)` | observed ID string OR `{target:string,offset?:number,limit?:number}` | Read native textual content of an observed element. Returns `{text,offset,next_offset,total_length,unit:'utf16'}` inside JavaScript; use `return` or `print` for requested tool output. Offset defaults to 0; limit defaults to 4096, from 2–16384 UTF-16 units. Reuse `next_offset` and the same target without a new look to continue one retained text revision. This read does not refresh the scene or request a PNG. A fresh look/input can retire its source; stale reads fail instead of joining revisions. Titles/descriptions are not substituted for document text. Unsupported native text remains explicit. |
| `await app.click(target)` | string OR `{target:string}` OR `{selector:{role?:string,text?:string,scope?:'window'\|'menu'}}` OR `{x:number,y:number,button?:'left'\|'right',count?:1\|2}` | Semantic target ID from the latest look or a unique selector, for controls advertising `press`; otherwise use the returned screenshot fallback; or image-pixel coordinates from the last capture. `app.click('w1.e12')` uses a target ID; other strings such as `app.click('Continue')` are text selectors. Never mix target forms. Mouse options apply only to coordinates. |
| `await app.move(point)` | `{x:number,y:number}` | Hover over a visible point in the current app image, enabling hover UI. |
| `await app.drag(options)` | `{path:[{x:number,y:number},...]}` | Content drag uses one left-button press, bounded path and paired release. 2–64 supplied points with movement, internally at most 256 samples. The entire path must stay within one observed window. A proven standard macOS or Windows title bar moves its window via native position, with the destination fully on one display. Other paths drag content. Unsupported provider/display boundaries are explicit. No held button survives a call. |
| `await app.typeText(text)` | string, or `{text:string}` | Unicode keyboard input into the active editing control. It may invoke application editing behavior. It does not replace a field value implicitly. |
| `await app.keypress(keys)` | chord string, string array, or `{keys:string[]}` | One press/release chord; on macOS this also works for a running app with no window (e.g. New). Native app/window focus is verified before key downs. E.g. `'META+A'`, `'ENTER'`, `['META','A']` or `['ESC']`. Use an array/object for key names containing a literal `+`. `META` means Command on macOS and Windows key on Windows. |
| `await app.scroll(options)` | `{delta_y:number,delta_x?:number,x?:number,y?:number}` | Integer wheel steps in [-100,100] per axis, positive down/right; these are not pixels. Supply both x/y from the current screenshot to target a pane. Without x/y DTW captures the active surface and anchors at its center. |
| `await app.quit()` | none | Request normal quit of the active app instance. A save dialog remains observable. No forced termination. |

`look`/`capture` accept no native refs. Input routes within the selected app.
Known selectors, keyboard input and default scrolling can begin an `exec`
directly. DTW prepares the app/current receiver on demand and returns observation
after input; no initial or per-script `look` is required. Context persists within
the MCP connection. Explicit IDs still require an observed target, and coordinates
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

Keyboard input uses the native keyboard window when available, independently of
visual z-order, within the already selected process instance; a frontmost tooltip or completion surface does not automatically
receive keys. Focus and app identity checks still apply. On macOS startup requests
neither activate nor hide the app. An app may activate itself; input that genuinely
requires foreground still uses the existing controlled borrow and restoration.

## View

Script `look()` and `capture()` always return `{running,windows,elements,coverage,...}`, even when unchanged. `windows` includes bounds and front-to-back order.

The outward tool baseline has `reset:true`, windows in front-to-back order, semantic elements,
and explicit coverage/semantic completeness. Semantic element names/values are
bounded; truncated observations cannot prove absence. Later views use
`{changed:false}` or added/changed/removed windows/elements and changed coverage.
Window order is meaningful. Intermediate reads inside one script do not consume
the baseline before its tool response. A context/process replacement resets it.

While its anchor window stays in place, a previously captured viewport is retained
when overlays disappear, preserving origin and scale. A new canvas adds
`image.coordinate_reset:true`; use the new image before choosing coordinates.
`image.bounds` is the desktop canvas rectangle; width/height relative to those bounds map desktop units
to image pixels. Input always uses image pixels. Old coordinates are not a
semantic target guarantee.

Images preserve authorized windows' positions, z-order and alpha, including
same-app overlays and proven associated surfaces. Empty space is transparent and
cannot receive input. Coordinate hit-testing selects the top nontransparent app
surface. Unrelated-app occlusion may still prevent input even though a read can
show the authorized app. If geometry or layer membership changes during capture,
no coordinate binding is published. A transient capture/layer disappearance gets
one bounded read-only refresh, including during coordinate preflight before input;
input is never replayed. Read errors/partial semantics remain explicit.

DTW prioritizes actionable controls, readable values and named content over empty
structural nodes, with bounded internal continuation. Stable native identities
keep target IDs across consecutive reads. Missing, omitted or duplicate
identities cannot revive a removed target ID; labels and traversal positions never establish identity.
Selectors still require proven uniqueness; incomplete discovery can reject them.
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
unproven surface. `changed:false`
compares these observed fields; visual-only changes require capture.
An explicit `look()` uses semantics when available, ending automatic visual
follow-ups and retiring the previous image binding; unavailable semantics still
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
An unreadable or partial tree does not block screenshot coordinates, hover, drag,
wheel input or keyboard delivery. Semantic actions still need
their own live target proof; a rejected semantic action is never replayed as a
mouse click. Process identity, visible-hit, geometry and input-release checks
continue to apply when no semantic elements are available.

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
