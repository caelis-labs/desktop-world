# DTW desktop world, JavaScript and model result contract — review draft

Status: **proposal only**, 2026-10-10. This document changes no runtime or interface. The full native POC gate remains closed. The [saved real task trace](ECONOMY_TASK_TRACE.md) demonstrates excessive escaped JSON, IDs and normal coverage flags. The earlier output-only draft was premature: a compact result is useful only when its objects and verbs are exactly those the next `exec` script accepts.

## First define the world

DTW is a desktop object world, not a pixel-only CUA. An App is an independently owned participant; its windows are interaction contexts, and controls expose finite affordances. The public script object graph should be small:

| Object | Model-relevant facts | Script relationship / responsibility |
| --- | --- | --- |
| `World` (`dtw`) | available Apps and changed/partial observation | discover one App; hold a Session-local address book |
| `App` | exact name, current instance, lifecycle, grant status | list/select its windows and App-owned menus/popups; never infer ownership from a title alone |
| `Window` | title, main/popup relation **when proven**, visible structure | bounded `one`/`find`/`outline`, read and capture; parent of controls only when provider/scope proves it |
| `Element` | address, role/name, requested value/state, **available behavior methods** | read and perform one of the finite behaviors below |
| `Seat` | foreground/focus/pointer health | shared physical resource coordinated by the executor, not a model-selectable transport |
| `Grant` | active/pending/revoked/expired scope | trusted host supplies authority; script may inspect or revoke, never self-grant |
| `Run/Receipt` | per-step delivery, verification, partial/unknown, restoration | internal execution fact keyed by original `execution_id`, not an AX node |
| `Capture/Asset` | selected window image and target-local geometry | visual observation on demand, not an input target or authorization shortcut |

An App-owned menu may have no trustworthy window parent. Geometry can suggest `top` or `left` for navigation but does not create a semantic sidebar, a main/popup link, or action authority. Window and element addresses point to **observed native instances** inside one Session; names, roles and visual positions are descriptive facts, not identity.

## The finite behavior vocabulary

The existing core already has these behavior classes. The JavaScript names below are proposed ergonomic wrappers; they are not new native capabilities.

| Read/explore | Semantic intent | Directed physical input | Execution/control |
| --- | --- | --- | --- |
| `app`, `windows`, `window`, `one`, `find`, `outline`, `read`, `changes`, `capture` | `invoke`, `setValue`, `setChecked(bool)`, `setSelected(bool)`, `setExpanded(bool)`, `scrollIntoView` | `focus`, `move`, `click`, `dragTo`, `scroll`, `press`, `typeText` | JS `transaction`; MCP `exec` operations `status`, `result`, `cancel` |

`invoke` and `click`, `setValue` and `typeText`, `scrollIntoView` and wheel `scroll` express **different behaviors** and must not be silently substituted after dispatch. For a requested behavior, the executor chooses a proven semantic/background/short foreground channel **before dispatch**; script input never includes `transport` or mode. If no verified route can carry that behavior, return unavailable. Unknown or partial delivery never triggers a physical replay. `bind`, `bind_focus`, guards and waits remain internal plan machinery or explicit advanced options, not extra model-facing desktop verbs.

| Existing capability | Proposed script shape | Important boundary |
| --- | --- | --- |
| Scoped discovery, locator, hierarchy, continuation | `dtw.app`, `app.windows/window`, `win.one/find/outline`, `dtw.at` | exact/unique/complete is checked in JS helper; broad trees are explicit |
| Value, state, changes | `element.read('value'/'checked'/...)`, `win.changes()` | fact status and freshness stay typed inside JS; redacted/unknown are not empty values |
| Semantic invocation and setters | `element.invoke/setValue/setChecked/setSelected/setExpanded/scrollIntoView` | explicit booleans for checked/selected/expanded; native availability and verification are authoritative |
| Screenshot | `win.capture('windowContent')`, scoped `dtw.capture('visibleRegion', ...)` | on-demand image; AX action and capture identities are joined only when proven |
| Pointer and keyboard | `element.move/click/dragTo/scroll/press/typeText` | click button/count, horizontal/vertical scroll, chords and multiline text are explicit arguments; no unknown target |
| Focus, shortcuts, guarded sequence | `dtw.transaction(tx => ...)` | focus binding, before/after guards, short foreground lease and cleanup are one native plan |
| Grants, user seat, virtual cursor | `dtw.grants()`/`revokeGrant()` for Session authority view; no `setMode` | trusted host grants; seat scheduler and virtual cursor derive from proven delivery, never grant new input authority |
| Execution, cancellation, late/unknown receipt | one MCP `exec` with `operation:exec/status/result/cancel` | original ID is the only recovery key visible by default; no replay |

