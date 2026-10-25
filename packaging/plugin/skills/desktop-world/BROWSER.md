# Native browser operations

These operations use the same private worker and session as scripts. They are
not `cua.call` methods. Read the exact worker schema without performing an action:

```json
{"operation":"describe","name":"browser_prepare"}
```

Supply native parameters under `arguments`; never provide `session`, private
context fields, a CDP endpoint, or invented target/ref IDs. Results use normal
DTW receipts. Poll a running receipt; retrieve `part:"structured"` for complete
native bindings, error context, side effects, continuations and diagnostic data.
Use `part:"image"` for the retained PNG. Input is never automatically replayed.

## Bind and observe

For an existing browser, enumerate the exact PID/window using the script API,
then call `get_browser_state` with `{pid,window_id}`. If setup is needed,
`browser_prepare` supports `{pid,window_id,strategy:{kind:"existing_profile"}}`.
Existing profile access requires upstream runtime authorization. The host can
approve a native capability manifest at DTW startup, as described below. Without
it, standard mode preserves `browser_consent_required`. A desktop MCP tool approval
or model-provided boolean is not a native profile grant.

For a task explicitly using a fresh isolated browser:

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
| `browser_download` | Current initiating `ref`, existing canonical `destination_root`. Retains native download consent. |

In CUA 0.38, call `browser_dialog` with `action:"inspect"` **before** a known
JavaScript-dialog trigger to register dialog events. A synchronous prompt can
still make the triggering click time out; do not replay it. Inspect and resolve
the dialog's returned generation instead. First registering after a dialog is
already open can itself time out. This is an upstream limitation, not a verified
smooth prompt flow. Native OS file pickers use the application screenshot route.

File assignment to an isolated page was verified with a task-owned local file.
`browser_download` asks the MCP host for an explicit form confirmation of that
one target/tab/ref/destination. Accepting the form with `approve:true` carries
CUA's native destructive-tool approval evidence through the trusted SDK adapter.
The public arguments never include that evidence. A host without form elicitation,
decline, cancel, invalid response or 120-second timeout refuses without dispatch.
The originating tools/call stays pending during confirmation; other actions return
`runtime_busy`. Set host tool-call timeouts accordingly. Native manifest, exact
binding, ref freshness and output validation remain required after confirmation.
Completed native output intentionally contains an opaque download ID and byte count,
not source URLs or filenames. Do not replay a timed-out download.

## Host-approved existing profile

The launcher must review exact grants and pass both flags (never synthesize
approval from a tool request):

```sh
dtw --capability-manifest /absolute/reviewed-browser.yaml --approve-capability-manifest
```

Add these arguments to the MCP host's DTW server configuration. Restart after
changing grants. DTW freezes the approved file before CUA initializes; changes to
the original file cannot widen a running process. Invalid manifests fail startup.
This is the native CUA **bounded** authorization path. Example native v2 manifest:

```yaml
version: 2
mode: bounded
expires_after: 20m
idle_timeout: 10m
resources:
  browser:
    existing_profiles:
      - pid: 12345      # replace with the exact reviewed running process
        window_id: 678 # replace with the exact reviewed native window
    origins: ["https://app.example.com"]
  files:
    write:
      - dir: /absolute/existing/canonical/downloads
        recursive: false
allow:
  tools: [start_session, end_session, browser_prepare, get_browser_state,
          browser_navigate, browser_click, browser_type, browser_select,
          browser_pointer, browser_dialog, browser_download, health_report]
```

Use the narrowest tools/resources needed; uploads additionally need the native
read-file grants and `browser_set_input_files`. `describe` still gives exact schemas.
An existing endpoint must be owned by the approved PID; any native setup side
effects are reported by CUA. Task-owned real Chrome acceptance verified attachment
to an already running profile with an existing endpoint, without copying profile
data, relaunching Chrome or changing preferences. It did not exercise a personal
logged-in account. Disconnecting DTW preserves the pre-existing browser process.

The DTW SCK `app`/`click` extensions are unavailable in bounded mode because they
cannot enforce native manifests; use permitted native browser/desktop routes.
After 25 minutes of DTW context age or idle time, restart and observe again;
shorter native manifest lifetimes still apply. There is no automatic grant renewal.

CUA's embedded `DriverAuthorizationHost` provides a different, protected dynamic
profile consent mechanism. The released private-worker protocol does not transport
that callback. DTW does not replace it with ordinary MCP elicitation; interactive
profile callback support remains a separate upstream transport/integration gap.

For click/pointer, `delivery_mode:"foreground"` explicitly permits the trusted
route to activate the browser. A background refusal does not authorize changing
route. Choose the route according to the user's task; do not silently fall back
or replay. Native receipts distinguish dispatch and effect. Refresh page state
and check the actual postcondition before another action. Keep native AX/SCK
routes for browser chrome and operating-system dialogs.

## Diagnostics

`health_report` is read-only. Its `include` and `skip` arrays select canonical
checks (`include` wins). An empty object runs defaults. Complete structured data
retains status, checks and provenance even if the text summary is abbreviated.
Screen-capture readiness can be skipped because probing it could prompt.

`get_guide` with a native topic such as `browser` or `permissions` returns the
pinned guide. `describe` returns authoritative native schemas for the ten browser
operations plus `health_report` and `get_guide`. It does not grant additional
authority or expose arbitrary driver tools.
