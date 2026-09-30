# macOS AX regressions and bounded recovery

This branch addresses upstream issues [#1](https://github.com/caelis-labs/desktop-world/issues/1),
[#2](https://github.com/caelis-labs/desktop-world/issues/2), and
[#3](https://github.com/caelis-labs/desktop-world/issues/3). Baseline was `5a2ae97`.
Tests ran on the connected Mac, using existing Accessibility/input permissions.
No permissions, safety settings, default browser profile, unrelated documents or
release pins were changed. Evidence below separates native E2E from fixtures and
Bot adapter validation.

## Scalar values and receipt recovery

Before the fix, a real AppKit checkbox preview was numeric `0`, while `ReadText`
returned the checkbox's label with `Source:label`. Both paths now use the same
NSString/NSNumber conversion. Value predicates require `Source:value`; labels,
unknown values and redaction cannot satisfy a value predicate. No input fallback
or receipt replay behavior changed.

`TestNativeValueFixture` validates real checkbox off/on and mixed AX `2`, slider
`37`, empty and nonempty Unicode field values, label-only controls and protected
fields. The mixed AppKit control starts at `.mixed` (AppKit raw state `-1`), which
its AX provider exposes as numeric `2`. Slider and mixed values are verified with
explicit value predicates. A string set-value verifies its full value.

On/off invokes each return delivery `complete`, verification `verified`. An
impossible checkbox predicate returns delivery `complete`, verification `not_met`
and the original `verification_timeout`; a label-only predicate remains unknown.
Original receipt lookup and identical request recovery add no input. Independent
app logs contain checkbox states `[1,0,1]` and exactly one label action. A full
string of 383 ASCII characters plus an emoji and suffix remains intact when read;
the preview stops before that emoji instead of splitting its UTF-16 surrogate.

## Timeout coverage and recovery

Native traversal has a separate query allowance within the caller's deadline.
Each native read uses the remaining allowance, capped at 50 ms, and traversal
stops checking attributes when it expires. The query leaves time for encoding,
registry commit and final authorization checks. Ordinary reads restore their
250 ms allowance; semantic writes retain their existing one-second allowance.
The query-local deadline/cancel state is cleared even on an Objective-C exception.

Partial output identifies `ax_timeout`, `ax_node_budget` or `ax_partial`, visited
nodes, declared scope and sample interval. Derive elapsed milliseconds from
`sample_end - sample_start`; no new public wire field is required. An outer query
error also retains diagnostics-only coverage with its fault. Incomplete changes
invalidate the view and require reset; absent nodes do **not** become removals.

`TestNativeObserveTimeoutFixture` uses 200 real AppKit text controls, each with a
fixed 25 ms accessibility value delay, through the managed helper and Go host
SDK. A 200 ms request returns incomplete `ax_timeout` coverage in about 158 ms,
with visited-node diagnostics preserved in the model projection. A narrow read
of an already observed fixture control then succeeds in the same helper. A native
8-node allowance visits eight nodes with either a 12,000 or 2,400-byte output
budget; the smaller output creates a stable continuation, not extra traversal.

EndTurn is tested independently: the ended turn cannot use old Refs (`turn_expired`),
and a new turn cannot write through the prior app grant (`permission_denied`).
Read authority for a fresh turn remains the existing helper policy; EndTurn does
not destroy otherwise live provider identities. No grant was carried forward.

## Fragmented Chrome text

The actual Chrome browser opened the fixed local
[`fragmented.html`](../tests/native-fixtures/browser/fragmented.html) fixture.
`TestNativeFragmentedTextFixture` discovers only its current native document and
reads that subtree with `role,parent,value_preview`. Two bounded stable pages
contain 47 visited objects and 41 text leaves. A depth-first reconstruction retains
Refs, sibling order, whitespace, nested Chinese/Arabic/emoji text, two distinct
visible `Repeat` sources, and four native blocks. Hidden and protected content is
excluded/redacted. Chrome exposes the fixture paragraphs as AXGroup with no
subrole; the recipe preserves native container boundaries rather than inventing
HTML paragraph semantics.

Compact native result bytes were 10,483 over two read pages versus 15,141 over
41 individual leaf reads. Window/document discovery is common to both. These
are result payload bytes, not token counts or full model/tool envelope bytes.
An initial broad window-field experiment used nine pages and 56,412 bytes; the
narrow document recipe avoids that overhead.

The reusable [`read-fragmented-text.js`](../scripts/read-fragmented-text.js) body
runs in the persistent JavaScript session after `state.document` is set to an
observed Ref. [Agent guidance](../skills/desktop-world/references/fragmented-text.md)
describes native order, source provenance, pagination and preview clipping.
Tests execute the exact recipe body with fixture transport, including redaction,
unknown values, possible preview clipping and incomplete native coverage. Complete
traversal does not imply complete text: previews hitting the 383/384 UTF-16 boundary
remain marked possibly clipped. Explicit bounded reads of those leaves are needed
for their full text.

## Commands and evidence

Use a new title/log suffix each run; the fixture app appends its independent log.
Native tests are opt-in, and ordinary `go test` skips real desktop input.

```sh
export GOWORK=off GOCACHE=/tmp/desktop-world-go-cache
go test -c -o bin/native-regressions.test ./tests/acceptance
go build -o bin/desktop-world ./cmd/desktop-world
DW_FIXTURE_TITLE='Desktop World Values unique' \
DW_FIXTURE_LOG="$PWD/artifacts/values-unique.jsonl" \
./script/build_and_run.sh --verify
DW_NATIVE_FIXTURE_TITLE='Desktop World Values unique' \
DW_NATIVE_FIXTURE_LOG="$PWD/artifacts/values-unique.jsonl" \
./bin/native-regressions.test -test.v -test.run '^TestNativeValueFixture$'

DW_FIXTURE_TITLE='Desktop World Slow AX unique' DW_FIXTURE_SLOW_COUNT=200 \
DW_FIXTURE_LOG="$PWD/artifacts/slow-unique.jsonl" \
./script/build_and_run.sh --verify
DW_NATIVE_FIXTURE_TITLE='Desktop World Slow AX unique' \
DW_NATIVE_HELPER_PATH="$PWD/bin/desktop-world" \
./bin/native-regressions.test -test.v -test.run '^TestNativeObserveTimeoutFixture$'

# Open the fixed local HTML in Chrome, then read only that fixture's document.
DW_FRAGMENTED_FIXTURE_TITLE='Desktop World Fragmented Text 20260930' \
./bin/native-regressions.test -test.v -test.run '^TestNativeFragmentedTextFixture$'
./scripts/check.sh
```

Sanitized evidence: [values](evidence/ax-20260930/values.txt),
[timeout](evidence/ax-20260930/timeout.txt),
[Chrome](evidence/ax-20260930/chrome.txt),
[existing native input/lifecycle](evidence/ax-20260930/native-general.txt),
and [Bot adapter checks](evidence/ax-20260930/bot-adapter.txt).
Full checks passed: Go race tests, vet, Windows cross-build/vet, protocol examples,
and 15 JavaScript regressions. Existing real native keyboard/pointer lifecycle
acceptance also passed without screenshot/cancellation opt-ins.

The Bot adapter was tested against a temporary local-source module override:
`go test -race ./internal/desktopcontrol` and the real child-helper read/refusal/
reconcile test passed. Bot's SDK and packaged payload remain pinned to alpha.1;
this branch does not claim that the shipped Bot/Wails app already consumes the
fix. No new tag, release, main merge or dependency hash was fabricated. Bot-facing
numeric workarounds remain applicable until the pinned payload is updated; the
upstream Desktop World skill is updated for the corrected behavior.

## Remaining limits

The original WPS Save panel was not rerun. Its application-specific behavior,
Windows real desktops and other browsers remain untested. The actual native
Chrome/API test passed; an optional fresh-session JavaScript recipe E2E was blocked
by automatic approval review at initial desktop-summary discovery. No retry or
workaround was used. CUA could not select the `file:` tab because its browser
policy allows only HTTP/HTTPS; no screenshot or alternate-surface workaround was
attempted. The owned idle JS helper was stopped. The local fixture browser tab
remains open for manual cleanup. These limits do not turn the JavaScript fixture
transport test into native E2E.
