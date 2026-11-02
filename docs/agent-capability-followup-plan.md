# Capability follow-up (2026-11-02)

Continue from `cbb9e07`; retain only `apps` and mandatory-app `exec` tools, ordinary
JavaScript and runtime-owned scene state. No model/Harness A/B or publication.

Status: local implementation and bounded acceptance completed. The
[acceptance report](agent-capability-followup-acceptance.md) records the 37 native
cases and the unsupported spanning-window physical-input boundary.

1. Add atomic `move({x,y})` and `drag({path:[{x,y},...]})` methods. Coordinates
   refer to the current app image. Validate the entire bounded path before input;
   initially support content gestures within one proven window. Native macOS
   input rechecks identity, focus, geometry and visibility and releases every
   admitted press on success, failure, cancellation and timeout. No input replay.
2. Extend independent native acceptance with hover/drag event readback, invalid
   paths, stale frames, changed/occluded surfaces, policy refusal and cleanup.
3. Exercise real system Open/Save panels on a disposable fixture, including
   helper-hosted surfaces and keyboard/coordinate fallback. Fix proved gaps;
   preserve authorization proofs and unsupported/partial coverage.
4. Inspect actual display availability; exercise available display geometries
   through the public protocol, including stale mappings after relocation. Add
   checks for geometry cases unavailable on the attached hardware and report
   those separately from native GUI acceptance.
5. Update Skill API, active capability documentation and evidence with explicit
   limits. Cross-window/app drag, held input APIs, clipboard and broad platform
   parity are not implied by this increment.
