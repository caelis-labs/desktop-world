# DTW `exec` model result contract — review draft

Status: **proposal only**, 2026-10-10. This document changes no runtime or interface. The full native POC gate remains closed. This draft responds to the [saved real task trace](ECONOMY_TASK_TRACE.md): its baseline repeated escaped JSON, request IDs and normal coverage flags; even the opt-in compact response still returned `structuredContent:{"state":"completed"}` on every clean call.

## One visible result, one internal record

- Public MCP surface remains **one tool named `exec`**. Its `exec`, `status`, `result` and `cancel` operations share one caller-chosen, short `execution_id`, such as `e3`. The ID must be known before dispatch so `status` and `cancel` work while JavaScript is stuck. Those control operations stay in the parent, outside the script child/queue. Reusing the same ID observes the original execution; changing its code under that ID is a conflict, never a replay. The server prints that same handle **once at the start** of every reply: `e3 · ...`. It never prints a native request ID or run ID in routine output.
- Normal model-facing result is **one `TextContent` block** of concise, readable text. `structuredContent` is absent; no `{"state":"completed"}` companion and no JSON-escaped `print` array. The MCP transport envelope itself remains JSON. An on-demand screenshot adds `ImageContent` to the same result only when requested.
- The full typed observation, AX/native references, coverage flags, sampling times, metrics, grant/dispatch facts, original native request/run IDs and receipts remain in the Session's execution record. `exec(operation:"result", execution_id:"e3")` gives a compact human-readable summary; an explicit `detail:"full"` view exposes the original record as readable text when diagnosis/recovery requires it. The default response never silently discards these facts. All detail/status/cancel queries use **the same** MCP tool.
- A JavaScript batch can retain full objects in its Session-local `state` and use them for filtering and action checks. The text renderer receives only selected facts. Script authors should not `JSON.stringify` whole native objects into `print`; a bounded native renderer must own observation/action status lines. A JS convenience method may produce text, but it does not add a public tool or authority.

The [MCP tool-result specification](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) allows unstructured `content` without `structuredContent`; the [official Go SDK server guide](https://github.com/modelcontextprotocol/go-sdk/blob/main/docs/server.md#tool-result-content) shows an `AddTool` handler returning content with a nil structured output. The installed v1.8.0 POC currently returns a non-nil `map[string]any`, which is why the SDK fills `structuredContent`; simply returning a typed nil map is not a proven fix. A future text-only trial would use an optional `any` output with nil and manually supplied `Content`, then check the actual wire/adapter. This is a feasible protocol direction, **not** an implementation or adapter acceptance result.

## Model text grammar

The first token of a reply is the caller's execution handle. `·` separates that handle from task facts. A line's address is exactly the address accepted by later `dtw` JavaScript; the model need not translate a display number to another Ref. App and window context appear once on the first relevant disclosure or when context changes. Indentation describes only trustworthy native parent/child structure. Region hints such as top/side/main are optional navigation hints and must not be invented from an ambiguous tree.

Clean, scoped control lookup (candidate format, not selected final alias scheme):

```text
e3 · Obsidian › 欢迎 [W1]
  W1.1 新建笔记 · container · invoke, scroll_into_view
```

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
| Verified semantic step | `e7 · ensure-visible: verified (semantic)` | Only the specified semantic effect is verified; business effect still needs readback when relevant |
| Physical delivery without App readback | `e7 · click sent; effect unverified` | Do not claim App state changed |
| Active user prevents borrow | `e7 · delayed: user active; no input sent` | Wait/reassess; never steal focus as a fallback |
| Partial or unknown delivery | `e8 · ! partial: step 1 verified; step 2 delivery unknown. Query result(e8); do not repeat.` | Reconcile the **original** execution; never replay an uncertain step |
| Cancellation with pending native work | `e8 · cancelling; outcome pending. Query result(e8).` | A late original receipt remains possible |
| Grant expired/target replaced | `e8 · denied: target grant expired; no input sent` | Refresh authority through the existing grant boundary |
| Foreground restoration failed | `e8 · ! foreground not restored. Stop physical input; query result(e8).` | Do not hide a seat problem behind an action success |
| Script state lost | `e8 · ! script state lost; prior effects may remain. Query result(e8).` | No silent state recreation or action replay |

`?` denotes incomplete **observation**; `!` denotes uncertain/unsafe **effect or seat**. These are text cues, not substitutes for actual words. Dirty can arise from an unstable/provider read, cached fallback or continuation semantics; it does not prove that the user changed the UI. Truncation means the scan hit a bound; continuation means more data may be read. The model need not see repeated `dirty:false`, `truncated:false`, `complete:true`, `more:false` or timestamps on clean results. If a read is incomplete, the text must say **why and what can be done next**. Zero matches with incomplete coverage are unknown, not absent.

For a batch with multiple effects, show one short line per consequential step, using the script's stable step names. The result header carries the one execution handle; native request/run IDs stay in the original receipt. A script `print` should supply task facts such as a selected label/value, while the server supplies exactly one action/capture outcome line. Do not print the same receipt twice.

## Explicit recovery and image views

```text
exec({operation:"status", execution_id:"e8"})
→ e8 · running

exec({operation:"result", execution_id:"e8"})
→ e8 · partial: step 1 verified; step 2 delivery unknown. Original receipt retained; do not repeat.

exec({operation:"result", execution_id:"e8", detail:"full"})
→ e8 · original receipt
  native request: ...
  step 1: ...
  step 2: ...
  restoration: ...
```

The full view may be long because the Agent explicitly asked for diagnosis. It should be a readable text rendering of the original record, not JSON escaped inside a JSON string. `result(include_image:true)` adds the requested MCP image block and a short text caption; default status/result never transfers an image or a screenshot file path.

## Decision and acceptance before implementation

This draft favors **text-only default MCP results** and full internal records. The following must be checked before selecting it: (1) exact official MCP client and actual model adapter both deliver `content.text` without a `structuredContent` fallback; (2) a model can use the displayed address for a real scoped action and independent readback; (3) two Sessions, window replacement, incomplete reads and revoked grants never rebind an address; (4) unknown/partial/cancelled/late receipts and user takeover remain visible and queryable by the same execution ID; (5) fixed task messages include Skill and JavaScript arguments in tokenizer/cost comparisons. The previous 4,978→650 result-byte comparison does not establish Token savings for this proposal.

No runtime code, product migration, Windows pass, push, PR or release follows from this draft.
