# Common interaction patterns

Use these patterns with the API and reference lifecycle in [SKILL.md](SKILL.md). They apply to classes of desktop controls, independently of any particular application or task. Names, values, coordinates, window refs, and supported actions must come from the current UI and the user's task.

## File choosers and native dialogs

Opening a chooser may create a separate sheet, dialog, or related helper surface. After the opening action, use `app.waitForWindow({window_ref: parent, state:"dialog"})` when supported; DTW waits for the transition and returns the uniquely proven related modal ref. Retain parent and dialog refs separately. Unsupported or unproven relationships require fresh `app.windows()` and inspection; empty titles, list position, and newly focused windows do not establish a relationship.

On Windows, a Chrome file window can appear as an ordinary window while the selected parent remains enabled. `dialog_not_present` does not prove that opening failed. Inspect fresh windows and their actual controls, and continue only on the positively identified intended surface. Keep an ordinary window ref separate from a proven child-modal relationship. If ownership or identity remains ambiguous, retain that uncertainty; do not repeat the opening shortcut to discover what happened.

Capture the dialog once to identify the filename/path field, directory contents, and confirmation controls. Large directory lists can dominate an AX/UIA traversal even when filtering for text fields. Query a semantic subtree or a capture region around the relevant controls before scanning the entire dialog. Regions are image coordinates; derive them from the current capture rather than a remembered layout.

On macOS, a native file chooser commonly supports **Command+Shift+G** to enter a directory path. This can open another sheet. Wait for its proven relationship before entering the path, then wait for that exact sheet to close before continuing in the chooser:

```javascript
await app.keypress({window_ref: state.chooser, keys:['META','SHIFT','G']});
state.path_sheet = (await app.waitForWindow({window_ref: state.chooser, state:'dialog'})).window_ref;
return await app.observe({window_ref: state.path_sheet, role:'text_field'});
```

Inspect the path field and its supported action. After confirming navigation once, use `waitForWindow({window_ref: state.path_sheet, state:'closed'})`, then reacquire chooser controls. The path-navigation field and filename field are different targets. Other platforms and custom choosers may use different controls; inspect their actual UI.

For a known filename field, `setValue` with a selector locates it afresh and sets it once, provided it advertises that action. Otherwise establish its editing focus using a current semantic action or capture-based click, then use the platform's select-all chord and `typeText`. DTW chooses delivery internally, including foreground fallback for native modal input. Do not assume a delivered keystroke updated the field: read or capture the value before confirming the chooser.

Confirm once after checking the selected path/name and the enabled control. If waiting for closure times out, the application may have opened a nested confirmation or directory-access dialog. Inspect a fresh capture or call `waitForWindow({window_ref: state.chooser, state:'dialog', timeout_ms:0})` to discover a proven related modal before new input; do not click the old confirmation again. Window waits refer to native surfaces: closing a document or tab can leave its containing window open, so inspect the resulting UI for those transitions. A closed dialog alone does not prove the requested save, open, or upload completed. Inspect the receiving application's resulting state. If confirmation has an unknown outcome, inspect that state and the original receipt before deciding what new work is needed; do not submit it again automatically.

## Forms and dynamic editing controls

Discover a form's controls with role/text filters and, when useful, a subtree or region. Select controls by the observed label, role, enabled state, and advertised action together. A text match can be a label rather than an editable field. If several candidates remain, inspect their container or geometry instead of choosing the first one.

`setValue` replaces a native value without promising ordinary typing/change events. `typeText` sends Unicode keyboard input to the selected editing control, without locating that control. On macOS, newline characters generate Return (CRLF once) and tabs generate Tab; rich editors may continue lists, indent text, or move focus. Verify the resulting content, including any automatic formatting. Choose the method supported by the current field, and verify the effect that matters to the application. Do not silently try both methods after an uncertain write.

A semantic press or shortcut can start an asynchronous UI transition. Delivery does not prove that the new editing control is ready or focused. For a known next control, `wait` can require its presence or explicit enabled state; selector actions also resolve a unique target and briefly wait while it is disabled, using reads only. `find` waits only for a unique match and reports its current `enabled` state; it can return a disabled control. For unstructured editors, observe or capture and establish editing focus before typing. Use `waitForWindow` for modal transitions. Do not repeatedly press or type as a waiting strategy.

