# Native browser authorization acceptance

Checked on macOS arm64, 2026-10-25, against the unmodified CUA 0.38.0 worker at
`85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`. This completes the profile/download
integration gaps in [the earlier repair checkpoint](cua-0380-repairs.md).

## Implementation and boundaries

- The host selects and explicitly approves a native capability manifest using
  `--capability-manifest PATH --approve-capability-manifest`. DTW freezes those
  bytes in its private runtime directory; CUA parses and enforces bounded mode,
  exact PID/window grants, origins, tools, file scope and expiry. No tool argument
  changes authorization. Standard startup has no existing-profile grant.
- The custom SCK app/click extensions are disabled for bounded runtimes because
  they cannot enforce CUA's manifest. Native browser/desktop routes remain subject
  to their native policy. There is no unrestricted fallback or automatic grant renewal.
- Downloads use [MCP form elicitation](https://modelcontextprotocol.io/specification/2025-06-18/client/elicitation).
  The pending tools/call asks for one exact target/tab/ref/destination, defaulting
  to false. Only accept plus boolean `approve:true` permits dispatch. The wait is
  bounded to 120 seconds; session admission stays occupied. Unsupported/URL-only
  hosts, decline, cancellation, malformed/error responses and timeout fail closed.
- CUA's Rust-only `call_tool_from_trusted_adapter` preserves trusted transport
  evidence in the official private worker. DTW strips incoming private fields,
  then supplies its own random session/transport identity and one-shot approval.
  Every native call uses this same configured authorization context. Mixing a
  delegated `CuaDriverSession` with the compatibility adapter was experimentally
  rejected with `session_ended`; the final implementation does not mix them.
- Native authorization, resource scope and live browser ref checks still run
  after host confirmation. Model-visible JSON cannot approve a download or profile.
  Host launch configuration and genuine client elicitation responses are trusted.

The launcher configuration and example native manifest are documented in the
packaged [browser guide](../packaging/plugin/skills/desktop-world/BROWSER.md).

## Real-machine results

`browser_authorization_probe.py` launched a disposable Chrome profile independently
of DTW before attachment, served a localhost page and used temporary download
directories. All browser commands then passed through DTW MCP and the official
worker. This did not read a personal profile or exercise account credentials.
Its fixture MCP host deliberately sent each decision; it verifies the actual
protocol and Chrome effects, not Codex/Claude/Cursor confirmation UI rendering.

| Case | Independent result |
| --- | --- |
| Existing profile with native grant | Exact PID/window attached to an already owned DevTools endpoint; native receipt reports no profile copy, browser launch/restart or preference changes. |
| Standard mode / unapproved launch | Profile rejected without grant; missing approval flag rejected before worker startup. |
| Wrong window / outside origin | Native refusal; no navigation to the denied site. |
| Replace original manifest after startup | Approved private snapshot still used; invalid replacement did not alter grants. |
| DTW SCK in bounded mode | Rejected before helper dispatch. |
| No elicitation / decline / cancel / false / host error | No HTTP download request and no file created. |
| Unrelated or replayed confirmation ID / forged private field | Rejected or ignored; no transferable approval. |
| Other action during confirmation | `runtime_busy`; exact request remains reserved. |
| Explicit accept | Exactly one HTTP download and one 35-byte file; SHA-256 `9fd842a5ce194aa5b9ac66e44bec9a22a9bbed395f76638d4d9dc66cbc696a76`. |
| Accepted download outside manifest directory | CUA refusal; no HTTP download or file. |
| Accepted stale ref | CUA refusal; no replay or second file. |
| Idle MCP EOF | Existing Chrome process remained alive, then the test cleaned up only its owned PID. |

The desktop-independent `authorization_probe.py` also exercises cancellation
during confirmation, URL-only clients, stale response replay, forged authority and
accepted confirmation followed by native invalid-capability rejection. It is wired
into macOS/Windows package and release checks; Windows GUI acceptance is not claimed.

Local raw evidence: `/private/tmp/dtw-authorization-final3-live` (final package; complete positive and
scope/ref refusal matrix), `/private/tmp/dtw-authorization-setup` (setup limitation),
and `/private/tmp/dtw-authorization-checks` (build and regression logs).

## Remaining issues and ownership

**Pinned CUA behavior:** a second owned Chrome profile launched without remote
debugging could not complete native setup. After valid manifest authorization,
CUA returned `browser_wrong_target_refused`: “the approved Google Chrome window
has no exact New Tab button”. The source is `platform-macos/src/browser/setup_ui.rs`,
which refuses when its exact AX control matcher fails. DTW preserves the refusal;
it does not guess coordinates or silently enable debugging. Therefore existing
profiles with already available endpoints are verified; automatic setup of this
machine's ordinary Chrome window is not reliable.

CUA's embedded `DriverAuthorizationHost` exists, but the released private-worker
protocol has no callback transport. Dynamic protected profile dialogs remain an
upstream transport/DTW integration gap. The completed profile channel is native
host-approved bounded startup, not an interactive profile consent callback, and
ordinary MCP elicitation is not substituted for that protected callback.

Other pinned upstream limitations remain as detailed in the repair report:
native parent/sheet composition, uncertain background gesture effects, synchronous
JavaScript prompt timeouts, picker AX discovery/Escape, Chromium-only typed browser
engines, and primary-display desktop capture. The SCK repairs address DTW's app
route; they do not claim to repair all upstream providers.

Host products still need to support MCP form elicitation and present a genuine
user decision. No cross-host confirmation UI acceptance, production install,
Windows/Linux GUI acceptance, release signing or notarization is claimed here.

## Candidate and regression provenance

Both final3 packages have 712 independently verified checksum entries and valid
local ad hoc signatures:

| Artifact | SHA-256 |
| --- | --- |
| `/private/tmp/dtw-cua-authorization-final3/bin/dtw` | `d704c1c87ea13c772931ab0aa936c2388719f263c32c0db0f8232a29bc287293` |
| `/private/tmp/dtw-cua-authorization-final3-slim/bin/dtw` | `1f073eb27f84ea480758f375a34d56c7bf1646adfd972a962a43dbdfa9e41cf2` |
| Packaged worker, both | `f1463aad7bda8f236003ecb1d8d20c7da7c2c2d4f72127bf7c258f37427f1014` |

The supervised native worker launch fixes its cleanup idle TTL at 1,800 seconds,
preserving the earlier delegated-session cleanup ceiling. This is CUA's native
`CUA_DRIVER_RS_SESSION_IDLE_TTL_SECS` maintenance setting, owned by DTW's launcher;
it is not a tool argument or a permission-mode override. The SDK general environment
allowlist excludes this setting, so it belongs at the actual supervised worker
launch. DTW's 25-minute absolute context cap precedes cleanup, including SCK-only
activity. Native manifest and individual authorization-grant expiry are independent.

The full browser form/gesture/upload/dialog regression and SCK scene regression
passed on `/private/tmp/dtw-cua-authorization-final-slim`; three isolated browsers
exited on idle EOF, and the independent standalone install probe passed. That
candidate predates only the explicit native cleanup TTL at worker launch. The
final3 candidates repeat the MCP/authority/lifecycle and real profile/download
matrix with this lifecycle setting in place. The prompt-trigger 20-second timeout
remains explicitly recorded as an upstream limitation, even though subsequent
dialog inspection and acceptance succeeds.

The 120-second unanswered confirmation path was measured at 120.002 seconds,
with a consent refusal and no native download dispatch, in
`/private/tmp/dtw-confirmation-timeout`. Source validation passed 13 Rust unit tests,
70 pinned-prelude checks, strict TypeScript authoring checks and eight Python unit
tests. These checks are distinct from the GUI fixture evidence above. A sandboxed
native-worker smoke failed to initialize macOS services; the same check passed
with normal host runtime access. No remote CI success is inferred from local tests.
