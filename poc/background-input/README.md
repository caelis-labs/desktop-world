# Same-desktop background input POC

This independent, opt-in macOS experiment tests an agent entering an order note
in a completely occluded application while a simulated user writes in the
foreground. Both applications run on the current user's ordinary desktop.

**Viable result: AppKit targeted clicks and Unicode text input.** This is a
per-provider result, not an independent OS input seat. WebKit's editable field
did not acquire focus under any of the three tested delivery probes. The
no-raise focus probe also disturbed the foreground application's key-window
state and is retained only as a diagnostic experiment.

## Run the acceptance

Existing Accessibility/input permissions are required. The runner never prompts
for permissions. It briefly opens two owned fixture applications, simulates
foreground typing, then closes only its own processes. Run on an idle desktop
to distinguish experimental effects from unrelated user actions.

```sh
python3 poc/background-input/accept.py --case appkit --mode public_pid --rounds 3
```

It writes `artifacts/background-poc-<run>/summary.json`, app-owned JSONL logs,
helper wire transcripts, metadata-only native timings, binary SHA-256 and a
source manifest. Wire transcripts can include desktop window titles; they stay
local and are excluded from Git. See [the acceptance report](ACCEPTANCE.md).

The following are **failure probes**, with nonzero exit expected from the
recorded WebKit runs:

```sh
python3 poc/background-input/accept.py --case webkit --mode public_pid --rounds 1
python3 poc/background-input/accept.py --case webkit --mode skylight --rounds 1
python3 poc/background-input/accept.py --case webkit --mode no_raise --rounds 1
```

## Use the experimental helper

```sh
GOWORK=off go build -tags dtw_background_poc -o bin/dtw-background-poc ./cmd/dtw
bin/dtw-background-poc serve \
  --experimental-background-input public_pid \
  --write-app-window 'EXACT LIVE WINDOW TITLE'
```

Keep this process alive and use the existing bounded `observe`, `read`, `act`,
and receipt protocol. The trusted host selects the transport; agent requests
cannot enable it. Ordinary builds do not expose the flag or experimental
constructor. A tagged binary without the flag also uses ordinary delivery.

| Operation | Experimental limit |
| --- | --- |
| `pointer.click` | Observed UI Ref/anchor, exact retained app/window, left button, count 1, currently on-screen window; occlusion allowed |
| `keyboard.type_text` | Observed editable target, exact current focus **inside its own application**, at most 256 UTF-16 units, no control characters |
| Semantic actions | Existing provider behavior and verification |
| Focus, absolute points, other pointer/keyboard operations | No automatic foreground or HID fallback |

Discover desktop summary with `name,role`, then a bounded name/role query within
the selected window. Request `states,bounds` only for the chosen target. Reuse
the same Ref for the click and text. Set `completion: verify` with a full-value
predicate on text input; put that same value in the submit step's `before`
predicate. Confirm the application's business result separately. Do not read
the entire tree or capture images on every action.

Existing CLI action schemas describe the production action shape. The table
above supplies this experimental host mode's additional limits; its text target
needs app-local focus rather than foreground desktop focus. Receipts use
`targeted_input_poc` for these two actions. `delivery: complete` means events
were posted; verification and business callbacks establish their effects.

The existing write grants, protected-object checks, stale native identities,
topology checks, pre-dispatch authorization, stable request IDs and uncertain
delivery fencing still apply. `no_shared_input` remains a stricter ceiling and
rejects targeted input before the first step of a mixed plan. This POC does not
claim that provider callbacks cannot activate an application.

## What the probes do

`public_pid` posts once through `CGEventPostToPid`; it stamps retained window
identities and private window-local coordinates without moving the shared
cursor or posting to the HID event tap. `skylight` selects `SLEventPostToPid`
instead, with no dual posting or replay. Symbol lookup failures refuse input.

`no_raise` tests a short `SLPSPostEventRecordTo` key-focus lease around a click,
followed by reverse focus records. It never holds the lease across model
reasoning or verification. It does **not** call a front-process activation API.
Missing preflight symbols return no delivery; unresolved focus after a started
lease returns unknown/fenced. A successful record-post return is not proof that
the user's key-window state recovered, as the failed native test demonstrates.

No independent login session, VM, hidden desktop, display driver, daemon,
Chromium authentication-record pointer parsing, or Bot integration is included.
Windows implementation and acceptance are outside this macOS POC.

## Next useful scope

Keep the accepted AppKit route behind the experiment gate. Before productizing,
test real applications (including text views, menus and dialogs), cancellation,
concurrent user clicks, IME/composition and provider-induced activation. The
WebKit failure requires its own focus/activation investigation; Chromium's
authentication and canvas event handling require separate evidence. For apps
that need the foreground, the user's stated goal also permits an explicit,
short foreground lease with verified restoration, released before the agent
waits or reasons. That should be a separate acceptance scenario.

Research references, pinned to the source reviewed for this POC:

- [Cua mouse routing](https://github.com/trycua/cua/blob/ec4a15455f192a12dcc11d2d7f9dbc3c506f54d0/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs)
- [SkyLight symbols and focus records](https://github.com/trycua/cua/blob/ec4a15455f192a12dcc11d2d7f9dbc3c506f54d0/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs)
- [Third-party notices](THIRD_PARTY_NOTICES.md)
