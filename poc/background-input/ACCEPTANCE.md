# Native acceptance — 2026-10-04

The same-desktop AppKit POC passed. WebKit and the no-raise focus probe did not
pass. All input effects below were executed on this Mac through persistent
`dtw` processes; the in-memory contract fixture is only used for guard tests.

## Accepted scenario

- Machine: macOS **27.0.1**, build **26A434**, arm64.
- Base: `a44be9feedf4602b309a6e9fbf475db4cbb504b8`; experimental branch
  `codex/background-input-poc`.
- Run: `background-poc-20261004T111403Z-40a64c`.
- Command: `python3 poc/background-input/accept.py --case appkit --mode public_pid --rounds 3`.
- Experimental helper SHA-256:
  `24027946f5b52fad85fee3df4b4904842f5f83cb385a9ac5a5e5947f8410e852`.
- Local evidence:
  `artifacts/background-poc-20261004T111403Z-40a64c/summary.json`, sibling
  fixture logs, helper transcripts, native timings and `source-sha256.json`.

The background window and human window use the same frame. An ordinary `dtw`
helper rejected the same background target as `target_not_hittable`, with
`delivery: none`. Thus the accepted experiment did not rely on an exposed
portion of the background window.

The agent clicked the occluded text field, entered `POC-0-中文-🙂`, and clicked
Submit; then repeated with round numbers 1 and 2. The controlled app's own
native event monitor and business callback logged the resulting text and all
three submissions. Resets used explicit semantic `set_value`; payload entry
used native keyboard events. Every text step verified the entire value, and
every submit checked that value before delivery.

| Evidence | Result |
| --- | --- |
| Business submissions | **3/3**, exact Unicode values, no duplicates |
| Same-ID/body reconciliation | Same receipt/run, no additional input or submit |
| Foreground document text | **69/69** characters, exact final value, no helper input errors |
| Human key-downs during background interval | **26** |
| Background interval, including reasoning pauses | **3.469 s** |
| Foreground app / active / key-window samples | **173**, sampled every 20 ms; **0** losses observed |
| Shared cursor samples | One unchanged position, `(419,744)` |
| Explicit key-focus lease | **0 ms** in the accepted transport |
| Native click dispatch | 7 clicks, **28.1–36.9 ms** each |
| Native short-text dispatch | 3 payloads, **37.6–38.6 ms** each |
| Background helper output | **15 calls, 25,198 UTF-8 wire bytes**, including initial discovery, detail, verification receipts and 3 reconciliations |

The native timings cover posting, not application-effect latency or desktop
occupancy. Sampling cannot exclude a focus change shorter than 20 ms. Exact
foreground text and app-owned background results provide independent effect
evidence. These are wire-byte costs, not measured model tokens; no screenshots
or full-tree dumps were sent to a model.

Two additional controls each used four helper calls: the ordinary occlusion
control returned 11,617 bytes, and the strict-policy control returned 12,356
bytes. The strict helper used the experimental backend with `no_shared_input`;
its mixed semantic-write/click plan refused before any step delivered input.
The foreground simulator's 74 calls / 60,915 bytes are test traffic rather
than agent task cost.

## Failed provider / focus probes

| Provider / transport | Recorded run | Result |
| --- | --- | --- |
| WebKit / public PID | `background-poc-20261004T105328Z-5aaab5` | Mouse down/up reached the window at the intended input coordinates; editable target remained unfocused. Text refused with `background_target_not_focused`; no business submit. |
| WebKit / SkyLight | `background-poc-20261004T105230Z-f1307d` | Same app-local focus refusal; no business submit. |
| WebKit / no-raise focus lease + public PID | `background-poc-20261004T105530Z-d04839` | Click leases lasted **118.6 / 102.4 ms**. Target stayed unfocused. **28/31** task-interval samples lost the human active/key-window invariant; the final key-window flag remained false despite reverse records. No business submit. |

For the no-raise probe, the human app remained reported frontmost and its full
55-character sequence still arrived. That is insufficient to accept the
technique: app key-window state failed to recover, and the background business
task failed. This also demonstrates why frontmost PID or successful SPI calls
alone cannot prove usable input isolation.

Failed runs were not retried against the same live targets or replayed through
a second transport. Each probe used fresh owned app processes and a fresh
helper epoch. Their implementation snapshots are retained in their local wire
and app logs; they preceded the final positive run's additional role guard and
controls. The final accepted source is identified by its source manifest.

## Verification and current boundary

- `./scripts/check.sh` passed: race tests, vet, Windows cross-build/vet,
  example verification and 19 JavaScript client tests.
- Tagged race tests passed for internal packages, local/helper CLI, contract
  tests, host and protocol.
- CLI tests verify ordinary builds do not expose the experimental flag, and
  invalid experimental modes fail before opening the desktop.
- Engine guard tests cover ordinary foreground/hit-test behavior, exact write
  scope, strict mixed-plan policy, stale identity, deduplication and fencing
  after unknown delivery.
- The final experimental binary compiled and passed the native scenario after
  the native text-role / secure-field guard was added.

This establishes a viable AppKit same-desktop delivery path. It does not
establish compatibility with arbitrary applications, rich text views, IMEs,
games, canvases, Chromium, menus or dialogs; those need separate acceptance.
User app switches/clicks during delivery and cancellation of a focus lease are
also outside this accepted scenario. Windows received no native acceptance
or availability promise.
