# Read fragmented native browser text

Use this workflow only for a document or container already observed inside the
user-authorized window. `read(documentRef)` reads that object's own value; it
does not concatenate descendants. Do not issue a read for every character.

Locate the current `document` with a bounded, window-scoped observation. Reject
ambiguous matches and incomplete discovery. Store its observed Ref as
`state.document`, then execute `scripts/read-fragmented-text.js` in the persistent
JavaScript session. The recipe observes that subtree once and consumes up to
eight stable output pages. It never focuses, scrolls, captures or expands scope.

Keep `role`, `parent` and `value_preview`. Reconstruct child lists in encounter
order, then traverse depth first. The native observation is breadth first;
concatenating its flat rows can reorder nested content. Keep every leaf's Ref.
Ignore container values when using their leaves, and never deduplicate by text:
two visible `Repeat` leaves are two distinct sources. Preserve spaces and Unicode.

Use AX heading/paragraph roles and top-level native container boundaries as
blocks. A container boundary is not proof of HTML paragraph semantics. When AX
omits boundaries, preserve the fragments and report the limitation instead of
inventing punctuation or breaks. Read only the selected document subtree, never
other tabs/apps to fill gaps. Hidden content absent from AX is not included.

The recipe retains redacted and unknown markers and reports `incomplete` when
traversal fails, pagination exceeds its bound, or a preview may be clipped. Native
previews cap at 384 UTF-16 units (383 at a surrogate boundary); a complete
observation does not prove complete leaf text. A fragment reaching that boundary
must remain `possibly_clipped`. If full text is required, explicitly read that
specific leaf with `limit_runes` and consume its text continuation, retaining its
Ref/source/version and a finite call/byte budget. Do not silently treat a clipped
preview as the whole paragraph.

Output bytes, traversal node allowance and read deadline are independent. Follow
only the original query's continuation. Preserve coverage and sample intervals;
`elapsed_ms` can be derived from `sample_end - sample_start`. If there is no
continuation and coverage is incomplete, report the partial sample. Increasing
output bytes cannot recover unvisited AX nodes. No screenshot or whole-desktop
fallback is implied by this workflow.

On macOS, a continuation may advance a retained native traversal after its
current result pages are exhausted. This recipe still stops after eight pages;
if that bound is reached, report incomplete text and use a narrower document
scope. For role/name target discovery on large browser pages, follow the
[large AX guide](large-ax-discovery.md).