Keep a known transition and its readback in one `exec`, using names and roles already observed in that application:

```javascript
const window_ref = state.window;
await app.press({window_ref, selector:{role:'button', text:'Edit'}});
const field = await app.wait({
  window_ref, selector:{role:'text_field', text:'Name'}, state:'enabled'
});
await app.setValue({
  window_ref, observation_ref:field.observation_ref,
  element_ref:field.element_ref, value:'Example'
});
await app.press({window_ref, selector:{role:'button', text:'Save'}});
const saved = await app.wait({
  window_ref, selector:{text:'Saved successfully'}, state:'present'
});
await app.capture({window_ref});
return {confirmation:saved.element};
```

The enabled field must advertise `setValue`; otherwise choose its supported editing route. Each action runs once. The waits use fresh discovery, not the refs consumed by the preceding action. `wait` defaults to ten seconds and shares the original script deadline with all other steps; it returns immediately when the condition is met. `state:'absent'` can wait for a known loading indicator to leave the exposed semantic tree, but this alone does not establish that expected data loaded: also wait for/read the relevant resulting state. Partial discovery cannot establish absence. No fixed sleep or extra MCP round trip is needed for a known condition.

Actions consume the target window's current semantic and pixel bindings. For a multi-field form, retain selectors and intended values; selector actions reacquire each target internally. Do not collect all element refs once and reuse them after earlier edits. Stable selectors and small JSON values can be stored in `state`; helper functions belong in the current script.

For toggles, inspect the actual state before pressing: pressing changes state rather than assigning a desired value. For multiline fields, pass real newline characters in the JavaScript string and let the MCP client's JSON serializer escape source once. Literal backslash-plus-`n` text is different from a newline.

For example, the JavaScript source `text: 'first line\nsecond line'` produces a newline in the text value. Let the client serialize that source as JSON; do not manually add another layer of backslash escaping.

Native value updates can be asynchronous. To wait for a short expected value, use `wait` with `state:'present'`, the field role and that value as `text`, then compare its returned `element.value`; a role-only match can return the old value immediately. Text selection is a substring match, so compare the expected value explicitly. This only waits for native field state, not persistence or application validation.

After editing, check relevant field values, validation messages, and commit-control state. Commit once, then verify the application's resulting state. Tool delivery, script completion, a field displaying new text, and persisted application data are distinct evidence.

## Large trees and bounded discovery

For a long list or table, first look for the application's own search, filter, or page controls. Native `observe` filters the currently exposed accessibility tree; it does not load other application pages or materialize virtual rows. Use the application's controls to bring the relevant records into view before discovering their actions. Accessibility cursor pagination and application pagination are different operations.

`limit` limits returned matches, not native traversal work. Use the smallest relevant scope: a known container, a visible region from a current capture, or the app-menu scope. Add role/text filters based on observed controls. Scanning a full window repeatedly for each field is expensive, especially in long documents, directory views, and browser accessibility trees.

Generic pagination, using a window ref already selected into state:

```javascript
let page = await app.observe({
  window_ref: state.window, role: 'text_field', limit: 12
});
const elements = [];
for (;;) {
  elements.push(...page.elements);
  if (!page.next) break;
  page = await app.observe({cursor: page.next});
}
return {elements, coverage: page.coverage};
```

An empty page can still have `next`. Continue with `{cursor: next}` when further matches are needed. Keep continuation reads together in one script when practical; bindings have a lifetime and pagination does not renew it. Stop when the question is answered, or preserve the final coverage and its partial reasons. Do not infer absence from `next:null` alone.

If a broad query is incomplete or too costly, use a fresh capture to choose a narrower region, or obtain a container and query its subtree. Reacquire the target after scrolling or any action. A screenshot-backed coordinate action remains a valid route when semantics are unsupported or do not expose the necessary control; use current image coordinates and verify the result.

Some applications expose only window decorations while inactive. If a capture shows controls but semantic discovery returns only the window shell or `content_unproven`, try `focus` followed by `observe` in the same script when foreground delivery is permitted. This can reveal semantic controls without guessing coordinates. If they remain unavailable, use a fresh capture and preserve the partial coverage.

Return a compact decision record: relevant candidates or values, unresolved ambiguity, coverage reasons, and an image only when it helps the next decision. Keep traversal details and unrelated text out of the model response.

