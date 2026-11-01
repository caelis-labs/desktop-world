# macOS background input port

DTW ports a selected native dependency closure from CUA commit `b38360443960a6540685a9a5eb5407f10fe3d03b`. It does not link the CUA SDK, worker, driver registry, overlay renderer, browser stack, or the full `platform-macos` crate. The agent contract remains `exec(app, source)`; route selection is private to the worker.

## Exact source inventory

The hashes below are SHA-256 of the original downloaded bytes, before namespace rewrites, formatting, and the changes listed below.

| Pinned source | Original SHA-256 |
| --- | --- |
| [platform-macos/src/input/mouse.rs](https://github.com/trycua/cua/blob/b38360443960a6540685a9a5eb5407f10fe3d03b/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs) | `3ba45abcfe10d3ed01961b7f482362fb5628c07823c35bc995649007e3db9200` |
| [platform-macos/src/input/keyboard.rs](https://github.com/trycua/cua/blob/b38360443960a6540685a9a5eb5407f10fe3d03b/libs/cua-driver/rust/crates/platform-macos/src/input/keyboard.rs) | `5ed07b20a5f378c8429cd09b5a5cf608ddcd1a6a840888d23d37e30ca800514a` |
| [platform-macos/src/input/skylight.rs](https://github.com/trycua/cua/blob/b38360443960a6540685a9a5eb5407f10fe3d03b/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs) | `4f0e7f08ae2955b5e9dd5c2e1c4156f6b50d897297fbe76275c2891ed4775aaa` |
| [platform-macos/src/ax/bindings.rs](https://github.com/trycua/cua/blob/b38360443960a6540685a9a5eb5407f10fe3d03b/libs/cua-driver/rust/crates/platform-macos/src/ax/bindings.rs) | `00b68eaab97fdbef71402bc48e37941f84940e02facbb89362355aaeaa484df7` |

[Upstream LICENSE.md](https://github.com/trycua/cua/blob/b38360443960a6540685a9a5eb5407f10fe3d03b/LICENSE.md) has SHA-256 `c0779290c1d4783169aa3dbfb55feb505e563ef8a004bbf55298ceffcfbda8d9`. The original MIT notice is retained in `cmd/dtw/src/backends/macos/cua_input/LICENSE-CUA`. The ported files are Copyright (c) 2025 Cua AI, Inc.; DTW additions are covered by the repository license.

| Local module | Ported closure / DTW responsibility |
| --- | --- |
| `cua_input/mouse.rs` | Chromium move/off-screen primer/click route plan; window, PID, click-group and event-phase stamps; generic AppKit move/down/up construction, right click and anchored wheel construction. |
| `cua_input/keyboard.rs` | Unicode payload/control-key conversion; named-key codes and modifiers; PID key press/release construction and upstream 8 ms pacing. |
| `cua_input/skylight.rs` | Private symbol resolution, authenticated PID delivery, no-raise focus records, exact-window key priming, and key-window restoration. |
| `cua_input/ax.rs` | Narrow upstream AX focused-window/focused-element/window-list bindings, with retained previous-window evidence. This is not DTW's canonical target resolver. |
| `cua_input/control.rs` | DTW call admission, deadline/cancellation, per-event target validation, paired-release ownership, explicit cleanup reporting and quiescence. |
| `cua_input/mod.rs` | Private JSON/C ABI, key-array adaptation, preflight, route execution and autorelease lifetime. |

The native crate dependencies match CUA's `core-graphics 0.24` (`highsierra`, `elcapitan`), `core-foundation 0.10`, and `foreign-types 0.5`, plus `anyhow 1`. The adapter uses `serde 1` with derive; `libc` and `serde_json` already existed. No Objective-C wrapper crate or new framework build is required.

## Scalar validation boundary

Unicode typing retains the upstream 8 ms down/up pacing and one character per
pair. Before allocating each pair, DTW checks local cancellation, deadline and
prior errors only. Event allocation has no desktop side effect; every KeyDown
still performs complete target validation at its dispatch boundary, and each
paired KeyUp validates its original input owner. This removes a duplicate AX
traversal before allocation without caching target permission across characters,
extending deadlines or batching input. Pure gate tests cover target changes
between allocation and dispatch, plus releases after a target closes.

## Deliberate differences from the pinned source

1. Each input event uses one directed delivery selected before input: authenticated keyboard and Chromium pointer events use SkyLight, while the generic AppKit pointer recipe uses public PID delivery. The worker supplies a private app/framework classification; no agent parameter is added. The original scroll path's SkyLight + public PID duplicate and the legacy `Both` route are excluded. No imported background route calls HID posting, cursor warping, global key events, window raising, or front-process activation. The separate foreground adapter ports CUA’s exact-window front-process + make-key records only inside DTW’s existing physical-input lease, followed by AX raise and read-only exact-focus proof. The `defocus_front` branch is retained inside the imported function but the adapter always passes `false`.
2. Chromium target/move events keep the screen point in the main CGEvent location but stamp the supplied `window_x/y` into `CGEventSetWindowLocation`, matching the local-window convention already used by the AppKit click, right-click and wheel paths. The off-screen down/up primer retains `(-1,-1)` in both fields. This is a measured correction to the donor's screen-space window stamp: candidate F's capture point mapped to screen `(559.375,679.375)` and window `(329.375,559.375)` at origin `(230,120)`, while its independent DOM event landed at client `(559.375,592.375)` instead of `(329.375,472.375)` after Chrome's 87-point toolbar. The incorrect offset exactly matched the window origin; no save occurred and the failed attempt was not replayed. The original background wrapper always selected Chromium; DTW also adds a pre-dispatch framework branch so AppKit uses the existing generic construction and `PublicOnly` route. The coordinate regression tests preserve distinct screen/window spaces, paired events and negative screen origins; candidate G independently verified the corrected covered click and exactly one save.
3. Scroll input is DTW's existing wheel-step unit, not CUA's 120-unit tool input. Positive DTW deltas (right/down) are inverted for the native CG wheel convention, matching the existing Swift provider. Large requests are split into at most ten lines per native event without losing the remainder. The single-right-click donor declines public right-double clicks during preflight so the existing foreground implementation handles them. CRLF remains one Return, matching the existing DTW text contract. Keys stay in the existing `keys` array; the adapter preserves named-key aliases and modifier-only chords.
4. All writes and focus records validate the original target immediately before dispatch through the existing worker/Swift bridge. Pacing waits poll cancellation/deadlines without repeating AX traversal; the following post revalidates the target. No extra identity format is exposed. Background keyboard preparation must prove the exact target window; sibling windows are not accepted and there is no app-wide ambiguous-window escape. After priming, every keyboard down rechecks the current AX focused-window ID, so a user switching to a sibling window stops the remaining text/chord. Paired key releases ignore current focus and window closure while validating the original app incarnation. New downs and mouse events retain the full original-window checks. The local deadline/stop gate is checked again after blocking validation, including the exact-key-window AX query, before admitting a new post. Scroll events and mouse-moved primers explicitly clear inherited hardware modifier flags.
5. A retained previous AX window must be available before background preparation. Restoration uses that original AX object, checks its liveness and window ID before each restoration record, and reports failure if an owned restoration cannot validate its previous window. If the original app remains valid but the target is no longer key, or focus is already back at the previous window, cleanup preserves the current choice and skips restoration without poisoning future admission. It never picks an arbitrary replacement sibling. AX messaging is bounded to 150 ms per native call.
6. Before a key or mouse down is posted, an owned release event is reserved. Cancellation stops new normal events; the same call owner releases its posted prefix and restores the key window. Terminal cleanup ignores the caller deadline only within its shared two-second budget. Paired KeyUp events use app-incarnation validation so a window closing between down/up does not suppress release; mouse releases and any attempted key-window restoration still require the original process/window identity. A normal KeyUp has a separate bounded release allowance and does not start the terminal cleanup timer on the first character of a long text operation. Every release keeps its originally selected transport. Unverifiable cleanup is visible and fences new admission in that worker.
7. The upstream authentication selector check queried the instance method table of `SLSEventAuthenticationMessage`; the factory is a class method. DTW checks the metaclass. A read-only Objective-C symbol probe on this host returned instance=false / metaclass=true. A missing authentication capability declines keyboard background input during preflight; a later authentication construction failure stops without posting a plain event. A Rust autorelease pool owns factory messages for each call.
8. `SLSGetWindowOwner` returning exactly `1002` during the read-only connection preflight is retained as an invalid worker WindowServer connection. DTW returns unavailable immediately instead of masking it with `GetProcessForPID` and discovering it on the first focus post. Other statuses are not misclassified as `1002`.
9. Upstream window-change tail bookkeeping and cursor-overlay runtime integration remain excluded; DTW retains observation invalidation and instance identity ownership. A separate [cursor presentation port](cursor-overlay-port.md) now observes actual posts through narrow feedback hooks; it does not change the imported input recipes. Upstream best-effort `Drop` restoration is replaced with explicit checked cleanup, plus `Drop` as an unwind guard.

精确滚动沿用以上事件构造和 wheel-step 单位，只在输入适配边界增加截图锚点映射与首次 wheel 前的原 AX 对象复核；固定落点和失败/取消语义见[精确滚动验收](anchored-scroll-acceptance.md)。

## Menu shortcuts and Chromium text dispatch boundary

The authenticated PID route can deliver a `keydown` to Chromium without producing the intended text edit. Candidate E (`/private/tmp/dtw-background-followup-browser-e/web/state.json`) retained the original `seed-to-replace` after Cmd+A; it recorded a trusted `🐦` keydown but no corresponding `input`. BMP Chinese and `café` inserted, and Backspace worked. The resulting note was `seed-to-replace后台复核：已核对  café`. All four calls had posted successfully, so none was retried. This evidence identifies a route capability boundary, not malformed JSON or truncated UTF-16: the port and donor both construct the complete UTF-16 payload for each scalar.

The pinned donor's [keyboard implementation](https://github.com/trycua/cua/blob/b38360443960a6540685a9a5eb5407f10fe3d03b/libs/cua-driver/rust/crates/platform-macos/src/input/keyboard.rs) documents that authenticated PID delivery bypasses the AppKit path needed for NSMenu key equivalents. Its [menu-equivalent implementation](https://github.com/trycua/cua/blob/b38360443960a6540685a9a5eb5407f10fe3d03b/libs/cua-driver/rust/crates/platform-macos/src/tools/menu_equivalent.rs) likewise distinguishes posting a shortcut from executing its menu action; the donor's [higher-level hotkey route](https://github.com/trycua/cua/blob/b38360443960a6540685a9a5eb5407f10fe3d03b/libs/cua-driver/rust/crates/platform-macos/src/tools/hotkey.rs) excludes browsers from menu lookup. DTW does not infer that unauthenticated posting fixes Chromium or retry a posted command through it.

After the worker’s read-only canonical target check and before any key-window preparation or input, DTW declines Command-plus-printable-key chords, Return/Tab keypresses and text containing newline, carriage return or Tab for every application; specifically for Chromium, it also declines text containing supplementary Unicode scalars. The existing worker then selects one foreground/HID attempt with its ordinary exact-window proof and foreground return. The decision covers the entire text request, including any preceding BMP text; it cannot append a partial background prefix and replay it. Other plain/BMP text, modifier-only chords, Control/Option/Shift chords and named-key combinations keep their background eligibility. This is a private capability decision and adds no public delivery flag. The relevant pure tests assert the preflight result and that native preparation was never entered; GUI tests independently prove the final text and returned foreground ownership. Candidate I completed Claude’s business task but its later background TextEdit Cmd+W posted without closing; that failure is retained and extends the menu-equivalent preflight to AppKit applications too. Candidate J’s agy task independently read back concatenated TextEdit lines after a background multiline request; its later explicit-focus, per-line work repaired the document but failed the usability gate. That original sample is retained. Control-bearing text now selects the whole foreground request before any prefix; the model need not split lines or add a delivery argument. Future background expansion requires the same application-effect evidence.

## Private worker contract

The preferred Rust call is `call(json, context, validate) -> serde_json::Value`. Equivalent C entry points are:

```c
char *dtw_cua_input_call(const char *json, void *context,
                       int32_t (*validate)(void *context, int32_t phase));
void dtw_cua_input_free(char *json);
bool dtw_cua_input_quiesce(uint32_t timeout_ms);
```

The callback/context remain alive until the synchronous call returns. Phase `0` validates the canonical app incarnation, window, cancellation and deadline, plus geometry only for coordinate/pointer input; keyboard input does not depend on the pointer or window position; phase `1` validates the same original target for mouse release/restore while ignoring normal cancellation. Phase `2` validates only the exact original app incarnation for paired key releases and the read-only ownership check before deciding whether restoration is still ours; it must reject process exit/reuse but does not require the old window to exist. Phases `1`/`2` obey the bounded cleanup/release allowance. They never authorize a new down or a replacement window. Returns are `1` valid, `0` stale, and `-1` cancelled/unavailable. The parent owns process-instance locking; this module admits one call per worker.

The private request contains `operation` (`click`, `scroll`, `type_text`, `keypress`), `pid`, `window_id`, `deadline_unix_ms`, and the operation's coordinates/count/button/deltas/text/keys. A private `chromium` framework flag is determined by the host before delivery. None of these implementation details adds an agent-visible delivery or request-ID parameter.

The response has `ok`, `dispatch`, `fallback_allowed`, optional `error`, and optional `cleanup_error`. `not_dispatched + fallback_allowed:true` is restricted to a known missing background precondition before any preparation/input effect. Preparation failure is `unknown` even when no click/key has yet been sent. Any posted or unknown attempt must not be replayed through foreground. Successful OS posting remains **unverified application effect**; independent observation is still required.

`quiesce` permanently closes admission, requests cancellation and waits for the actual call owner. It does not inject releases from a racing watchdog thread. It returns false on timeout or cleanup failure and never equates a cancellation request with completion.

## Foreground boundary

Background app-local key/AX focus does not prove a physical raise. The selected foreground route performs its own single raise/activation, then checks focus. Before the first physical click, the existing point proof may settle for at most 750 ms within the action deadline; app/window identity and geometry changes, cancellation and expiry still stop immediately. No pointer/down is sent during this wait, and the second click retains its strict single check. The combined background-to-foreground cases passed on candidate L; those bounded results and the earlier failed candidates remain separate in the [stage report](background-input-acceptance.md).

## Foreground coordination

Directed input and semantic AX writes take a shared cross-process coordination lease before their app-instance lease. A physical foreground operation takes that coordination lease exclusively, then the instance lease and physical-input lease. Independent background mutations can overlap. A side-effect-free background decline releases its shared/instance leases before acquiring exclusive coordination; the exact native target is revalidated after that handover.

Foreground fallback and explicit focus now leave the selected window raised. Script completion and cancellation release owned input resources and activation observers, but do not read or raise a previous application/window. No script-wide lock is retained. External focus changes invalidate the current activation scope; subsequent operations prefer background delivery or establish fresh target focus when required. This replaces the automatic return policy exercised in historical acceptance records.

The original action receipt stays independent of cleanup. `retained` means the cleanup did not restore another app, not independent evidence that the target is still frontmost. `changed_externally` preserves an observed external focus choice; unobserved cleanup remains `unconfirmed`. Held input release and pointer cleanup are still bounded, and their failure remains a shutdown error.

## Evidence and acceptance boundary

Compilation and focused pure tests cover original-key compatibility, Chromium screen/window coordinate separation and retained event pairs, exact-window priming proof, preflight-only fallback, explicit cleanup errors, post-validation deadline expiry, app-only paired-key release gating, lost-key-ownership restoration decisions, and waiting for the actual owner during quiescence. These tests do not claim GUI delivery or OS compatibility.

Real acceptance must exercise the signed, standard local stdio product route: background Chromium editing and scrolling, AppKit editing, multiple windows of one process, unrelated foreground activity, app/window closure, cancellation/worker exit, and foreground retention. Pointer movement alone is observation, not failure; keyboard tests do not require desktop-wide inactivity. Space changes, interference during a modifier chord and forced input-release failures remain separate coverage gates. Record the selected route, independent app result, foreground app, pointer position and cleanup outcome. A foreground fallback case should separately show that its preflight was side-effect-free and the target remained foreground after cleanup.

SkyLight authentication, focus records, `_AXUIElementGetWindow`, and opaque CGEvent record offsets are private SPI. Symbol availability does not prove event acceptance; OS releases and app frameworks can reject or reinterpret them. Locked-session use, native menu/dialog traversal, overlay ownership, and Windows WGC are separate acceptance groups. This port does not claim them complete.

最终 L 同候选验证通过后台核心、两外部 Agent 任务及完整清理。抬升后仅对已从完整父库存消失的非目标 AX 对象做有界只读重验；原目标失效仍停止。历史失败、纯控制与未自然触发恢复分支的边界见[验收报告](background-input-acceptance.md)。