## JavaScript surface and direct input/output correspondence

The common exact-target path should fit in a small batch without copying a native schema. These proposed methods fail on incomplete coverage or ambiguous identity instead of selecting the first match; read scope and each later action still pass the existing authorization checks:

```javascript
const app = await dtw.app('Obsidian');
const win = await app.window('欢迎 - DevNote - Obsidian 1.13.7');
const note = await win.one({name:'新建笔记'});
print(note);
```

`app.window` uses exact App/window identity; `win.one` means **exactly one control in clean, complete declared scope**. It should build a targeted native query with a bounded budget, not fetch a desktop tree and filter it in the model. `win.find({...})` returns a bounded collection plus coverage for exploration, and `win.outline({...})` returns a scoped tree only when broader context is needed. Neither turns partial zero matches into absence. An unrelated desktop page cannot invalidate a fully proven exact App/window target; missing AX-to-window identity remains unresolved rather than being patched with a CG title or implicit activation. `print(note)` uses a bounded DTW text renderer, not `JSON.stringify` of an AX object. Full typed objects remain available to the JS batch and persistent `state`.

The text line includes the exact address and behavior name that the next script accepts:

```text
e3 · Obsidian › 欢迎 - DevNote - Obsidian 1.13.7 [W1]
  W1.1 新建笔记 · container · invoke, scrollIntoView
```

```javascript
const note = dtw.at('W1.1');
await note.invoke();
```

The visible `W1.1` is passed **unchanged** to `dtw.at`; `invoke` is both the advertised behavior and the method called. No model-side conversion to Ref, role prefix, snapshot token or native action JSON is required. Within one `exec`, scripts use the `note` object directly and need no address round-trip. `state.note = note` is permitted across calls, but every method still checks Session, native epoch, current instance and grant; a stale handle fails rather than rebinds. `W1.1` remains a candidate notation; the exact characters can change only if this invariant survives.

Each method returns a typed JS result for branches, loops and `await`; the MCP reply renders only selected facts and one authoritative effect line. For example, `note.invoke()` yields `e4 · W1.1.invoke: delivered; effect unverified` until an independent readback proves the note appeared. The script need not print its receipt again. `await win.capture()` retains an image asset internally and prints only a short ready/status line unless the original result is queried with `include_image:true`.

For a stateful control, explicit desired state remains part of the script, and readback is another object method:

```javascript
const box = dtw.at('W1.2');
await box.setChecked(true);
print(await box.read('checked'));
```

The `read` result must retain `known`, `unknown`, `redacted` and `unsupported` distinctions inside JS; its short text shows the requested property/value only when safe. JS loops can filter a bounded `find` result before `print`, while exceptions stop a batch without inventing a successful action.

For keyboard/focus/drag sequences, independent awaited calls are unsafe because focus or user ownership can change between them. A synchronous builder submits one guarded native plan:

```javascript
await dtw.transaction(tx => {
  tx.focus(field);
  tx.press(field, 'primary+A');
  tx.typeText(field, 'example');
});
```

The result names the same target/method for each consequential step, including any skipped or unknown step. The transaction coordinates physical foreground use and input cleanup; it is **not** a rollback guarantee. Observe before it, read back afterward, and split plans only at a safe pre-effect boundary. The executor keeps user input priority and same-App contention rules; `tx` exposes no mode switch.

