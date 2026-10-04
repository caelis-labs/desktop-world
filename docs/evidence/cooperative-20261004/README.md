# macOS cooperative input evidence

2026-10-04. Controlled real-machine scenarios on macOS 27.0.1 arm64. Each feature uses a separate dtw request/receipt and independent application state/events. All three production source snapshots match the committed production implementation. Reconciliation uses the original request ID/body. Raw desktop inventories and full wire transcripts remain local.

Chrome 154.0.8037.93; Electron 44.5.1. Existing OS permissions; no DOM/CDP target automation, no VM or separate login session.

## Foreground duration per scenario (ms)

| Scenario | AppKit | Chrome | Electron |
| --- | ---: | ---: | ---: |
| move | 191 | 185 | 180 |
| single | 145 | 139 | 137 |
| double | 127 | 133 | 136 |
| middle | 124 | 119 | 129 |
| drag | 436 | 428 | 413 |
| vertical-scroll | 134 | 132 | 116 |
| horizontal-scroll | 131 | 132 | 132 |
| unicode-submit | 284 | 279 | 288 |
| shortcut-replace | 442 | 428 | 430 |
| multiline | 194 | 229 | 206 |
| window-key | 137 | 146 | 146 |
| context-menu | 330 | 305 | 287 |
| dialog | 974 | 427 | 379 |
| long-drag-rejected | 0 | 0 | 0 |
| long-text-rejected | 0 | 0 | 0 |

Every successful borrowed scenario reports restored. Rejected long drag/text report not_borrowed with zero occupancy. Foreground duration includes restoration; the one-second input budget is not a hard real-time cutoff.

## Cleanup and independent tasks

- cancel_drag: 284 ms, `restored`, seat `ready`, original outcome `unknown`.
- lease_expiry: 1078 ms, `restored`, seat `ready`, original outcome `unknown`.
- user_switch: 296 ms, `user_superseded`, seat `ready`, original outcome `unknown`.
- WebKit: three real note submissions; 477 ms, 443 ms, 441 ms. Exact 69/69 human characters preserved; 24 entered during task execution. Simulated user retries only independently proven no-delivery input after restoration; no unknown action is replayed.
- TextEdit: owned document saved and disk bytes verified exactly; 327 ms foreground occupancy; independent human input resumes.
- Legacy public_pid: three AppKit rounds remain accepted without foreground/key loss; earlier WebKit probe failures remain documented.

## Costs

| Agent scenario | Helper calls | Received JSON bytes | PNG bytes |
| --- | ---: | ---: | ---: |
| appkit full scenario | 52 | 70220 | 48538 |
| chrome full scenario | 42 | 57295 | 48868 |
| electron full scenario | 42 | 57229 | 38842 |
| WebKit task | 14 | 31527 | 0 |
| TextEdit save/cleanup | 6 | 17863 | 0 |

Calls/bytes include Hello, narrow discovery, actual tasks, verification, deliberately repeated receipt reads, negative scenarios and capture metadata. Human, third-app and read-only discovery clients are separately recorded in JSON. Bytes are not model tokens; no LLM was run and no universal success rate or token saving is claimed.

The minimal catalog is 157 bytes; click/text/drag schemas are 5,428 / 5,545 / 6,497 bytes. Capture discovery is confined to the known app, and native dialog bind to the known window. No new model-facing mode parameter or action catalog is introduced.

## Artifacts and checks

- [AppKit results](appkit.json), [owned window](appkit.png)
- [Chrome results](chrome.json), [owned window](chrome.png)
- [Electron results](electron.json), [owned window](electron.png)
- [WebKit](webkit.json), [TextEdit](textedit.json), [legacy public_pid](legacy-public-pid.json)

JSON contains binary SHA256 and controlled results; full-provider JSON also has the matching production source SHA256 list. Base commit identifies the starting checkout, not a claim that the working tree was clean during POC. Contract checks passed: race/vet, Windows cross-build/vet, protocol examples, 19 JavaScript tests, and tagged POC race tests. Native tests are separate from CI.

VS Code is unaccepted: isolated-profile startup exposed no owned document. Windows real-machine availability, minimum macOS/amd64, IME, arbitrary providers, same-app human changes and crash/kill recovery remain outside this acceptance. Full scope, commands and interpretation: [FULL_ACCEPTANCE.md](../../../poc/background-input/FULL_ACCEPTANCE.md).
