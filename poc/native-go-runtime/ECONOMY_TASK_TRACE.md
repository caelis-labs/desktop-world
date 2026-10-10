# Task-shaped DTW result economy experiment

2026-10-10. Isolated POC only, starting from source `7f63e60f9f20587e13d5f816682adc106814cfb3`. The selected target was the user's disposable Obsidian vault and its exact granted window `欢迎 - DevNote - Obsidian 1.13.7`. These rounds used the official Go MCP `exec` tool, actual QuickJS child and existing native helper. They did not read unrelated Apps, type, borrow the physical foreground, or create a note. The action was a verified semantic `scroll_into_view` on an already visible control, so it did not prove scroll movement. Earlier [selected-App evidence](evidence/real-app-obsidian-2ac7016.json) separately covers one note creation, screenshot effect and cleanup.

## Exact operation returns

The [baseline text trace](evidence/task-result-baseline-7f63e60.txt) and [compact text trace](evidence/task-result-compact-7f63e60.txt) save **every** model-visible `content.text` and `structuredContent` result for these nine `exec` operations. Both reached the same task outcome; separate live rounds may see slightly different trees. The [byte ledger](evidence/task-result-byte-comparison-7f63e60.tsv) can be recomputed with `compare_task_traces.py`. The unsuccessful [incomplete exact lookup](evidence/task-result-incomplete-7f63e60.txt) and [AX-Ref/capture-Ref mismatch](evidence/task-result-capture-mismatch-7f63e60.txt) remain saved, rather than disappearing from the narrative.

| Step | Real task | Compact `content.text` | Baseline → compact result bytes |
| --- | --- | --- | ---: |
| 01 | Confirm one active exact-window grant | `grant active` | 229 → 33 |
| 02 | Find the exact `kind=window` in the granted App | `W1 Obsidian "欢迎 - DevNote - Obsidian 1.13.7"` | 1,098 → 69 |
| 03 | Find the exact New Note control and usable actions | `W1/R1 container "新建笔记" invoke,scroll_into_view` | 957 → 75 |
| 04 | Repeat the scoped control observation | `W1/R1 unchanged` | 442 → 36 |
| 05 | Ensure the control is visible | `ensure-visible: completed, verified via semantic` | 698 → 69 |
| 06 | Independently reobserve the control | `W1/R1 unchanged` | 446 → 36 |
| 07a | Find the selected window's capture candidate | `capture target ready` | 369 → 41 |
| 07b | Capture its window content | `capture: 1 image(s) ready` | 284 → 46 |
| 08 | Diagnose a broad window outline | `48 nodes seen` plus `observation incomplete (truncated, more); execution_id: task-08-broad` | 455 → 245 |
| **Total** | Nine comparable result IDs | | **4,978 → 650** |

The metric is `len(UTF-8 content.text) + len(compact JSON structuredContent)` for matching calls, an 87% reduction **in this returned payload**. It omits MCP envelopes, tool arguments, the 4,750-byte experimental Skill, the JavaScript sent to `exec`, model tokenizer behavior and cache. It is not a measured Token or cost saving. The test driver's baseline scripts sometimes print wider facts than the compact scripts, so the result reflects a **task-shaped scripting and return-projection combination**, not a pure server-only A/B. A broader task mix and fixed tokenizer accounting would be needed to estimate cost.

## What was redundant

- The selected window's exact title matched four AX objects: one window plus container, menu item and document. The scoped script selected the unique window. Returning all four names would consume space without changing that decision.
- A clean result repeated `execution_id`, native request ID, `complete:true`, `dirty:false`, `truncated:false` and the script's facts across text and structured channels. The execution ID is already in the call arguments; full state stays queryable through `exec(operation:"result", execution_id:...)`.
- The control reported four capabilities; `focus` and `set_value` were unsupported/blocked. The task line includes only the supported, available `invoke` and `scroll_into_view`. Omission does **not** license an Agent to guess an action exists.
- The semantic action appeared in both native action summary and script `print`, including a long run ID. The compact script does not print a second receipt; the server emits one verified action line. A physical delivery without App effect would still require an independent readback.
- Unchanged observations print an ID plus `unchanged`, not the same role, name, capabilities, source and sampling metadata. The Session keeps the full observations and alias mapping; sampling times remain in original records, not routine model output.
- The broad scan stayed explicitly incomplete: `truncated`, `more` and original execution/native request IDs remain visible. A missing object in this scan is **unknown**, never absent. Unknown/partial/cancelled/error outputs retain recovery metadata, including retry class and original receipt access; a failed foreground restoration is not hidden.
- The current action AX Ref and visual capture Ref are distinct. Passing the AX window Ref directly to `window_content` returned `capability_unavailable`; the script had to resolve a `capture_windows` candidate. The model should work with one selected window concept while the script/native layer resolves each capability's authorized handle. It cannot silently treat the handles as equivalent.

## Model usability check

The first independent `gpt-6-luna` scratch task failed: it guessed the wrong shape for `dtw.grants`, then a structured-only short result left `content.text` at `state: completed`, which the model treated as lacking the grant facts. This disproved the assumption that fewer bytes alone meant usable output. The corrected [experimental Skill](SKILL_ECONOMY_POC.md) includes one working batch example; clean task facts now live in `content.text`, with routine `structuredContent` just `{"state":"completed"}`. On a second actual model task, it read that Skill, sent **one** `dtw_poc.exec` call, and correctly reported `W1/R1`, role `container`, and `invoke,scroll_into_view` from the exact selected Obsidian window. It sent no mutation or physical input. [Bounded model evidence](evidence/model-economy-7f63e60.txt) gives trace hashes and observed limitations.

The successful model's JavaScript argument was 1,833 characters. Whole-turn counts were 99,767 input / 72,448 cached / 546 output for the failed attempt, and 81,345 input / 66,560 cached / 794 output for the successful one. Different prompts, Skill content, calls and cache make these **noncomparable for a saving percentage**. The script and Skill may dominate a short answer; a future high-level `dtw` helper should be justified with task-level measurement before any product selection. This result proves one Agent can use the concise facts after concrete guidance, not that every Agent or task will.

The best mental model is a scoped desktop world: select an authorized App, one window, then just the interactable controls relevant to the current objective. A display alias is a local route to a native Ref; it is not the authority boundary or the main economy mechanism. Its exact format remains experimental. Stable mapping, invalidation on App/window/epoch change, and refusal on ambiguous or incomplete observations must be proven before product use.

## Verification and boundary

`DTW_POC_COMPACT_OUTPUT=1` enables this projection **only for `exec`** in the isolated POC. `status`, `result`, `cancel`, original native receipts and on-demand images remain under the same single MCP `exec` tool. An official MCP + QuickJS test injects unknown and partial responses only at the native boundary and checks the compact wire response, original run/request IDs and no automatic replay. Real provider uncertain delivery remains unverified. The full native POC gate and Windows runtime acceptance remain open; this experiment does not authorize product migration.