## One visible result, one internal record

- Public MCP surface remains **one tool named `exec`**. Its `exec`, `status`, `result` and `cancel` operations share one caller-chosen, short `execution_id`, such as `e3`. The ID must be known before dispatch so `status` and `cancel` work while JavaScript is stuck. Those control operations stay in the parent, outside the script child/queue. Reusing the same ID observes the original execution; changing its code under that ID is a conflict, never a replay. The server prints that same handle **once at the start** of every reply: `e3 · ...`. It never prints a native request ID or run ID in routine output.
- Normal model-facing result is **one `TextContent` block** of concise, readable text. `structuredContent` is absent; no `{"state":"completed"}` companion and no JSON-escaped `print` array. The MCP transport envelope itself remains JSON. An on-demand screenshot adds `ImageContent` to the same result only when requested.
- The full typed observation, AX/native references, coverage flags, sampling times, metrics, grant/dispatch facts, original native request/run IDs and receipts remain in the Session's execution record. `exec(operation:"result", execution_id:"e3")` gives a compact human-readable summary; an explicit `detail:"full"` view exposes the original record as readable text when diagnosis/recovery requires it. The default response never silently discards these facts. All detail/status/cancel queries use **the same** MCP tool.
- A JavaScript batch can retain full objects in its Session-local `state` and use them for filtering and action checks. `print(DTWHandle)` invokes one bounded text renderer; ordinary `print(string)` remains available for task facts. Script authors should not `JSON.stringify` whole native objects into `print`. The executor adds each native action/capture outcome once, without making the script echo a receipt. Rendering adds no public tool or authority.

