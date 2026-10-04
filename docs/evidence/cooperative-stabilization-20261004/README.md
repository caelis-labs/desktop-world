# Packaged input handoff stabilization

2026-10-04. Alpha.5 packaged Chrome validation twice stopped before the first input event of a transaction, once at move and once at dialog open. Focus was acknowledged but the retained target was not yet returned by the native hit test. Both receipts report delivery none and restored foreground; no uncertain action was replayed. The alpha.5 release remains a draft and its pushed tag is preserved.

Fix: before pointer dispatch, allow up to 120 ms for the same retained target/ancestor to become hittable under the original one-second lease. Do not reactivate, change target, fall back or post input while waiting. Expiry/user interruption retain their faults; activation refuses an already expired deadline. Persistent mismatch still refuses input.

Validation uses the release-style compiler settings: `-trimpath`, `CGO_CFLAGS=-mmacosx-version-min=14.0`, the corresponding linker target, and ad-hoc signing. The physical host remains macOS 27.0.1 arm64; this is not minimum-version acceptance. AppKit and Electron full scenarios passed, and Chrome full scenarios passed repeatedly after the change (two complete runs of the final source/settings). Every scenario is an independent dtw request with app event/state evidence.

| Scenario | AppKit ms | Chrome ms | Electron ms |
| --- | ---: | ---: | ---: |
| move | 177 | 174 | 182 |
| single | 145 | 126 | 122 |
| double | 127 | 122 | 122 |
| middle | 133 | 128 | 124 |
| drag | 434 | 444 | 456 |
| vertical-scroll | 112 | 141 | 129 |
| horizontal-scroll | 135 | 123 | 119 |
| unicode-submit | 275 | 267 | 277 |
| shortcut-replace | 456 | 428 | 435 |
| multiline | 220 | 233 | 199 |
| window-key | 172 | 144 | 120 |
| context-menu | 342 | 301 | 282 |
| dialog | 954 | 412 | 373 |
| long-drag-rejected | 0 | 0 | 0 |
| long-text-rejected | 0 | 0 | 0 |

AppKit cancellation, expiry and user-switch results: cancel_drag 297 ms / restored, lease_expiry 1065 ms / restored, user_switch 302 ms / user_superseded. Post-expiry reads work. Independent user text is checked exactly; the third app still accepts THIRD-KEPT.

| Scenario | Agent calls | Received JSON bytes | PNG bytes |
| --- | ---: | ---: | ---: |
| appkit | 53 | 71543 | 48694 |
| chrome | 42 | 57311 | 47390 |
| electron | 42 | 57209 | 38933 |

Counts include Hello, discovery, original-receipt checks, negative cases and capture metadata. Human/discovery clients are separate; bytes are not LLM tokens. Source fingerprints match the final patch, and helper hashes are recorded. Contract race/vet, Windows cross-build/vet, protocol and 19 JavaScript tests passed.

- [Pre-fix safe refusals](before.json)
- [AppKit](appkit.json), [owned capture](appkit.png)
- [Chrome](chrome.json), [owned capture](chrome.png)
- [Electron](electron.json), [owned capture](electron.png)

WebKit, TextEdit and original PID-routing evidence remain in the [initial evidence](../cooperative-20261004/README.md). [Full scope and reproduction](../../../poc/background-input/FULL_ACCEPTANCE.md). Private SPI, Windows/minimum-macOS/amd64/IME/VS Code limitations and deferred Bot integration remain unchanged.
