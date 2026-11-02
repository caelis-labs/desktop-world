---
name: desktop-world
description: DTW scripting and capability reference for app-scoped desktop computer use through standard MCP.
---

# Desktop World

For file choosers, forms, dynamic editing controls, and large semantic trees, read the relevant section of [Common interaction patterns](PATTERNS.md). It describes reusable interactions; obtain application-specific targets from current observations.

## MCP and script contract

The configured DTW server advertises `apps`, `api`, and `exec`; clients may prefix these tool names. `apps` returns stable platform app IDs, advertised capabilities and macOS permission preflight status. `permission_required` identifies missing OS access; grant the host app named by macOS, refresh state, and retry only an operation known not to have dispatched. Screen Recording changes may require restarting that host and reconnecting. A host name inferred by DTW is a hint, not the system's attribution decision. `api` returns the exact schema and semantics of selected methods.

Call `exec` with **`{app, source}`** for desktop work. The app is its stable platform ID; macOS also accepts an existing absolute `.app` path for an installed target. Source is an async JavaScript function body with `app`, `session`, `state`, `print`, `await` and `return`. Omit `app` for provider-free Session work, including receipt recovery after app exit or failed native cleanup. That lane cannot call app methods. Serialize the source string once through ordinary JSON. There is no request ID, source file path, delivery or approval argument.

One desktop script addresses one app. Each selected app and the app-less lane retain separate JSON `session.state`; `state` is its alias. Committed JSON survives app quit/relaunch and native context rebuild until the MCP connection ends; native refs do not. Ordinary variables, functions and bound Window objects do not survive VM recreation. Await methods sequentially. State commits only when the script succeeds and never rolls back desktop effects. One desktop script and one provider-free script may run alongside each other; each lane is otherwise serial.

Keep the selected window ref in `state` and group known steps in one script: action, `wait` for an explicit next condition, then readback/capture and the remaining known steps. Prefer selector-based `press`/`setValue` for known controls and `waitForWindow` for modal transitions. Use `observe` or `capture` when a new target, ambiguity, or visual decision needs inspection. A separate `exec` for every native method adds unnecessary round trips. Request `api` only for the methods whose exact schema you need.

Scripts have a 30-second deadline and no native-call quota. Imports, filesystem, network, timers and process APIs are unavailable. Use `print(...)` (the bounded `console.log` alias) for small output, or return a small JSON result and the images needed for the next decision. Print and return have separate budgets; emitting the same data twice increases output. Print is limited to 200 lines / 32 KiB, 2000 characters per line, with explicit truncation. Omit undefined object fields or use null.

## App lifecycle and bound windows

On macOS, `await app.launch()` resolves the installed target and starts it once, or reuses a verified running instance. It requests no foreground activation. `application_running` proves process identity, not UI readiness: then use `await app.window({title:'Observed title'})` to wait briefly for one uniquely matching window. Omit the query only for a complete single-window inventory. Titles filter observations; refs retain the exact identity. Ambiguity exposes candidate refs, never selects the first.

A bound window supports `await w.press({selector:{...}})`, `w.observe(...)`, `w.wait(...)`, `w.capture()`, and the other window methods with `window_ref` supplied internally. Use `w.close()` to request normal closure of that exact window. Store only `w.window_ref` in JSON state and rebind later with `app.window({window_ref:state.window})`; use `app.observe({cursor})` for continuation pages.

`await app.quit()` requests normal application termination once. With multiple instances, choose one using `{window_ref}`. `application_exited` confirms that exact process ended; `application_not_running` is a no-dispatch conclusion. A dispatched/unverified result may be waiting on a save dialog: inspect fresh state or a proven related dialog before deciding on new input. Quit and close never force-kill or discard documents. Closing a window does not prove its process exited. Launch/quit/closeWindow are advertised only where implemented; currently macOS.

## Simple action workflow

`app.windows()` returns window refs across instances of the selected app. A ref routes to its exact instance; titles are descriptive and may be empty or non-unique. Select the intended window from returned metadata and observations, then keep its ref in `state`; another `windows()` call is not required before every action. Never construct a ref or substitute a PID or another app ID.

