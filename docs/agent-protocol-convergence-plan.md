# Agent protocol convergence — closed plan (2026-11-02)

Status: historical. This records the approved direction of the two-tool protocol
that shipped in `0.3.0` (2026-11-02). It is neither acceptance evidence nor a task
list; do not treat its items as pending work.

Current references: the installed [Skill API](../skills/desktop-world/API.md) is
the Agent contract, [architecture](architecture.md) records implementation
boundaries, [product matrix](product-matrix.md) records supported scope, and the
[acceptance snapshot](agent-protocol-convergence-acceptance.md) keeps the verified
behavior and the gaps observed at delivery.

## Delivered direction

- Exactly `apps` and `exec` are advertised; the script reference ships with the
  installed desktop-world Skill. The public `api`/`result` routes are retired.
- Every `exec` names one app authorization identity and JavaScript source. There is
  no app-less lane, Agent-owned Session, default-app switch or raw native escape.
- Launch/reuse and input readiness belong to execution. Normal quit keeps an
  unsaved-document dialog observable; it never forces termination.
- Main loop is look → input → look with a compact baseline plus changes, semantic
  targeting with image fallback, and internal window routing/receipts.
- Unknown effects are never replayed, cleanup is bounded, and scene state belongs
  to DTW rather than the Agent.

Standing exclusions from that plan remain valid: no real model/Harness A/B was
required, one provider's evidence never implies another's, and no token/latency
improvement is claimed without measurement.
