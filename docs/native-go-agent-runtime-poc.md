# Native Go Agent runtime: target and gated POC

Date: 2026-10-10. Baseline: `c8e7579eb55cd002ad8189ced6574f9a80ccd62f` on `feat/native-go-agent-runtime-poc`. This document is the **pre-implementation gate**, not an acceptance report. A historical test, source inspection, fixture or cross-build is not a pass for this candidate.

## Product target and authority

`dtw` is one native Go desktop core, using the official `github.com/modelcontextprotocol/go-sdk/mcp` for stdio MCP and an embedded JavaScript engine for approved scripts. Distributed runtime has no Node/npm or embedded Node. One public MCP tool, `exec`, accepts a discriminated `operation` (`exec`, `status`, `result`, `cancel`) and a stable `execution_id`; `exec` also accepts `code`. The CLI is `dtw`; the script global is `dtw`, alongside persistent `state` and `print`. The Skill describes this exact surface. Status, full result and cancellation are available through the same tool while execution is blocked. Trusted owner authorization stays outside model parameters and OS approval is never bypassed.

One MCP connection owns one Session. Its Go supervisor handles MCP/control independently of a same-binary Go script subprocess that hosts QuickJS and preserves its JS context across calls. The native helper may remain a separate process for provider isolation. Killing a wedged script child loses that Session's JS state and must report `state_lost`; it does not imply that an in-flight native action was undone. Keep original request IDs/receipts until reconciliation. Multiple connections create separate Session state, refs, grants, records and cancellation. A narrow per-user coordination primitive shared by these processes serializes physical foreground access and same-app/window writes; unrelated background reads and independent-app semantic actions can proceed. This is coordination, not a new daemon or multi-tenant service. User input/switching wins over agent borrowing. Lock order and stale-owner recovery need explicit proof; a process-local mutex is insufficient.

Foreground acceptance requires the exact target App window to become the **actual key window**, not only the frontmost PID or an AX focused-window value. The execution layer must stop before posting if that handoff is not acknowledged. A quiet interval before borrowing is only one signal: actual user keyboard/pointer activity during the handoff must take precedence, and a user-chosen new foreground must not be overwritten by restoration. A dispatch-complete receipt is not proof that the App callback or business result happened. If user input may reach the agent target during a borrow, the gate stays failed until a tested input isolation/refusal path prevents it.

The execution layer classifies each **known step** before dispatch: semantic native action or independent window capture stays in background; verified, target-scoped input may use a provider-specific background route only after live acceptance for that app/action; otherwise a known key/mouse step takes a short cooperative foreground lease and restores. The script never chooses a transport or mode. Unsupported capability fails explicitly; a failed or unknown delivery never falls back or replays. Receipt records selected route, delivery, verification, input lease and restoration. Cross-app plans split only before any effect at a safe boundary; already dispatched plans preserve the original ID and outcome.

`exec` defaults to bounded printed output plus execution ID, state, coverage/incomplete flags, effect outcome/delivery/verification, errors and receipt lookup hint. Both `content` text and `structuredContent` use the **same compact facts**; neither silently includes the full tree/metrics/receipt. `result` by the original execution ID retrieves internal full detail with explicit bounded page/image requests. Unknown delivery, partial completion, cancellation, state loss and original native request IDs must be visible even when no `print` happened. Images use MCP `ImageContent` only on explicit demand; paths/metadata remain queryable. No token-saving claim follows from byte counts alone.

Keep the existing Go World/Actor, macOS AX/CGEvent/ScreenCaptureKit and Windows UIA/SendInput/Win32 capability, authorization, refs, native receipts, cooperative restoration, virtual cursor and reconciliation mechanisms wherever sound. Node MCP/JS/packaging assets are historical references, not runtime dependencies. Do not migrate or remove them until this entire POC gate passes.

## Engine and SDK choice gate

Candidate: `buke/quickjs-go` binding over vendored quickjs-ng, isolated in `poc/native-go-runtime/`. Its published README documents MIT, cgo macOS/Windows CI, async Go callbacks with `Context.Schedule`/`Await`, and requires one owner OS thread. It also calls itself pre-production and notes Windows MSYS2 toolchain requirements. QuickJS/quickjs-ng expose a runtime interrupt hook and pending-job execution; **the binding's safe interrupt path and cancellation after `await` are not established yet**. The POC must prove those details in an actual build and busy-loop test. If a binding cannot interrupt a loop reliably, the script subprocess watchdog must prove control responsiveness and state-loss semantics; no unkillable in-process JS may reach product code. Evaluate `fastschema/qjs` (QuickJS-in-Wasm, context timeout) if the cgo candidate fails, including async native bridge and Windows packaging. The older `quickjs-go/quickjs-go` is WIP and requires thread affinity; it is not selected by name alone. Do not treat README claims as a passing POC.

The official Go MCP SDK supports `mcp.NewServer`, `mcp.AddTool`, `mcp.StdioTransport`, context cancellation and `ImageContent`. Its `AddTool` fills `StructuredContent` **and**, unless explicitly set, repeats JSON in text `Content`; explicitly supply compact text to prevent this duplication. Pin an exact module version and verify the generated schema, concurrent handler responsiveness and cancelled request behavior. Check QuickJS/quickjs-ng and binding licenses/notices in the built artifact. Windows must actually compile and run on a Windows desktop before any Windows native acceptance claim.

