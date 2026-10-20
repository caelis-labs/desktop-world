# Click contract and CUA 0.37.0 acceptance

Issue [#41](https://github.com/caelis-labs/desktop-world/issues/41), checked
2026-10-20. SDK, lockfile, worker build scripts and notice inventory now pin
`2d2f99228d4d2508dc6fcc84c176b296e146714a`, tag
[`cua-driver-rs-v0.37.0`](https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.37.0).
This is the latest published stable **driver component** checked that day. Its
release notes explain that GitHub's prerelease flag keeps the monorepo's Latest
pointer on other products; plain driver SemVer releases use stable channels.

The shipped [API declaration](../packaging/plugin/skills/desktop-world/api.d.ts)
and [Skill](../packaging/plugin/skills/desktop-world/SKILL.md) describe the same
contract. DTW adapts app handles from `getApp` / `launch` and `cua.call` without
modifying the native worker. Either `modifiers: ["ctrl"]` or
`modifier: ["ctrl"]` becomes the native array field `modifier`; strings, unknown
key names and conflicting aliases are rejected before pointer dispatch.

| Method | Supported options beyond delivery_mode | Observation binding |
| --- | --- | --- |
| `click` | button; modifier array; from_zoom | capture_id with a pixel Point, mutually exclusive with from_zoom |
| `rightClick` | modifier array with a pixel Point; Windows from_zoom | None; use `click(point, {button:"right", capture_id})` for bound right input |
| `doubleClick` | Windows from_zoom | None |
| `typeText` | None in DTW's authoring declaration | None |

Right/double-click preserve Windows pixel `from_zoom` conversion after `zoom()`;
macOS rejects that field. Unsupported capture/button options fail explicitly
rather than dropping a binding or selecting another input route. macOS modified `click` requires an
exact window and explicit foreground delivery; DTW never fronts or rebinds a
window automatically. Native `operation=click` retains its separate one-use
observation contract and single unmodified left-click behavior.

## Evidence

Local macOS arm64 checks used the official 0.37.0 release worker. Downloaded
`cua-driver-rs-0.37.0-darwin-arm64.tar.gz` SHA256:
`1e9b8c58cc830e080ada881ec76286529f5f1264fe9ecc371b0ae95cff98928b`
(matched GitHub's asset digest); extracted executable SHA256:
`bf3d7e400bd8d46186e624e74413715b02924c74512ce9001f549b0e7af506cf`.
Package assembly ad hoc signs its own copy; the SDK was built from the same tag.

- 70 checks run against the clean, exact upstream prelude with a recording host:
  argument mapping, getApp/launch handles, raw calls and pre-dispatch rejection.
  This layer sends no native input.
- TypeScript positive calls and nine expected type errors pass with TypeScript
  5.9.3, `--strict --lib es2020 --noEmit`.
- Packaged worker probes omit all input targets and coordinates. They reproduce
  upstream `unknown argument modifiers` and `right_click: unknown argument
  capture_id`; both adapted array spellings reach the same missing-target
  error. DTW rejects scalar/conflicting/unknown modifier values and unsupported
  method options before reaching native pointer dispatch.
- Rust's eight tests, existing MCP receipt contract, formatting and release
  version consistency checks pass. CI is configured to run the prelude, types and packaged
  click probes on both supported build platforms; local execution was macOS.

A dedicated AppKit fixture recorded real `NSEvent`s in its own `state.json`,
independently of DTW receipts. Each case ran once with fresh fixture-only capture
coordinates; no input was replayed. All valid calls returned completed receipts.
This GUI pass used the development adapter (SHA256
`55f9e0fc43f8e1fff222f470eb9d084db481b93494070bb9531621e831195ea1`),
not a signed distribution candidate. Final standalone/slim packages separately
passed click/MCP contracts, checksums and ad hoc signature verification; the
standalone package also passed runtime isolation and the license inventory.

| Case | Independently observed events |
| --- | --- |
| DTW modifiers array, foreground exact-window click | One left down/up pair, ctrl=true |
| Unadapted upstream scalar modifier | One left down/up pair, ctrl=false |
| DTW scalar modifier | Rejected receipt; no new event |
| click with button=right and capture_id | One right down/up pair |
| rightClick without capture_id | One right down/up pair |
| doubleClick | Two left down/up pairs, clickCount 1 then 2 |
| rightClick with ctrl array | One right down/up pair, ctrl=true |

Raw local receipts are in `/private/tmp/dtw-issue41-evidence/click-contract.json`
and `native-click.json`; fixture readback is in
`/private/tmp/dtw-issue41-native-fixture/native-report.json`. These paths are
local test artifacts, not distributed runtime dependencies.

## Remaining upstream limits

0.37.0 still accepts a scalar native `modifier` but reads it as an empty array;
the fixture confirms ordinary input. DTW rejects that shape. Native rightClick
still has no capture binding; doubleClick has no modifier option on macOS, while
Windows accepts modifier arrays as a no-op. DTW rejects that double-click modifier option on both platforms and preserves
Windows right/double-click zoom conversion. Windows modifier handling
and other provider-specific GUI effects remain unvalidated locally. A delivered
mouse event is not proof of a context menu, selection or other application effect.

These upstream differences are non-blocking for DTW delivery. No upstream report,
app-authorization research (#42), live-account test or production replacement is
part of this change. Existing historical acceptance documents retain their pins.

The PR #43 review follow-up also verifies real pinned-worker `calls[].line`,
error lines and human-readable logs against Agent source lines (including the
wrapped app click). Adapter frames use a separate QuickJS function source and
no lines are prepended to the Agent body. Native acceptance now hard-fails on a
failed receipt, missing/wrong events, modifier mismatch or double-click count;
each case uses an explicit append-only event window and waits for quiet before
continuing. Negative controls prove wrong/empty results fail, and the release
candidate workflow runs the packaged click probe after macOS signing/notarization
and on the final Windows package. The seven asserted native cases passed using
the updated local ad hoc slim package and a canvas that explicitly accepts the
activation click. Two earlier zero-event failures are retained as negative
evidence; no input was replayed in either failed run. Review receipts are in
`/private/tmp/dtw-issue41-evidence/review-first-mouse-native.json`. This remains
macOS fixture evidence, not a notarized-distribution or Windows GUI acceptance.

## Reproduce

```sh
node tests/acceptance/click_prelude_contract.mjs /path/to/clean-pinned-cua
npm exec --yes --package typescript@5.9.3 -- tsc --strict --lib es2020 --noEmit tests/acceptance/click_types_contract.ts
python3 tests/acceptance/click_contract_probe.py /path/to/package/bin/dtw
# Opt-in native acceptance; this opens only the dedicated empty test app.
sh tests/fixtures/click-contract/build-run.sh /private/tmp/new-click-fixture
python3 tests/acceptance/click_native_probe.py /path/to/package/bin/dtw /private/tmp/new-click-fixture
```
