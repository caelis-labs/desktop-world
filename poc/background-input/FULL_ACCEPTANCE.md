# Full-operation same-desktop POC and implementation

2026-10-04, macOS 27.0.1 arm64, a logged-in physical desktop with existing Accessibility, input and screen-capture permission. The user's chosen objective is brief foreground borrowing and less total desktop occupation; no VM or separate OS session.

## Conclusion

The graduated path is `dtw serve --input-mode cooperative`, available in ordinary builds. It executes a known short plan using actual shared keyboard/mouse events, then restores the user's previous window and input focus. Reads, independent-window capture and supported semantic actions remain on their existing background paths. Reasoning and network waits between calls do not retain the foreground.

AppKit, Google Chrome and a standard Electron BrowserWindow application passed each input scenario below in independent requests. System WebKit completed three order-note tasks during simulated foreground typing; all 69 user characters survived. TextEdit saved an owned plain-text document through window-targeted Cmd+A, Unicode/multiline typing and Cmd+S; independently read disk bytes matched exactly. The original AppKit PID-only probe still passes three rounds without foreground/key loss and stays experimental. The earlier WebKit PID/SkyLight/no-raise failures remain in [ACCEPTANCE.md](ACCEPTANCE.md).

The Electron fixture uses the normal [BrowserWindow application lifecycle](https://www.electronjs.org/docs/latest/tutorial/tutorial-first-app), with renderer accessibility enabled. It does not call a DOM/CDP automation interface, inject input handlers or implement target AX actions for the helper. AppKit uses standard text controls and an ordinary canvas that records delivered physical events. Browser form/canvas event handlers record business effects through a loopback HTTP endpoint. Every agent operation runs through persistent dtw. Captures contain only the owned test windows.

## Independent scenarios

| Scenario | Independent business/state evidence |
| --- | --- |
| Move / single / double / middle click | Target application records the corresponding physical event; click count/button are checked |
| Drag | Application records drag/drop; resulting horizontal displacement is checked |
| Vertical / horizontal wheel | Application records nonzero movement on the requested axis and zero on the other |
| Unicode and submit | Full `Full-中文-🙂` value verified through dtw and actual submit callback |
| Modifier/select/replace | Cmd+A, replace, Shift+Left and Backspace produce exact `replace` |
| Multiline | Exact `Line1\n中文🙂\nLine3` in actual NSTextView / browser textarea |
| Window-targeted Tab | AppKit text acquires the tab; browser focus moves to the canvas, recorded by a passive focus event listener |
| Context menu | Actual right click, unique bind of new menu item, invoke, business callback |
| Dialog | Actual dialog/sheet, unique bind, click/type `CONFIRMED-中文`, bind/confirm, actual business callback |
| Long drag / text | 501 ms drag and 257 UTF-16 units rejected before borrowing or input |
| Original request reconciliation | Reusing the exact ID/body returns the same Run; business effects are not repeated |
| Occluded-window capture | App-scoped capture directory, current capture-specific Ref, independent `window_content` PNG |
| User resumption | Exact cumulative user text between transactions in a separate controlled application |

AppKit additionally passes an interrupted 500 ms drag, lease expiry during an unsatisfied predicate, successful observation after expiry, and a simulated user selecting a third app. Late cancellation is reconciled against its original Run until cleanup completes; no replacement drag is sent. Expiry stops the wait and restores. The user's third-app choice remains foreground and accepts `THIRD-KEPT` instead of being forced back to the original user app.

## Time and cost

Measured values and source/binary fingerprints are in [the committed evidence](../../docs/evidence/cooperative-20261004/README.md). Foreground duration includes handoff and cleanup. The one-second input budget is not a hard real-time maximum; an in-flight provider call or restoration can run past it. Native dialog binding is scoped to the known window rather than scanning an entire application.

The schema catalog remains 157 bytes. Selected schemas are 5,428 bytes for click, 5,545 for text, and 6,497 for drag. There is no additional model-facing action or input-mode parameter. The trusted host selects the mode. Discovery starts with name/role, narrows to a window/control, then requests required fields/actions/capture explicitly. Individual plans use known steps and bounded unique binds. Screenshots are requested only in the separate capture scenario.

Costs count helper requests and received UTF-8 JSON bytes, including startup Hello, discovery, required checks, deliberate same-ID receipt reads, negative cases and capture metadata. PNG bytes are reported separately. Simulated human input and read-only discovery clients are separate. They are not LLM tokens, an agent success-rate measurement or a savings percentage.

## Reproduction

Use an idle authorized interactive macOS desktop; the runner simulates the human so unrelated input does not confound results. No permission prompt, existing user app termination or OS configuration change is performed. Only created processes/documents are cleaned up.

```sh
python3 poc/background-input/accept-full.py --provider appkit
python3 poc/background-input/accept-full.py --provider chrome
# Test-only official runtime; not a production SDK dependency.
npm install --prefix bin/dtw-electron-poc --no-save --no-audit --no-fund electron@44.5.1
node bin/dtw-electron-poc/node_modules/electron/install.js
python3 poc/background-input/accept-full.py --provider electron
python3 poc/background-input/accept.py --case webkit --mode cooperative --rounds 3
python3 poc/background-input/accept-editor.py --provider textedit
python3 poc/background-input/accept.py --case appkit --mode public_pid --rounds 3
./scripts/check.sh
GOWORK=off go test -race -tags dtw_background_poc ./internal/engine ./local ./cmd/dtw
```

`DTW_ACCEPT_HELPER=/absolute/path/bin/dtw` selects an existing normal helper for packaged-binary acceptance. Production cooperative runs use ordinary builds. Experimental modes still use the tagged helper. Full wire traces and desktop inventories stay in ignored local artifacts; committed evidence contains only controlled business results, selected receipts, costs, hashes and owned-window PNGs.

## Explicit limits

Private exact-window key-focus and WindowServer sampling SPI are dynamically probed; missing symbols/permissions refuse input. The default shared mode and `no_shared_input` ceiling are unchanged. Raw Points, cross-window drag and silent alternate-transport fallback are unavailable. Restoring failure produces unknown/fenced, and uncertain effects are never replayed automatically.

The isolated-profile VS Code launch did not expose the owned document (blank initial window/startup warning); its save scenario is **unaccepted**, rather than inferred from the standard Electron result. More applications, IME/composition, global shortcuts, minimum macOS 14, amd64, same-app human focus changes, lock/disconnect/Spaces and crash/kill recovery need their own evidence. The input detector is best effort, not full user-input isolation. Forced process death cannot promise restoration. Existing windows must be on the current visible desktop; minimized/hidden windows are not automatically resurrected.

Windows real-machine adaptation and availability claims are entirely deferred. Selecting cooperative on Windows explicitly fails. Bot M0 integration/version update remains deferred. Implementation details, authority boundaries and receipt fields are in [cooperative-input.md](../../docs/cooperative-input.md); third-party attribution is in [THIRD_PARTY_NOTICES](../../THIRD_PARTY_NOTICES.md).
