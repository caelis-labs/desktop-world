# Agent protocol convergence — working plan (2026-11-02)

Status: implemented locally on `codex/agent-protocol-convergence-20261102`.
This records the user's approved direction. The separate
[acceptance report](agent-protocol-convergence-acceptance.md) records verified
behavior and remaining gaps; the plan alone is not acceptance evidence.

## Public contract

- Advertise exactly `apps` and `exec`. Remove the public `api`/`result` routes;
  ship script reference documentation in the installed desktop-world Skill.
- `apps` searches installed and running user applications, including stopped
  applications, without reading window contents. Return canonical authorization
  identities, names and running status; bound output and preserve coverage.
- Every `exec` requires one explicit app authorization identity and JavaScript
  source. No app-less lane, Agent-owned Session, default-app switch or raw native
  escape. App identity is independent of a current process.
- Launch/reuse and input readiness belong inside execution. Respect the selected
  application and externally changed foreground. Normal quit remains available;
  an unsaved-document dialog is observable state, never forced termination.
- The main loop is look → input → look. Expose a small app-scoped script surface;
  keep window routing, observations, pagination, receipt bookkeeping and native
  lifecycle internal. Arbitrary JavaScript variables need not persist.

## Observation and input

- Observe app windows/layers and relevant controls as a compact first baseline,
  followed by changes. Explicitly reset after identity/context replacement.
- Screenshots represent the authorized app's stacked surfaces, preserving
  geometry and z-order. Include related surfaces only with ownership proof.
  Other applications remain outside authorization. Read eligibility and input
  readiness are separate; ordinary same-app overlaps must not reject a read.
- Image coordinates and input hit-testing must share the same scene mapping.
  Validate identity and geometry before delivery; never click through a popup.
- Support semantic targeting and screenshot/coordinate fallback. Unsupported or
  incomplete semantics must remain explicit and must not prove absence.
- Preserve no replay after unknown effects, cancellation and bounded cleanup.
  Runtime recovery must not require the Agent to manage Session objects.
- Default output contains task-relevant changes and essential effect/error
  evidence. `print`/return are optional bounded output; full diagnostics are not
  repeated in the main loop. Historical images must not become current images.

## Implementation sequence

1. Persist this plan, inspect the existing implementation and test boundaries.
2. Converge MCP admission/discovery and Skill reference; remove optional-app and
   advertised API/receipt branches, with real stdio contract coverage.
3. Add app-scoped default observation/input routing, automatic launch/readiness,
   compact changes and semantic/pixel fallback without public session/ref work.
4. Correct macOS stacked-surface capture and mapping; preserve authorization,
   stale-geometry checks and separation of read versus input conditions.
5. Exercise native boundaries and document platform limits and remaining gaps.

## Verification boundaries

Exercise malformed/missing app, duplicate RPC/cancellation, stopped app, an app
without a ready window, restart/PID replacement, same-app overlapping windows,
popovers/menus/sheets, related helper surfaces, unrelated-app occlusion,
incomplete/unavailable semantics, coordinate scaling, stale geometry, cleanup
failure, output truncation and accidental replay. Check actual stdio and native
paths where available; mocks and fixtures are supporting evidence only.

No real model/Harness A/B run is requested. Do not claim functional completeness
from one app or fixture, infer untested Windows behavior from macOS, or claim
token/latency improvements without measurements. No publishing or merging is
part of this implementation request.