## Complete baseline preservation matrix

Each row needs current-candidate evidence through `exec`/native `dtw`, including refusal/error paths. `P` means pending, **not passed**. Historical evidence is a fixture design input only.

| ID | Capability and hard edge | Current-candidate gate |
| --- | --- | --- |
| B01 | app/window/control discovery, narrow projection, locator/ref identity, no guessed absence on incomplete zero-match | P |
| B02 | bounded AX/UIA continuation, page/output/cursor limits, stale/dirty/unavailable coverage | P |
| B03 | value/text read, fragmented text, protected/redacted/unknown, field freshness | P |
| B04 | invoke, set_value, set_expanded true/false, set_checked true/false, set_selected true/false, scroll_into_view; supported/unsupported/provider-specific outcomes | P |
| B05 | target/focus binding; before/after predicate and dispatch versus verified task completion | P |
| B06 | independent `window_content` and visible-region PNG, current window identity, occlusion, coordinate transform, budgets, stale/hidden refusal | P |
| B07 | move, click buttons/count, drag/drop, horizontal/vertical wheel, Unicode/multiline, key chords, shortcut/focus; refusal before unsafe input | P |
| B08 | short cooperative foreground borrowing, restore/failure/user-superseded, input lease, held-key/button cleanup | P |
| B09 | semantic background and targeted-input POC separated by app/provider/action; no silent fallback after unsupported/unknown | P |
| B10 | virtual cursor overlay reflects proven pointer delivery only; expiry and no independent input authority | P |
| B11 | per-app grant/declaration, deferred/ambiguous/expired app identity, revoke/end-turn, OS permission denial | P |
| B12 | exact request ID dedupe/conflict, original Run get/cancel/reconcile, complete/partial/unknown delivery, late receipt and state loss | P |
| B13 | CLI/Skill/MCP names and single public tool; public schema excludes owner operations and transport selection | P |

## Core POC acceptance matrix and evidence rules

All `C` rows must pass on this macOS host before formal product implementation. A POC may add only isolated files under `poc/native-go-runtime/` plus this plan/evidence; no batch migration, deletion or release. Use owned fixture apps and existing OS authorization; avoid personal windows/accounts. Record candidate source SHA, module versions/checksums, build command, OS/arch, exact test commands, raw bounded receipts, app callback/business logs and screenshots only of owned windows.

| ID | Required actual proof | Gate |
| --- | --- | --- |
| C01 | Build/start official Go MCP stdio server and load matching Skill/JS batch through `exec`, with Node/npm absent from runtime PATH/package | P |
| C02 | JS async/await, loops/filter/catch, `print`, cross-call `state` and refs; asynchronous Go/native completion and on-demand MCP PNG | P |
| C03 | Infinite loop before/after `await`, slow native call and cancellation: same MCP connection answers status/cancel promptly; original receipt remains queryable; no effect replay | P |
| C04 | Full B01–B13 on real owned AppKit, WebKit/Chrome and Electron/other-provider apps as relevant; fixture tests do not substitute for these calls | P |
| C05 | Real cross-app automatic routing: background semantic while user app stays active, verified targeted input where supported, short foreground where required, unsupported/unknown refusal | P |
| C06 | Two simultaneous Agent/MCP Sessions: independent state/refs/grants/history; parallel background; physical foreground contention, same-app write conflict, user input precedence | P |
| C07 | Cancel one Session during contention without stopping other; revoke/expired grant and stale/ref-cross-session refusal; late/unknown receipt recovery | P |
| C08 | Compare actual model-received text and structured content for duplication, PNG on demand; fixed task model token accounting or clearly bounded alternative measurement | P |
| C09 | Windows conditions: at least inspect cgo compiler/linking and cross-build feasibility; Windows GUI/background/multi-Agent remain unpassed without a Windows machine | P |

If any C row is unpassed, stop dependent product implementation and report the exact blocker and smallest next experiment/fix. Historical macOS/Windows acceptance and `go test` may support a baseline but cannot be promoted to this candidate's pass. If Windows is unavailable, report C09 as platform-scope unresolved to the coordinating agent; do not invent Windows results. A final POC report must bind all claims to exact SHA and differentiate current live proof, fixture/CI proof and historical reference.

The current isolated candidate, exact test commands, owned-App callbacks, controlled foreground failure and still-open rows are recorded in the [POC results](../poc/native-go-runtime/RESULTS.md). Those checkpoints do not change this stage gate.

## Primary references

- Repo: `api.go`, `protocol/protocol.go`, `docs/features.md`, `docs/cooperative-input.md`, `docs/agent-plugin-host-contract.md`, `poc/background-input/FULL_ACCEPTANCE.md`, `skills/desktop-world/SKILL.md`.
- [Official Go MCP SDK quick start](https://github.com/modelcontextprotocol/go-sdk/blob/main/docs/quick_start.md), [server tool output semantics](https://github.com/modelcontextprotocol/go-sdk/blob/main/docs/server.md), [protocol cancellation](https://github.com/modelcontextprotocol/go-sdk/blob/main/docs/protocol.md).
- [buke/quickjs-go README](https://github.com/buke/quickjs-go), [QuickJS API interrupt/job test](https://github.com/quickjs-ng/quickjs/blob/master/api-test.c), [fastschema/qjs runtime](https://github.com/fastschema/qjs/blob/master/runtime.go).
