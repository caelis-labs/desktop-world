# Native browser operations

These operations use the same private worker and session as scripts. They are
not `cua.call` methods. Read the exact worker schema without performing an action:

```json
{"operation":"describe","name":"browser_prepare"}
```

Supply native parameters under `arguments`; never provide `session`, private
context fields, a CDP endpoint, or invented target/ref IDs. Results use normal
DTW receipts. Poll a running receipt. Bindings, refs, outline and continuations appear directly
in the completed receipt. Follow `more` only when the response is truncated;
`part:"structured"` keeps raw diagnostics.
Use `part:"image"` for the retained PNG. Input is never automatically replayed.

## Bind and observe

Start with `{"operation":"describe"}` and follow the configured target's
`next_call`. It works in bounded mode without scripts or desktop enumeration.
Otherwise discover the task's PID/window with the script API when available.
For setup, follow the returned DTW `next_call`; if it says `requires:"host"`,
report that requirement. Do not copy session fields from raw upstream diagnostics.

In standard mode with `run` available, for a task explicitly using a fresh isolated browser:

```json
{"operation":"browser_prepare","arguments":{"allow_launch":true,"profile":{"mode":"isolated_new"}}}
```

Read `prepared_pid`, enumerate its windows, then bind with `get_browser_state`.
`isolated_named` also accepts a path-safe `name`; it retains its own profile.
Neither mode copies a personal profile. Read the returned lifecycle: isolated
browser processes belong to the session, and end on session shutdown/expiry.
DTW gives an idle session a bounded graceful shutdown to release its browsers;
forced cancellation or a crash can still leave isolated-browser cleanup incomplete.

Binding returns `target_id` and `tabs[].tab_id`. Observe an exact tab:

```json
{"operation":"get_browser_state","arguments":{"target_id":"RETURNED_TARGET","tab_id":"RETURNED_TAB","snapshot_format":"semantic_v2","include_screenshot":true}}
```

Choose action refs by role/name/actions from this snapshot. `content_refs` and
`outline` are for reading. Use `query`, `scope_ref` or `continuation` when needed.
Every newer snapshot of the tab and every navigation invalidates older refs.
Browser screenshot pixels map to viewport CSS coordinates using the returned
`screenshot.pixel_to_css_scale_x/y`; these are not native-window pixels and have
no `capture_id` or application `observation_id`.

## Act and verify

Each call includes the same exact `target_id` and `tab_id`:

| Operation | Other native arguments / behavior |
| --- | --- |
| `browser_navigate` | `url` (`http:`, `https:`, `about:`); then take a new snapshot. |
| `browser_type` | Current `ref`, `text`, optional `replace:true` and `mode:"insert_text"` or `"keystrokes"`. Replacement preserves input events. |
| `browser_click` | Current `ref` or CSS `x,y`. Default trusted input may refuse background activation. Explicit `input_route:"dom_event"` sends synthetic DOM events. |
| `browser_select` | Current select `ref`, exact `value` or visible `label`. |
| `browser_pointer` | `action:"hover"/"right_click"/"double_click"/"scroll"/"drag"`; current ref or CSS coordinates, deltas or destination as described by the native schema. |
| `browser_dialog` | `action:"inspect"` returns the current dialog generation. `accept`/`dismiss` requires that exact `dialog_id`; accepting a prompt optionally uses `prompt_text`. |
| `browser_set_input_files` | File-input `ref`, `files` containing scoped absolute paths to regular files. Retains native upload consent. |
| `browser_download` | Current initiating `ref`, existing absolute `destination_root`. Harness approves the tool call; CUA checks the binding and file scope. |

In CUA 0.38, call `browser_dialog` with `action:"inspect"` **before** a known
JavaScript-dialog trigger to register dialog events. A synchronous prompt can
still make the triggering click time out; do not replay it. Inspect and resolve
the dialog's returned generation instead. First registering after a dialog is
already open can itself time out. This is an upstream limitation, not a verified
smooth prompt flow. Native OS file pickers can use the application screenshot
route only when `describe` lists that capability; bounded browser-only scope
requires host help instead.

File assignment to an isolated page was verified with a task-owned local file.
`browser_download` accepts a normal absolute directory path (including ordinary
Windows paths); DTW canonicalizes it for CUA. The Harness owns tool approval,
with no additional DTW form or approval argument. Native manifest, exact binding,
ref freshness and output validation still apply. A receipt reports an opaque
download ID and byte count, not source URLs or filenames. Do not replay a timed-out
download.

## Diagnostics

`health_report` is read-only. Its `include` and `skip` arrays select canonical
checks (`include` wins). An empty object runs defaults. Complete structured data
retains status, checks and provenance even if the text summary is abbreviated.
Screen-capture readiness can be skipped because probing it could prompt.

`get_guide` with a native topic such as `browser` or `permissions` returns the
pinned guide. `describe` returns authoritative native schemas for the ten browser
operations plus `health_report` and `get_guide`. It does not grant additional
authority or expose arbitrary driver tools.
