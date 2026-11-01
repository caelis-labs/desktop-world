# Historical top2-only POC acceptance — 2026-10-16

Superseded for current behavior by [runtime acceptance](https://github.com/caelis-labs/desktop-world/blob/f1d243a/docs/runtime-acceptance.md).
The hashes and claims below describe the earlier implementation.

The final implementation is a thin DTW extension using **unmodified official CUA
0.35.4**. The earlier experimental native patch is not part of this worktree or
any result counted below. The primary checkout was fast-forwarded to
`10d6c056222fd0756f87493a8db62761e34331e7`; its pre-existing untracked bin/ and
dtw.exe were preserved. POC changes are in branch `codex/dtw-associated-capture-poc`.

Exact binary/source hashes are in [acceptance.json](acceptance.json). Final DTW
binary SHA256 is `9c9f3d6b0f54abd760525901104e4ba0d7227967e3dfdd0d8ec805a72acb7e75`.
The worker is extracted from the checksum-verified official macOS universal asset,
SHA256 `c76d59f64b90147a3928fe7775594b0da26af2b2e523968c90246cdcbd5ad4f4`.
The SDK resolves to the exact release commit; no private CUA source types are used
by the extension. These are local development binaries, not a signed distribution.

## Evidence levels

| Check | Result | Local evidence |
| --- | --- | --- |
| Rust unit/contract tests | 5 passed | /private/tmp/dtw-top2-tests.log |
| MCP smoke, state isolation | passed | /private/tmp/dtw-top2-smoke.log |
| Receipt paging, errors, cancellation, lifecycle | passed, process exit 0 | /private/tmp/dtw-top2-contract.log |
| listApps / permission read-only probe | passed, process exit 0 | /private/tmp/dtw-top2-probe.log |
| Locked license inventory | 373 packages / 679 notice files passed | /private/tmp/dtw-top2-license-probe.log |
| Obsidian chooser image contract in separate worker | passed | /private/tmp/dtw-top2-obsidian-contract.log |
| Separate-worker popup probes | failed; see below | /private/tmp/dtw-top2-wps-contract.log, /private/tmp/dtw-top2-menu-contract.log |
| Format, Python syntax, diff whitespace | passed | local command results |
| Windows/Linux native GUI, signed packaging, CI | not run | no acceptance claim |

License verification exposed a stale cursor-overlay-0.34.0 notice path. The audit
now derives that path from the actual dependency version, avoiding this recurring
upgrade edit. First audit attempt used Cargo's checkout, whose untracked .cargo-ok
violated the existing clean-source gate; verification used a fresh clean local clone.
License inventory uses repository Rust 1.98.0 settings; the exact GUI adapter was
built/tested with Rust 1.97.1. Full release packaging was not validated.

## Real GUI, final binary

Persistent-session evidence: `/private/tmp/dtw-top2-final/NNN`, with request,
receipt, native structured result and separate original image-0.png/image-1.png.

| Case | Observed effect | Evidence |
| --- | --- | --- |
| Obsidian chooser + main window | two images, chooser first | 001 |
| WPS new popup + main window | layer-24 untitled popup first, 728x1268; main second | 002–003 |
| WPS pixel click on 文字 | original popup capture_id, background delivery; CUA AX hit-test; popup closed and another new-document template tab appeared | 004–005 |
| Obsidian path dropdown | native semantic click opens it; service-owned layer-101 menu first, chooser second; original CUA images | 006–007 |
| Obsidian Cancel | native semantic click; chooser disappears, only creation page remains | 008–009 |

No document/vault was created or saved. Obsidian used the temporary profile at
/private/tmp/dtw-scene-apps/obsidian-profile (PID 9712). WPS was a separately
launched PID 11710 at /Applications/wpsoffice.app/Contents/MacOS/wpsoffice, but
still exposed shared account state: it was not account-isolated. The user's
other application processes were not targets. Test processes are stopped afterward.

## Important negative evidence

Earlier same-implementation diagnostics are in `/private/tmp/dtw-top2-acceptance`.
Its adapter predates only the final stricter response-contract validation and
explicit best_effort metadata. All runs listed here use the official stock worker.

- 007: foreground click on the remote path-menu service window failed with
  foreground_unavailable (WindowServer rejected activation).
- 011: foreground click on WPS's non-key popup failed because the main window
  retained focus. The activation attempt also dismissed the popup. A failed
  receipt does not mean no UI state changed.
- 015–017: after reobservation, independently testing background delivery on WPS
  succeeded via upstream AX hit testing and visible navigation. This was a manual
  diagnostic, not an automatic fallback/retry policy.
- 018–020: a chooser coordinate click was posted but did not visibly open its
  menu. Native semantic activation worked. Delivery success is not effect proof.
- 022–023: background coordinate input to the remote service menu failed with
  off_space_or_ax_unresolved: the service window was absent from current AXWindows.
- A separate-worker WPS popup probe timed out while capture reported an unavailable
  frame. Its first harness also narrowed PATH and overlapped the manual sequence;
  it is not a clean causal comparison. The harness was corrected to preserve the
  real GUI environment, drain stderr to a file and allow a bounded 60-second wait.
- With the corrected separate-worker harness, a remote-menu probe still failed
  with “No content produced (neither AX tree nor screenshot succeeded)”. The
  persistent-session capture immediately before it succeeded. Cause is unresolved;
  do not claim fresh-worker popup capture is stable or hide this failed gate.

These results support the extension architecture and the release's basic SDK/
run_script compatibility, but do **not** establish exhaustive window coverage,
universal stable coordinate input, or superiority to the old fixed commit.
0.35.4 remains a tested integration candidate in this worktree, not a recommendation
to promote globally until the failed fresh-worker capture case is understood.

## Upgrade gate

Update the release tag/revision and matching SDK lock together; use the matching
official worker. Run Rust tests, mcp_smoke.py, contract_probe.py, probe.py and the
license audit/probe. Then run app_view_probe.py serially against test windows,
and manually verify WPS popup, native chooser, remote menu, semantic actions,
pixel delivery and stale/closed-window refusal. The public SDK/tool contract is
the compatibility boundary. OS discovery (including its private macOS AX symbol)
has a separate OS-version gate. Do not copy run_script or patch CUA to make a
failed gate pass inside DTW.
