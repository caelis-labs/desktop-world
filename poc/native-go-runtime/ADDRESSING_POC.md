# Short AX addresses in the isolated POC

Status: tested macOS alias-routing POC slice, **not** a selected addressing design, product contract or full POC gate pass. The sole public MCP tool remains `exec`; scripts and the CLI namespace remain `dtw`.

## What the current POC proves

The current script can use `W1/B2` after one scoped observation, and need not print or write a raw native `Ref` for that action. This establishes feasibility, not that role-coded aliases are the best interface. In particular, the current first-observation counters depend on which query ran first, a role-coded alias becomes misleading if the same element's role changes, and the POC has no explicit per-window snapshot invalidation. It has not demonstrated a model-token saving over a shorter native Ref.

A raw native `Ref` **cannot** be removed from execution identity merely by replacing it with `{app name, window number, role number}`: names collide, AX roles and tree order change, controls with identical labels coexist, windows can be replaced with the same title, and PIDs can be reused. A display address is a Session-local lookup key, not proof of authorization.

The internal action key is bound to the Session and native epoch, an observed AX element Ref and lifecycle, plus the existing grant and fresh per-action exact-window check: PID, process start, native window ID and AXWindow identity. The original execution ID and receipt remain the recovery keys. The POC helper keeps these native facts out of the normal printed output. `dtw.ref(id)` remains available for legacy POC scripts, but the alias action path takes `target:{id:'W1/B2'}` and resolves it internally.

## Current experimental numbering

1. A new native observation epoch clears the Session's alias map. Each newly observed window receives a monotonic `W1`, `W2`, etc.; numbers are not recycled during that Session. The app display name is optional context and is not the authority key.
2. Within the window, a newly **indexed observed** object gets the next number for its role family: `B` button, `T` text/text field, `R` container, and `N` other. For example, `W1/B2` is the second indexed button-family object in that window in this Session. Exact role, accessible name, current value and capabilities travel beside the short ID; the prefix never substitutes for those facts. The implementation also accepts a native object without a direct window field when the one-window query scope proves its owner.
3. An existing Ref keeps its alias on later disclosures in the same epoch. An unchanged field is omitted; a new or changed field is disclosed under the same alias. Requested fields and item count are bounded. Volatile sampling timestamps are omitted from the presentation; the original observation retains them.
4. Only objects surfaced by the scoped observation get IDs. A filtered read does not renumber existing aliases. A structural or display-only node can have an ID for navigation, but an action still requires a supported native capability, a clean complete observation, and the native grant/identity check.
5. An alias emitted by a dirty, incomplete, truncated or continued observation is **not** action eligible. A later clean complete disclosure re-emits at least the ID before it becomes action eligible, even when its descriptive fields are unchanged. An undisclosed, expired, foreign-Session or mixed `{id,ref}` target is refused before native dispatch. Native stale, replacement, revocation and unknown-result handling remain authoritative after alias resolution. Zero matches with incomplete coverage remain unknown.

This is a POC's **first-observation numbering**, not a claim that macOS publishes a durable number for every AX node. The first lookup should choose a window, then a region or exact locator, then a small set of controls. Main/popup relation requires a real provider/window relationship; a geometry-only `area_hint` (`top`, `left`, `main`, etc.) is a navigation hint and never changes target identity. A future product snapshot policy should explicitly invalidate previously disclosed action aliases when its window snapshot is replaced; this POC currently relies on native stale-Ref and exact-window checks for that later boundary, so it does not claim Cua-equivalent snapshot invalidation.

## Addressing candidates before product selection

| Candidate | Model/script link | Advantage | Open problem |
| --- | --- | --- | --- |
| Short opaque Ref, e.g. `E2` | The observed row and later `target:{id:'E2'}` use the same identifier | Smallest rule set and no extra role-based counter | The ID alone does not show window ownership; a window header or explicit target scope is needed |
| Window-qualified untyped ID, e.g. `W1.E2` | A `W1` header groups rows `E2`; the script uses `target:{id:'W1.E2'}` | Cross-window intent stays visible while role/name can change without renaming the ID | Needs a Session-local window map and a clear retirement/refresh rule |
| Current role-coded ID, e.g. `W1/B2` | Role is hinted in the ID | Familiar at first glance | Role changes or provider misclassification can make it misleading; separate counters add state and possible token cost |
| Snapshot row number, e.g. Cua's `[2]` | Very short within one shown window snapshot | Simple to scan a compact tree | Must include or retain a snapshot binding; a reread can reindex it, so it is unsuitable as a durable cross-call key by itself |

The next POC comparison should favor **one stable visible address per observed native instance**, not a guessed semantic identity. `W1.E2` is the working candidate for that comparison, not an approved final choice. Window IDs would be monotonic within one Session and never reused. Element IDs would be monotonic within that window and never reused; role, label, value and region are adjacent facts, not parts of the identity. A changed fact yields `~E2 ...` under the `W1` header. Reordered, filtered or paged observations retain `E2` for the same native instance; a genuinely new instance gets a new ID even if its label and bounds match. The App name appears once as human context; it is not part of authorization or the action key.