Window inventory is complete only within its declared scope. `association_complete:false` means not every possible helper relationship is established. Related surfaces use the same logical app ID and their own returned window refs. For a blocked target, use `waitForWindow` with `state:'dialog'` to discover a proven related modal when supported; inspect that returned window before new input. If the relationship remains unproven, use fresh inventory or report the boundary.

For a known control, use a selector instead of writing an observation/pagination loop. Selectors combine an exact returned `role` with case-insensitive `text` matching the label, name, or value; provide at least one. The default scope is the selected window; use `selector:{scope:'menu',role:'menu_item',text:...}` for its application menu. Use labels from the current UI:

```javascript
await app.setValue({
  window_ref: state.window,
  selector: {role: 'text_field', text: 'Name'}, value: 'Example'
});
await app.press({
  window_ref: state.window,
  selector: {role: 'button', text: 'Continue'}
});
```

DTW resolves selectors afresh, handles pagination and waits briefly, then acts once. It may prepare cold content or menu focus under service policy; this never authorizes a second action after unknown delivery. Multiple matches are ambiguous; incomplete coverage is unproven. Use `find` to read one unique element, or `observe`/capture to inspect. See [selector/wait details](PATTERNS.md#selector-and-wait-reference) for readiness and timeout behavior.

`wait({selector,state})` reads until present/absent/enabled/disabled; default 10s, bounded by the 30s script deadline. Positive conditions require a complete traversal and unique match; absent requires complete traversal with no match. Enabled/disabled need an explicit Boolean. Wait sends no input, never proves persistence, and never extends refs. `wait_timeout` is catchable; cancellation/deadline remain terminal.

`waitForWindow({state:'dialog'|'closed'|'unblocked'})` follows only proven related modals or the exact selected surface, never arbitrary new/focused windows. Inspect a returned dialog's own ref before input. Defaults for selector resolution and native-window waits remain 2s; zero checks once.

An action receipt marked `unverified` describes its dispatch evidence. It does not require another MCP call or repeated input: wait for the expected UI condition and read back in the same `exec`. A successful wait records only the sampled condition and leaves the earlier receipt unchanged. If the condition times out, inspect the receipt and current state before deciding on new work.

## Images and discovery

Read [image/discovery details](PATTERNS.md#image-and-semantic-discovery-reference) when using region/subtree scopes, paginating a large tree or handling Windows occlusion. Capture returns the last explicit PNG, never an automatic post-action image. Check `last_capture.step/timing/later_calls`; take a fresh capture after input when needed. Historical receipt images are not current bindings.

Click coordinates use returned image dimensions, even if the viewer resized it. For 0–1000 grounding pass `coordinate_space:'normalized_1000'`; both axes must be >=0 and <1000. Do not rescale and also set that flag.

Observe supports window, subtree, captured-region and app-menu scopes where advertised; request `api` for their exact mutually exclusive arguments. Continue with only `{cursor:page.next}`. An empty page can have a continuation; `next:null` does not prove completeness. Preserve `coverage.complete/reason`; incomplete reads cannot prove absence. `name` is native text, while `label` may add a role prefix. Filters search label/name/value, including empty-name controls. Roles and supported actions come from current observations.

## Atomic actions and text

- `press` also accepts `{window_ref,observation_ref,element_ref}` and requires a current semantic element advertising `press`.
- `setValue` also accepts `{window_ref,observation_ref,element_ref,value}` and requires advertised `setValue`. It replaces the native value and may not emit ordinary typing events.
- `typeText({window_ref,text})` sends Unicode keyboard input to the currently selected editing control. Application formatting can change the resulting text: Return/Tab are keys, and editors may auto-continue lists or indentation. For literal replacement prefer `setValue` when the control advertises it; otherwise type into a selected editing control and read back before continuing. Do not repeat an already supplied list prefix when the editor inserted it. A dispatch receipt does not prove an editable target received every character.
- `keypress({window_ref,keys:[...]})` presses and releases one chord. `META` is Command on macOS and the Windows key on Windows; modifier meanings are platform dependent.
- `click({window_ref,observation_ref,x,y,coordinate_space?,button?,count?})` uses a current capture. The default space is `image_pixels`; choose `normalized_1000` for normalized grounding. Buttons are left/right and counts are 1/2.
- `scroll({window_ref,delta_y,delta_x?})` uses wheel steps, positive down/right, at the window center. Supported platforms also accept a current capture's `observation_ref,x,y` or semantic `observation_ref,element_ref` as an anchor.
- `focus({window_ref})` raises the selected window and leaves it foreground after the script. DTW does not switch back automatically. At a task milestone, close or minimize only the task-owned window through its ordinary UI when appropriate. The user may switch away; later input still prefers background delivery and raises the target only when needed. Each action independently validates its target.

Some applications defer background actions until activated. If a delivered action has not appeared, use `focus` followed by `wait`, `find`, `observe` or `capture` in the same script before any further input: activation can reveal the original action. For a window that needs foreground interaction, group `focus` and its dependent actions in one script. App-menu semantic actions also require this same-script focus. Desktop state changes can be asynchronous: a dispatched action does not establish readiness of a later target. Use `wait` for the relevant semantic condition or `waitForWindow` for a modal transition; never repeat an action merely to wait. For an asynchronously opened nonmodal window, refresh `app.windows()` and identify its own ref; `state:"dialog"` is only for proven related modals. Do not keep typing into the parent or use the new window's title as proof of its editable content.

`app.readText({window_ref,observation_ref,element_ref,offset?,limit?})` reads native text in UTF-16 pages without splitting surrogate pairs. Pass returned `next_offset` unchanged until null. A changed text revision requires a new observation; do not join pages from different revisions. Unsupported native text is explicit. Labels and screenshots are not substituted as native text. Return only the relevant text or verification result within the output budget.

## Reference lifecycle and recovery

Each window retains one semantic query and one capture. A new read replaces the same kind on that window; admitted semantic scope replacement uses newly returned refs. Actions consume both kinds on their target window. Other windows and apps retain their state and refs.

Semantic and pixel observations expire at most 15 seconds after acquisition; pagination does not extend that lifetime. Obtain a fresh binding before a later action. Invalid or unsupported scope admission preserves the current refs; a failed continuation retires its query.

On `stale_observation`, `target_changed`, `focus_changed` or `focus_unproven`, stop the affected sequence and inspect the target. Refresh windows if identity changed, or observe/capture the still-current target. Unrelated pointer movement does not invalidate keyboard input.

`context_reset:true` means the native context was replaced. Logical state can survive, but refs embedded in it must be reacquired through fresh inventory and observations. The original failed execution remains separate; do not replay its source automatically.

`dispatched` proves delivery and `completed` proves script execution, not the requested application effect. Verify the relevant native or visual state. Unknown or interrupted input may already have effects: retain the original receipt and inspect before recovery. JSON state rollback does not undo those effects.

## Results and platform conditions

Ordinary `exec` returns a compact summary; successful work needs no polling. After cancellation, status can remain running while cleanup settles. Recover without an app/native owner, for example `exec({source:"const r=await session.result('returned execution_id');print(r.status,r.error?.code);"})`, then inspect its original step receipts and independent state before new input. Default `session.result()` skips pure receipt queries and selects prior accepted work; a lost request may leave an older record. Prefer its returned ID.

Queries accept `{execution_id?,offset?,limit?}` and return `script.next_offset`; use small projections within the normal return/print budgets. Native cleanup failure does not block this lane. Queries never replay work or refresh refs, and recovered images are historical. Read [receipt recovery details](PATTERNS.md#receipt-recovery) for pages, bounds and errors.

DTW selects semantic, targeted background or foreground delivery internally. Normal installation uses `auto`: DTW raises the exact target when foreground input is needed, before dispatch. Agents use the ordinary methods without routing parameters. A host explicitly configured with the restricted `background_only` policy can prohibit that route; `foreground_required` then describes that service constraint, not an approval request.

Delivery metadata records the actual route. On macOS, `script.focus_return:"retained"` means cleanup left foreground in place; `changed_externally` preserves the user's new foreground. These are not guarantees that focus persists until the next call. `unconfirmed` means cleanup status was not observed; missing metadata means unavailable evidence. Capabilities vary by platform; locked-screen operation is unsupported.

An `unconfirmed` focus return without a `shutdown_error` permits a fresh read of the target; inspect the original action receipt and current state before new work. It does not authorize replay of the earlier action. A `shutdown_error` indicates failed native cleanup and blocks further desktop execution in that connection. The provider-free Session lane remains available for receipt inspection; it cannot repair native cleanup or replay the original work.