Native paragraph text can be split across several accessible elements. `name`, `value` and `label` are discovery excerpts, not complete paragraph reads. Use `readText` on a currently observed text container when it advertises native text support, and page its returned offsets. There is no implicit parent-navigation or guaranteed visual-paragraph reconstruction; obtain any container from actual discovery rather than inventing a parent ref.

For an unnamed icon button, use a fresh capture to identify a small region, then observe that region with the returned button role. An unambiguous current `element_ref` with `can_press:true` can be pressed directly; otherwise use the current capture's pixel coordinates. Native semantic activation and physical clicking may trigger different application handlers. Verify the resulting state before choosing new input; never automatically follow an accepted but ineffective press with a second click.

## Menus and focus-dependent interactions

For a known application-menu command, use `app.press({window_ref, selector:{scope:'menu',role:'menu_item',text:...}})`. DTW proves a unique match, prepares focus and resolves the menu again before pressing once. `app.find` with the same selector reads without focusing. Menu scope must be advertised by the provider; it does not search window controls or context menus.

Use `app.observe({window_ref,scope:'menu',...})` to explore the application's menu tree. For direct ref-based app-menu actions, establish focus and observe/act within the same script; app-menu actions apply to the focused window. An explicit `focus` does not reserve foreground focus for a later script.

Some applications defer a background semantic or coordinate action until their window becomes active. A delivered action with no visible effect may still be pending. First use `focus` and a fresh read in the same script, without repeating the action; the original effect may then appear. When the resulting state calls for new input, keep `focus` and the dependent actions in one script. This also applies to custom controls whose semantic actions are unavailable.

After an action opens a modal, use `waitForWindow` on the selected parent when supported; otherwise refresh inventory and inspect. DTW resolves target ownership, delivery, and foreground return internally. An unrelated pointer movement does not invalidate keyboard input, but a changed target or unproven focus requires fresh inspection. Keep unknown effects separate from new work, and never replay an earlier script merely because its state did not commit.


## Image and semantic discovery reference

`app.capture({window_ref})` returns an image and a pixel observation ref. The image accompanying a script is its last explicit capture, not an implicit screenshot after clicks or typing. Check `last_capture.step`, `timing` and `later_calls`; call capture after an action when visual readback is needed. A changed or unchanged image alone does not prove the intended effect or justify replay. Coordinates refer to that image, not desktop or CSS coordinates.

On Windows, an explicit occlusion failure during `exec` can make DTW prepare the selected window's foreground once and capture afresh. This uses ordinary input coordination and the original deadline; its focus receipt is retained in the capture step's `resolution.focus_preparation`. That preparation retires this window's semantic refs too. A denied or unconfirmed preparation stops. No delivery-mode argument is needed, and a failed capture does not authorize repeating prior input.

For pixel grounding, use the returned `image.width` and `image.height`, even if a viewer resizes the image. For 0–1000 grounding, pass `coordinate_space:"normalized_1000"` to `click`; DTW converts both axes through the bound capture. Both coordinates must be in the same space; normalized values are at least 0 and less than 1000. Do not manually rescale normalized coordinates and also mark them normalized.

`app.observe` supports these exclusive scopes, subject to `supported_observe_scopes`:

- Window: `{window_ref, role?, text?, limit?}`.
- Subtree: `{window_ref, observation_ref, element_ref, role?, text?, limit?}`, using a current semantic container.
- Region: `{window_ref, observation_ref, region:{x,y,width,height}, role?, text?, limit?}`, using a current capture.
- App menu: `{window_ref, scope:'menu', role?, text?, limit?}`.

Region queries filter native geometry. macOS clips through proven scroll-area viewports and skips disjoint scroll contents; Windows uses UIA bounding rectangles. Region discovery is not OCR or an occlusion guarantee. Capture support does not imply semantic support.

Each page contains `elements`, `observation_ref`, `next` and `coverage`. Element fields are `role,name,value,label,enabled,actions,element_ref,can_press`. Names, labels and values are bounded excerpts. Roles and advertised actions come from the provider; an intended task does not establish a control's name or capabilities.

`name` is the native accessible name. `label` is a display summary and may include a role prefix, such as `text Example`; that prefix is not part of the native name. Prefer the observed role and name text when constructing selectors. An empty name does not imply a missing element.

