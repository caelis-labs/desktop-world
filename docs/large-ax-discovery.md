# Large AX discovery: bounded traversal with recovery

Issue [#6](https://github.com/caelis-labs/desktop-world/issues/6), based on
`cc50357` (#2/#3). A partial scan with no matches means **unknown**, never
"not present". The earlier `continuation` only paged selected results. This
change retains a helper-local native queue and visited set: after output pages
are exhausted, the same public continuation advances the native walk. The
caller must keep the original scope, projection, fields, match, depth and node
budget. The helper may reduce the presentation byte budget by a few bytes for
wire envelope growth; that does not invalidate continuation.

## Choice and rejected alternatives

| Approach | Assessment |
| --- | --- |
| [AXUIElementCopyMultipleAttributeValues](https://developer.apple.com/documentation/applicationservices/1462051-axuielementcopymultipleattribute) | **Used.** Common node attributes are fetched in one provider call, with the previous individual-read path when a provider does not implement batching. Value is read separately only after checking for a protected field. This reduces IPC work without changing redaction semantics. |
| Role/subtree frontier pruning | Kept only for known scope, depth, summary and detail boundaries. A nonmatching parent role can contain the requested child, so pruning it by role would hide valid targets. Match remains a result filter. |
| Persistent queue, visited set, cursor | **Used.** Opaque helper-local scan state survives output pages and calls for up to 90 seconds. A scan holds at most 10,000 retained native keys and 10,000 pending queue entries, at most 16 active cursors, and obeys each call's node and deadline budgets. `visited_nodes` is cumulative on resume. At a cap, `ax_scan_limit` is explicit. |
| Per-node messaging timeout | Existing 50 ms maximum remains. Increasing it or the 10 s public read deadline cannot escape a fixed-prefix walk; they are safety bounds, not discovery. |
| Two-stage child count, then details | A count can estimate breadth and choose a scope, but cannot identify a named target. Extra provider messages may cost more than batching; it is not the recovery path. |
| [CDP `Accessibility.queryAXTree`](https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/#method-queryAXTree) | Good browser-specific server-side role/name query. It requires a separate debugger connection and trust/authority design, so it is a recommended separately authorized browser path, not silently added to this OS helper. Returned ignored nodes need explicit handling. |
| [CDP `getFullAXTree`](https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/#method-getFullAXTree) | Useful for local diagnostics/counts, but a full page tree is too large for model output. The real Wikipedia page in this run had 35,247 CDP AX nodes. |
| [Playwright `ariaSnapshot`](https://playwright.dev/docs/aria-snapshots) / older `accessibility.snapshot({interestingOnly})` | A scoped ARIA text view is compact, but a whole-page snapshot still needs a byte limit. The older snapshot API and its `interestingOnly` heuristic are version-dependent; neither grants native Ref authority. |
| [WebDriver BiDi accessibility locator](https://www.w3.org/TR/webdriver-bidi/#command-browsingContext-locateNodes) | The draft supports role/name location within a browsing context. Its `getTree` is a browsing-context tree, not a replacement for native AX enumeration. A browser transport would need its own binding and authorization. |
| Compact ARIA text with a byte budget | **Used as presentation policy.** Ask for `role,name`, short names, and exact role/name matches. The helper compacts facts; `max_output_bytes` bounds each page and a native multi-call scan has a 24 KiB wire-output cap plus a terminal `ax_output_limit` marker. No full tree is returned to the model. |

The existing [fragmented-text recipe](../scripts/read-fragmented-text.js)
and [reference](../skills/desktop-world/references/fragmented-text.md) solve
text aggregation *after* a document Ref is observed. The earlier
[AX regression record](ax-regressions.md) established timeout diagnostics,
per-node messaging bounds and output pagination. Neither recipe previously
advanced an unfinished native walk.

## Semantics and safe use

Use the cheap path first: `seat.focused_object` and `seat.foreground_window`,
then detail reads of the focused Ref and its `parent` chain, then a narrow
window/document `match` with role and exact/contained name. For a large
browser page use `fields:["role","name"]`, `max_results:8`,
`max_text_runes:64`, `max_output_bytes:4096`, a finite visited-node budget
and `read_deadline_ms <= 10000`. Follow the same continuation for a bounded
number of calls; stop and report the limit on `ax_scan_limit`,
`ax_output_limit`, expiry or incomplete coverage with no continuation.
Use a separately authorized browser role/name locator when available. A
screenshot plus anchor is the last resort and is never automatic.

Native cursors are not durable snapshots. Resume marks coverage incomplete
because a live page can mutate between calls; a found Ref must be freshly
validated before action. A terminal zero-match partial read cannot prove
absence. The existing incomplete-sync path retains `Removed=nil`. Each
continuation still passes actor/turn scope, permission version, request
identity and Ref checks. `EndTurn` revokes old-turn use. No permission, input,
capture or release pin changed.
Each resumed page's sample interval describes that call, while `visited_nodes`
counts progress since the scan began. For sub-500 ms reads, the native driver
reserves 100 ms for encoding and authorization after a possible in-flight AX
message; the public deadline maximum is unchanged.

## Evidence and comparison

All numbers below are from the real GUI with the repository's managed helper
and native acceptance binary, except the explicitly marked deterministic
fixture. The Chrome work used new test windows/tabs; no screenshot or anchor
was requested. "Model bytes" is the UTF-8 byte length of the compact
`host.Content(...).Content[0].Text` field. Token counts are **rough**
byte/4 estimates, not a tokenizer measurement. Exact target names are kept
out of evidence; SHA-256 hashes identify equality across runs.

| Case | Calls | Cumulative visited | Hits | Model bytes (approx tokens) | Wall time | Outcome |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| `cc50357` Wikipedia exact role/name | 1 | 2,243 | 0 | 726 (~182) | 7.683 s | `ax_timeout`, no traversal continuation |
| Current Wikipedia, same exact target | 4 | 4,800 | 1 | 3,719 (~930) | 11.089 s | Found beyond the old prefix, no screenshot |
| Current Bilibili homepage exact role/name | 6 | 600 | 1 | 5,531 (~1,383) | 0.515 s | Real SPA path; first call 0 hits, continuation recovered it |
| Current Wikipedia broad `role=link` | 4 | 1,200 | 72 shown | 15,039 (~3,760) | 2.676 s | Explicit `ax_output_limit`; `complete:false`, no further output |
| Deterministic N=20,000, delayed node, unique control at 3,800 | 7 | 4,200 | 1 | 4,636 (~1,159) | 0.337 s | Cursor progress; subsequent scan stops at 10,000 with `ax_scan_limit` |

The broad survey before the cumulative cap required 44 output pages and
201,512 model-text bytes to reach 4,800 visited on Wikipedia. **That would
overfill a model context if surfaced as one agent workflow.** The cap now
ends this broad query after four small pages and reports its limit. Exact
role/name discovery reaches the same target with 3,719 bytes over four
calls. A page remains bounded by its requested 4,096 or 8,192 wire bytes;
the final broad test's largest model-text page was 4,773 bytes. The cap is
measured on typed wire output; compact model text may be smaller, and the
final limit marker adds a small terminal reply.

Raw sanitized logs and exact commands are under
[`evidence/ax-20261004`](evidence/ax-20261004/README.md). The baseline and
current runs used the same Wikipedia window and SHA-256 target. The public
site can change, so node rates and selected names are not stable benchmarks.

## Remaining limits

- Native provider keys and the engine registry both cap at 10,000. A target
  beyond that bound needs a narrower scope or an authorized browser locator.
- The private cursor expires with its helper and after 90 seconds. A page
  mutation between calls prevents absence proof; resume intentionally keeps
  coverage incomplete even when the retained queue drains.
- The internal native query still reads nonmatching nodes to discover their
  role and descendants. Batching and cursor progress lower cost and remove the
  fixed-prefix wall, but do not provide CDP-style server filtering.
- Native Windows desktop behavior and other browser engines were not exercised
  in this real-site run. Their compile and contract paths are covered by the
  repository checks.
