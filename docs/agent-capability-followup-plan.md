# Capability follow-up — closed plan (2026-11-02)

Status: historical. The scope below was implemented and accepted on macOS in the
`0.3.x` line; this page is not a task list. Verified cases, retained failures and
the unsupported boundaries are in the
[capability follow-up acceptance](agent-capability-followup-acceptance.md).
Current method arguments and limits are in the installed
[Skill API](../skills/desktop-world/API.md).

## Delivered scope

1. `move({x,y})` and `drag({path:[{x,y},...]})` over the current app image, with the
   whole bounded path validated before input and one proven window per gesture.
   Every admitted press is paired with a release on success, failure, cancellation
   and timeout; input is never replayed.
2. Native acceptance extended with hover/drag event readback, invalid paths, stale
   frames, changed/occluded surfaces, refusal and cleanup.
3. Real system Open/Save panels on a disposable fixture, including helper-hosted
   surfaces and keyboard/coordinate fallback, keeping the ownership proofs.
4. Display availability inspected and exercised through the public protocol,
   including stale mappings after relocation; geometries unavailable on the
   attached hardware reported separately from native GUI acceptance.
5. Skill API and capability documentation updated with explicit limits.

## Boundaries this increment did not close

Cross-window/app drag and drop, modifier/right-button drag, held input APIs,
clipboard, window geometry controls and platform parity beyond macOS remained
outside it. Provider support for the same gesture set therefore needs its own
acceptance rather than an inference from these macOS results.