Proposed reader-facing rules must stay short enough for the Skill:

1. A Session owns its IDs. Another Session, a restarted child, or a new native epoch cannot use them. The response must say `ids reset` when a reset is visible, without printing raw native IDs or timestamps.
2. Closing/replacing a window retires its `W` and all child `E` IDs, even with the same title or a reused OS window number. An element that disappears or is replaced retires its `E`; the same name, role or position never silently rebinds it. A role/label/value change on the **same** native instance keeps `E` and emits a delta.
3. An incomplete/dirty read cannot establish or refresh an action target. An already proven target may be used only if the action boundary can freshly validate its exact native identity and grant; otherwise return `needs_refresh` or the precise unknown state. After an action that could replace controls, the next action in that window requires fresh identity validation; an unknown delivery is reconciled by its original execution ID, never by retrying a short ID.
4. Window and element addresses are navigation handles, not grants. Per-action authorization still binds PID, process start, native window lifetime/ID and AX target. Unsupported or unknown capability cannot silently fall back to foreground input.

The presentation can use one concise window header plus rows, for example:

```text
W1 Obsidian · 欢迎
E2 button "新建笔记" · invoke
```

The same target would appear in script as `target:{id:'W1.E2'}`. A later unchanged read can say `W1 no change`; a changed fact can say `W1 ~E2 name:"新名称"`. Incomplete coverage must say why, for example `W1 partial: ax_output_limit`, even if no rows changed. This format is **proposed**, not implemented by the current JSON `dtw.disclose` POC. Full native observations, metrics and receipts remain under the original execution ID, including unknown/partial/cancelled states.

Selection requires a fixed real-App task comparing model-visible **text plus structured content** and the script needed for the next action, across the same observation/action sequence. Measure actual adapter content and tokenizer/model tokens where possible; report cache and whole-turn variation separately. A smaller UTF-8 payload alone does not prove fewer billable tokens. The POC must also test reread, pagination/filter reorder, role/label change, same-title window replacement, element replacement, Session/epoch reset, incomplete read, grant revoke, and original-receipt recovery. No product migration follows from the current one-control no-op.

## Cua source comparison

[`trycua/cua` at `f4a7f5ef2f90a9e2663ee6f949fb3482507a4f56`](https://github.com/trycua/cua/tree/f4a7f5ef2f90a9e2663ee6f949fb3482507a4f56) was read, not run or copied. Its [macOS AX tree](https://github.com/trycua/cua/blob/f4a7f5ef2f90a9e2663ee6f949fb3482507a4f56/libs/cua-driver/rust/crates/platform-macos/src/ax/tree.rs) assigns `element_index` in depth-first order to actionable nodes. Its [snapshot](https://github.com/trycua/cua/blob/f4a7f5ef2f90a9e2663ee6f949fb3482507a4f56/libs/cua-driver/rust/crates/platform-macos/src/ax/snapshot.rs) retains the corresponding native AX element. Its [token](https://github.com/trycua/cua/blob/f4a7f5ef2f90a9e2663ee6f949fb3482507a4f56/libs/cua-driver/rust/crates/cua-driver-core/src/element_token.rs) is `s<snapshot-id>:<index>`; the [window-state tool](https://github.com/trycua/cua/blob/f4a7f5ef2f90a9e2663ee6f949fb3482507a4f56/libs/cua-driver/rust/crates/platform-macos/src/tools/get_window_state.rs) renders compact indexed Markdown by default and invalidates old tokens on a new same-window snapshot. Thus Cua supports a short model-visible number, while preserving a snapshot/native binding underneath. It does not establish a permanent `{app name,W1,B2}` key.

## POC verification and limits

`TestDiscloseOnlyChangedFieldsPerSession` passed over two official Go MCP client Sessions: partial alias action refusal; one-window and two-window aliases; changed/additional-field disclosure; peer-Session and old-epoch refusal; exact `id` translation for observe/read/capture/action, including predicates and drag destination; mixed `{id,ref}` refusal. The exact selected Obsidian main window passed `TestSelectedRealAppExactWindowSemanticScroll` through MCP → QuickJS → native helper using only `target:{id:'W1/R1'}` for the semantic action. The original receipt reported `completed`, `semantic`, `verified`, `not_borrowed`; the selected control was already visible, so this proves action routing and a verified no-op, not actual scroll movement. Neither test promotes B01/B02/B04/B11 as a whole.

The current POC uses structured JSON for `dtw.disclose` instead of Cua's text tree. It suppresses unchanged facts across calls and removes Ref/time from printed nodes. A later [real task trace and actual model check](ECONOMY_TASK_TRACE.md) showed a text-first one-line `W1/R1` disclosure could be used by one Agent, and measured a smaller returned byte payload. It did not compare alias formats, billable tokens, or full task cost. A compact text renderer, deeper region classification and snapshot invalidation remain design/acceptance work, subject to the full native POC gate.
