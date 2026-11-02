# Protocol release preparation

This execution follows the 2026-11-02 readiness review. Implement the changes as
one batch, then perform consolidated checks. Native boundary acceptance and
fixed-source development candidates follow code completion; no Harness A/B.

Implemented scope:

- Required-app error contract, two-tool package schema checks and retirement of
  the obsolete CLI/type declaration from public distribution.
- Current install examples and a separate human migration note, preserving old
  verification history as history.
- Deduplication of a directly returned view, meaningful output-budget guards and
  restoration of the provider type-check dependency manifest.
- Windows physical layout/z-order metadata and covered exact-HWND reads, with
  layered-alpha refusal and retained input ownership/visibility checks.
- Current apps/exec lifecycle/LSUIElement/helper/cancellation acceptance and a
  Windows baseline/modal runner. Both record protocol metrics and can bind an
  installed executable/Skill pair. Legacy diagnostics are clearly separated.

Consolidated verification will record actual results below. Until then these
are implementation statements, not passing GUI or candidate acceptance claims.

Remaining boundary work: real Chrome translation/menu surfaces, real app tasks,
Windows full build/GUI/DPI verification, installed clean/upgrade permissions,
consumer-host migration smoke, signing/notarization and fixed-source candidates.
Cross-display physical input and cross-app drag stay outside this increment.

## Consolidated checks

- Release-profile Rust: 495 unit + 7 integration = 502 passed.
- Required source check: passed. Original first run failed because the owned
  candidate fixture did not include newly shipped files; the fixture now mirrors
  those files and archive assertions check their exact bytes. Both logs remain.
- Original Windows and X11 provider type-check entry points: passed. No product
  cross-link, Windows GUI or Linux desktop claim.
- Pure Swift inventory/ownership/input controls: passed.
- New binary worker cleanup controls: passed.
- Actual staged-binary CLI/MCP/schema/required-app/EOF check: passed. The original
  error-code mismatch is resolved. Stage includes the migration note and all
  three canonical Skill files; it is development packaging evidence.

Logs: `/private/tmp/dtw-protocol-prep-{source-final,rust,win-types,linux-types,inventory,native-input,workers,packaged-mcp}.log`.
Development binary SHA-256:
`7a1238bf757a162292b5959f3d59f27954fa41c7eda0cec6a1fb60f5f3f1a5d7`.

Native GUI boundary/candidate results will be recorded separately after code
completion. The Windows runner is authored and type/build prerequisites remain
independent of running it on a real Windows desktop.

## Native boundary follow-up

Real Chrome translation-popup testing exposed a PID-directed click cleanup
failure. Image clicks now use the visible physical-pointer route on macOS, with
background-only refusing them before input. Semantic background presses remain.
The corrected binary passed 503 Rust tests and 38 native cases, and the actual
Chrome popup closed with continued semantic/pixel interaction and clean workers.
See [boundary evidence](protocol-boundary-acceptance.md) for original failures,
exact byte boundaries and remaining acceptance scope.
