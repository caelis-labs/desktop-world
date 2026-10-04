# Semantic desired states

Start with name/role discovery in a known window. Request states/capabilities on
the selected Ref only if needed. Use `dw.check(ref, boolean)`,
`dw.select(ref, boolean)` or `dw.scrollIntoView(ref)`; low-level users can request
`dtw schema act set_checked`, `set_selected` or `scroll_into_view` individually.

All three verify their desired state, even with completion=dispatch. A verified
no-op means the provider was not called. Unknown delivery is never retried with
a new request ID. Check actual business callbacks/result after a submit.

Checked states are booleans; mixed/indeterminate stays unknown. A native setter
may handle it; a toggle is only allowed from a freshly known boolean state and
is sent at most once. Native errors never trigger keyboard/pointer fallback.

Selection writes only the requested item's state. Windows adds/removes through
SelectionItem and never invokes its replacement Select operation. Providers may
reject single-choice/required-selection constraints; macOS AXSelected follows the
provider's own selection rules, which may adjust other items. Verify those items
when their preservation matters to the user's task.

Scroll requires the provider's target-specific semantic capability. Verification
proves at least some target content intersects the native viewport; it does not
prove freedom from occlusion, physical hit testing or capture freshness. On macOS
only providers advertising AXScrollToVisible with verifiable viewport ancestry
are available. It is not a generic wheel operation.

The host's no_shared_input ceiling still applies. Unsupported means stop and
report the original receipt. Windows functionality exists but interactive
acceptance/adaptation is deferred; current Windows availability is not promised.