`limit` bounds matches per page, not traversal work. Narrow the scope and filters when possible. Continue with only `{cursor:page.next}`; a cursor advances once. An empty page can have a continuation. Earlier-page refs share the same observation.

Text filters apply to label, name and value, including empty-name controls. A filtered page can be empty while `next` is present: continue the same query, or use `find` for a known control. Browser window scope can start with browser chrome before page contents; narrow by role/text or a captured region instead of dumping the full tree. `next:null` does not imply complete discovery. `coverage.complete` describes the live traversal of the selected scope; it is not an atomic snapshot or proof that an element does not exist. Preserve partial-coverage reasons. Missing, disabled, ambiguous and unsupported are different outcomes.

Windows region queries filter returned UIA geometry; they do not necessarily prune traversal work. A retained, observed container subtree is preferable when available. Page bounds describe traversal work separately from returned matches, so an empty bounded page does not establish that the page is still loading or has no matches.



## Selector and wait reference

DTW refreshes each target, handles pagination and briefly waits, then acts once. If an inactive application exposes only its window shell (`content_unproven`), selector actions may raise the selected window once under the configured delivery policy and resolve afresh before sending input. For `scope:'menu'` actions, DTW first proves a unique match, prepares the selected window's focus and resolves the menu afresh; no separate `focus` call is needed. `find` remains a read. Optional `timeout_ms` defaults to 2000; zero disables polling and cold-window preparation, but retains the menu action's focus preparation. Multiple matches return candidates; incomplete coverage is unproven. Narrow or inspect those cases rather than taking the first match. `app.find({window_ref,selector,timeout_ms?})` waits for one unique element and returns its refs plus `element` metadata, useful for reading its current bounded value. Use `observe` for exploration and large trees requiring a smaller scope.

On supported providers, `app.waitForWindow({window_ref,state:'dialog'})` waits for a proven related modal and returns its own fresh `window_ref`. Use `state:'closed'` to wait for that exact dialog to disappear, or `state:'unblocked'` for the original window's modal gate to clear. It never follows arbitrary new or focused windows. Optional `timeout_ms` has the same default; condition timeout is a normal error and does not replay input. See the file-chooser pattern for nested dialogs.

`app.wait({window_ref,selector,state:'present'|'absent'|'enabled'|'disabled',timeout_ms?})` waits for a semantic condition within this script. Its default is **10000ms**, maximum 30000ms, bounded by the remaining 30-second execution budget; zero checks once. Positive states require a complete traversal and one unique match; enabled/disabled require the explicit Boolean state, never null. They return fresh `observation_ref`, `element_ref` and current `element` metadata like `find`, plus `state`. Absent requires a complete traversal with no exposed match and returns only `window_ref,state`; incomplete coverage cannot establish absence. Every fresh query replaces this window's previous semantic refs while preserving captures and other windows. Polling uses read traversals separated by a 50ms delay; large trees still cost native reads. No focus or input is sent. A `wait_timeout` can be caught for inspection, but execution deadline/cancellation remain terminal. Waiting never extends refs' TTL or proves persistence.



## Receipt recovery

Ordinary `exec` returns synchronously with a compact summary. After cancellation an execution can remain `running` while cleanup settles. For its original receipt, use a provider-free script: `exec({source:"const r=await session.result('returned execution_id'); return {status:r.status,error:r.error,steps:r.script.steps,next:r.script.next_offset};"})`. `session.result()` selects the latest previously accepted work execution, excluding pure receipt-query executions and the querying execution itself; a bound script restricts queries to its app, while the app-less lane can inspect this connection's records. A lost request may leave an older latest record. Prefer the returned ID when available.

Receipt queries accept `{execution_id?,offset?,limit?}`; default 16 steps, maximum 64, with a 256 KiB step-page budget. Follow `script.next_offset`; project small fields before returning because the ordinary 64 KiB result and print budgets still apply. A single oversized step returns `receipt_page_limit`. Original errors, dispatch facts and payload-expiry flags remain intact. Recovered images are historical (`timing:'historical'`), not a fresh capture or live click binding. Queries never refresh native refs or replay work. The old `result` tool is unadvertised and temporarily retained only for compatibility.