The [MCP tool-result specification](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) allows unstructured `content` without `structuredContent`; the [official Go SDK server guide](https://github.com/modelcontextprotocol/go-sdk/blob/main/docs/server.md#tool-result-content) shows an `AddTool` handler returning content with a nil structured output. The installed v1.8.0 POC currently returns a non-nil `map[string]any`, which is why the SDK fills `structuredContent`; simply returning a typed nil map is not a proven fix. A future text-only trial would use an optional `any` output with nil and manually supplied `Content`, then check the actual wire/adapter. This is a feasible protocol direction, **not** an implementation or adapter acceptance result.

## Model text grammar

The first token of a reply is the caller's execution handle. `·` separates that handle from task facts. A line's address is exactly the address accepted by later `dtw` JavaScript; the model need not translate a display number to another Ref. App and window context appear once on the first relevant disclosure or when context changes. Indentation describes only trustworthy native parent/child structure. Region hints such as top/side/main are optional navigation hints and must not be invented from an ambiguous tree.

Within that Session and window, a second unchanged read is:

```text
e4 · W1.1 unchanged
```

A broad outline may resemble the supplied CUA example, with only useful hierarchy and visible controls. An exact-name query should normally return **one row**, not its ancestors, duplicate title matches or the whole tree. Role/name/value/action words remain literal and readable. Show only actions proven both supported and currently available; an omitted action cannot be guessed. Protected values say `redacted`, unknown values say `unknown`. If no selected facts or native actions were produced, return `e5 · ok`.

`W1.1` is a **candidate** short window-qualified address; `W1/R1`, opaque `E2` and CUA-style snapshot numbers remain candidates. The format is less important than one visible address matching the JavaScript target exactly. Internally it resolves to the Session's native instance and grant. It must never rebind on the same title, label, role, position, window number or reused PID. A replaced App/window or changed native epoch retires addresses; a fresh clean disclosure is needed for new addresses. A partial/dirty read cannot create an action-eligible address. An old proven address still requires a fresh per-action native identity and grant check or returns `needs refresh`.

## Status and uncertainty are facts, not routine flags

| Internal fact | Model-facing text | Required decision |
| --- | --- | --- |
| Complete clean observation | No flag line | Findings and absence within the stated scope may be used |
| `dirty`: sample is not a trustworthy complete/fresh view | `e6 · ? observation uncertain; refresh or narrow` | No absence claim or new action target from that read |
| `truncated`/`more`: cap or continuation | `e6 · ? incomplete: 48 shown, more; narrow or continue` | No absence claim; keep cursor privately or expose one short continuation handle if a next call needs it |
| AX/source unavailable | `e6 · ? AX unavailable; target unresolved` | Do not replace the AX authority with a CG title/geometry guess |
| Verified semantic step | `e7 · W1.1.scrollIntoView: verified` | Only the specified semantic effect is verified; business effect still needs readback when relevant |
| Physical delivery without App readback | `e7 · W1.1.click: delivered; effect unverified` | Do not claim App state changed |
| Active user prevents borrow | `e7 · delayed: user active; no input sent` | Wait/reassess; never steal focus as a fallback |
| Partial or unknown delivery | `e8 · ! partial: 1 W1.1.focus verified; 2 W1.1.typeText delivery unknown. Query result(e8); do not repeat.` | Reconcile the **original** execution; never replay an uncertain step |
| Cancellation with pending native work | `e8 · cancelling; outcome pending. Query result(e8).` | A late original receipt remains possible |
| Grant expired/target replaced | `e8 · denied: target grant expired; no input sent` | Refresh authority through the existing grant boundary |
| Foreground restoration failed | `e8 · ! foreground not restored. Stop physical input; query result(e8).` | Do not hide a seat problem behind an action success |
| Script state lost | `e8 · ! script state lost; prior effects may remain. Query result(e8).` | No silent state recreation or action replay |

`?` denotes incomplete **observation**; `!` denotes uncertain/unsafe **effect or seat**. These are text cues, not substitutes for actual words. Dirty can arise from an unstable/provider read, cached fallback or continuation semantics; it does not prove that the user changed the UI. Truncation means the scan hit a bound; continuation means more data may be read. The model need not see repeated `dirty:false`, `truncated:false`, `complete:true`, `more:false` or timestamps on clean results. If a read is incomplete, the text must say **why and what can be done next**. Zero matches with incomplete coverage are unknown, not absent.

For a batch with multiple effects, show one short line per consequential step with the **same target and method names the script used**; an optional script step label or ordinal distinguishes repeated methods. The result header carries the one execution handle; native request/run IDs stay in the original receipt. A script `print` should supply task facts such as a selected label/value, while the server supplies exactly one action/capture outcome line. Do not print the same receipt twice.

## Explicit recovery and image views

```text
exec({operation:"status", execution_id:"e8"})
→ e8 · running

exec({operation:"result", execution_id:"e8"})
→ e8 · partial: 1 W1.1.focus verified; 2 W1.1.typeText delivery unknown. Original receipt retained; do not repeat.

exec({operation:"result", execution_id:"e8", detail:"full"})
→ e8 · original receipt
  native request: ...
  step 1: ...
  step 2: ...
  restoration: ...
```

The full view may be long because the Agent explicitly asked for diagnosis. It should be a readable text rendering of the original record, not JSON escaped inside a JSON string. `result(include_image:true)` adds the requested MCP image block and a short text caption; default status/result never transfers an image or a screenshot file path.

## Decision and acceptance before implementation

This draft favors **the object/behavior script surface plus text-only default MCP results** and full internal records. Before selecting it, test: (1) the official MCP client and actual model adapter both deliver `content.text` without a `structuredContent` fallback; (2) an Agent can reuse an address **unchanged** in the next script and can use a same-script object for a real action/readback; (3) semantic and guarded physical plans select routes automatically, with no unsupported/unknown replay; (4) two Sessions, window replacement, incomplete reads and revoked grants never rebind an address; (5) unknown/partial/cancelled/late receipts and user takeover remain visible and queryable by the same execution ID; (6) fixed task token accounting includes the Skill, JavaScript arguments and result body, with model success and safety checked alongside cost. The previous 4,978→650 result-byte comparison does not establish Token savings for this proposal.

No runtime code, product migration, Windows pass, push, PR or release follows from this draft.
