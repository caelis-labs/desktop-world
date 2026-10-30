# Common interaction patterns

Use these patterns with the API and reference lifecycle in [SKILL.md](SKILL.md). They apply to classes of desktop controls, independently of any particular application or task. Names, values, coordinates, window refs, and supported actions must come from the current UI and the user's task.

## File choosers and native dialogs

Opening a chooser may create a separate sheet, dialog, or related helper surface. Refresh `app.windows()` after the opening action. Route subsequent calls to the returned dialog ref, retaining the parent ref separately. Titles can be empty or repeated: use the inventory change and captures to identify the active surface. DTW validates ownership and modal relationships internally. Do not select a window solely because its title is blank or it is the last item.

Capture the dialog once to identify the filename/path field, directory contents, and confirmation controls. Large directory lists can dominate an AX/UIA traversal even when filtering for text fields. Query a semantic subtree or a capture region around the relevant controls before scanning the entire dialog. Regions are image coordinates; derive them from the current capture rather than a remembered layout.

On macOS, a native file chooser commonly supports **Command+Shift+G** to enter a directory path. This can open another sheet. Refresh inventory and inspect it before entering the path; after confirming that navigation, refresh and reacquire the chooser controls. The path-navigation field and the filename field are different targets. Other platforms and custom choosers may use different controls; inspect their actual UI.

For a filename field, prefer `setValue` only when it advertises that action. Otherwise establish its editing focus using a current semantic action or capture-based click, then use the platform's select-all chord and `typeText`. DTW chooses delivery internally, including foreground fallback for native modal input. Do not assume a delivered keystroke updated the field: read or capture the value before confirming the chooser.

Confirm once after checking the selected path/name and the enabled control. A closed dialog alone does not prove the requested save, open, or upload completed. Inspect the receiving application's resulting state. If confirmation has an unknown outcome, inspect that state and the original receipt before deciding what new work is needed; do not submit it again automatically.

## Forms and dynamic editing controls

Discover a form's controls with role/text filters and, when useful, a subtree or region. Select controls by the observed label, role, enabled state, and advertised action together. A text match can be a label rather than an editable field. If several candidates remain, inspect their container or geometry instead of choosing the first one.

`setValue` replaces a native value without promising ordinary typing/change events. `typeText` sends literal text to the selected editing control, without locating that control. Choose the method supported by the current field, and verify the effect that matters to the application. Do not silently try both methods after an uncertain write.

A semantic press or shortcut can start an asynchronous UI transition. Delivery does not prove that the new editing control is ready or focused. Before dependent text input, observe or capture the relevant control and establish its editing focus. A bounded readiness check may repeat reads; it must not repeatedly press the control or type the text as a waiting strategy.

Actions consume the target window's current semantic and pixel bindings. For a multi-field form, retain logical selectors and intended values, then reacquire each target immediately before its action. Do not collect all element refs once and reuse them after earlier edits. Stable selectors and small JSON values can be stored in `state`; helper functions belong in the current script.

For toggles, inspect the actual state before pressing: pressing changes state rather than assigning a desired value. For multiline fields, pass real newline characters in the JavaScript string and let the MCP client's JSON serializer escape source once. Literal backslash-plus-`n` text is different from a newline.

For example, the JavaScript source `text: 'first line\nsecond line'` produces a newline in the text value. Let the client serialize that source as JSON; do not manually add another layer of backslash escaping.

After editing, check relevant field values, validation messages, and commit-control state. Commit once, then verify the application's resulting state. Tool delivery, script completion, a field displaying new text, and persisted application data are distinct evidence.

## Large trees and bounded discovery

For a long list or table, first look for the application's own search, filter, or page controls. Native `observe` filters the currently exposed accessibility tree; it does not load other application pages or materialize virtual rows. Use the application's controls to bring the relevant records into view before discovering their actions. Accessibility cursor pagination and application pagination are different operations.

`limit` limits returned matches, not native traversal work. Use the smallest relevant scope: a known container, a visible region from a current capture, or the app-menu scope. Add role/text filters based on observed controls. Scanning a full window repeatedly for each field is expensive, especially in long documents, directory views, and browser accessibility trees.

An empty page can still have `next`. Continue with `{cursor: next}` when further matches are needed. Keep continuation reads together in one script when practical; bindings have a lifetime and pagination does not renew it. Stop when the question is answered, or preserve the final coverage and its partial reasons. Do not infer absence from `next:null` alone.

If a broad query is incomplete or too costly, use a fresh capture to choose a narrower region, or obtain a container and query its subtree. Reacquire the target after scrolling or any action. A screenshot-backed coordinate action remains a valid route when semantics are unsupported or do not expose the necessary control; use current image coordinates and verify the result.

Some applications expose only window decorations while inactive. If a capture shows controls but semantic discovery returns only the window shell or `content_unproven`, try `focus` followed by `observe` in the same script when foreground delivery is permitted. This can reveal semantic controls without guessing coordinates. If they remain unavailable, use a fresh capture and preserve the partial coverage.

Return a compact decision record: relevant candidates or values, unresolved ambiguity, coverage reasons, and an image only when it helps the next decision. Keep traversal details and unrelated text out of the model response.

## Menus and focus-dependent interactions

Use `scope:'menu'` for the application's menu tree when advertised. App-menu actions apply to the focused window, so establish focus and perform the dependent menu action within the same script. An explicit `focus` does not reserve foreground focus for a later script.

Some controls accept directed input only while their window is active. If a dispatched coordinate action has no visible effect, inspect the current state before deciding the next action; do not repeat clicks to test delivery. When another input is appropriate, establish `focus` and reacquire its target in that same script. This also applies to custom controls whose semantic actions are unavailable.

After an action opens a modal or associated surface, refresh inventory before continuing. DTW resolves target ownership, delivery, and foreground return internally. An unrelated pointer movement does not invalidate keyboard input, but a changed target or unproven focus requires fresh inspection. Keep unknown effects separate from new work, and never replay an earlier script merely because its state did not commit.
