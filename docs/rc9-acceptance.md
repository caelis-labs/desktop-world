# RC.9 integration acceptance

> Historical evidence: this report applies only to its named source revision, binary and entry point. The current product uses [standard MCP and explicit app IDs](app-protocol-v1.md); any grant, manifest, SDK/worker or custom Host setup below is superseded and is not a current installation instruction. No acceptance result here is claimed to have been rerun for the new entry.

> Historical checkpoint record. Build flags, default entry points and release plans below belong to the recorded revision. The current rewrite-only build is described in [building](building.md); this evidence does not validate a newly built binary.

Candidate: `v0.1.0-rc.9`, following checkpoint `f07a4d5`. Local exact-package
verification passed; CI/signing gates remain separately recorded below. Publication is not implied. CUA remains unmodified
at component prerelease 0.35.4, revision
`d74a9cab3fb2e80ab7f0cef47025ec828586ea75`.

## Reproduced blocker and fix

The checkpoint failure was not limited to a cold process. A second live copy of
the same executable could enumerate SCK windows but never receive its image
callback while the first copy remained alive. A minimal public-API Swift program
reproduced this without CUA. Stopping the live peer restored eight consecutive
fresh-process captures. This establishes a multi-instance lifetime trigger on
macOS 27.0.1 (26A434), not Apple's underlying implementation cause.

DTW now runs native extension calls in a bounded child of its own signed binary.
A parent-held per-user file lease lasts until child exit, so the OS capture client
is released before the next native operation. There is no new daemon, private OS
API, copied CUA script engine or separately installed helper. The original CUA
worker/SDK and process-private Agent state remain unchanged. Lock wait is bounded
to three seconds and child execution to 25 seconds. Ambiguous input-child failure
is reported as `unknown`; input is never retried.

Source: [bridge](../cmd/dtw/src/app_view/macos.rs),
[minimal diagnostic](../tests/fixtures/sck-client/README.md),
[multi-Runtime regression](../tests/acceptance/multi_runtime_scene_probe.py).
The earlier negative record remains in [checkpoint acceptance](runtime-acceptance.md).

## Exact local package

- Path: `/private/tmp/dtw-rc9-final`
- DTW SHA-256: `08dd9f41505121e92f44e0a2be30fd02ad2c5c2dec166bd7df6b644b09905300`
- Worker SHA-256: `15a2abacf6a856978de4b2c3e8463c79bc3c502321d4169f4bcf38d8e44ba043`
- Local ad hoc signing; all three Plugin manifests and Cargo identify RC.9.
- Build log: `/private/tmp/dtw-rc9-final-build.log`.

## Local acceptance passed

- Rust 1.98.0 release-mode unit tests: 6 passed, including lease exclusion,
  release-on-drop, unsafe mode and symlink refusal.
- Final package: MCP, contract, Runtime process isolation, license inventory and
  independent Unicode/space-path installation passed.
- WPS final GUI receipts 001–003: associated floating New surface captured;
  screenshot-bound click closes it and opens the text-template page, verified
  in the next image. No document was created or saved.
- Obsidian final receipts 015–024: chooser and main window associated; menu adds
  a third, remote-service window. Menu click changes `private` to `Macintosh HD`;
  Cancel closes chooser. The main window alone remains. No vault was created.
- Five fresh final-package Obsidian processes captured the remote menu while the
  original Runtime remained alive: all passed, 1.05–1.11 seconds each. Evidence:
  `/private/tmp/dtw-rc9-final-obsidian-cold`.
- A pre-final package with identical runtime code but an older embedded Claude
  manifest also passed five cold WPS overlay reads. This is supporting evidence,
  not an exact-final-package repeated WPS test.

- Final real fixture: a fresh Runtime inventories 3 windows (default 2 images,
  expanded 3), and independently verifies both SCK and CUA ordinary-window point
  effects. Reused, moved, closed, script-invalidated, opaque-covered and expired
  observations refuse input; no extra counter increment occurs.
- Final multi-Runtime regression: 6 alternating and 2 concurrent reads all pass
  with two live identical processes (maximum 1.397 seconds). A third preexisting
  GUI Runtime also remained alive.
- Ten further fresh-process final-package captures pass in 0.827–0.857 seconds.
  Evidence: `/private/tmp/dtw-rc9-final-fixture-evidence`,
  `/private/tmp/dtw-rc9-final-multi-evidence`, `/private/tmp/dtw-rc9-final-cold`.

GUI receipts, structured observations and PNGs are in
`/private/tmp/dtw-rc9-final-gui`. Obsidian test PID 86447 used an isolated profile
at `/private/tmp/dtw-rc9-obsidian-profile`; existing user PID 3167 was untouched.
WPS PID 86445 used the existing WPS account/profile and opened template tabs.
Only these test app instances were terminated afterward.

## Known semantics and remaining release gates

CUA's background coordinate path can return AX delivery success without an
observable Electron button effect. Final Obsidian receipts 009–013 show no chooser
after Browse; after readback, an explicitly chosen CUA foreground click (014)
opened it. This is not an automatic retry or an effect guarantee. The app-bound
SCK click path then handled the chooser and remote menu. Agent must always verify
UI effect and use the documented delivery ladder deliberately.

Application attribution remains best effort; top2 limits image output, not the
inventory. Other Spaces, protected surfaces and arbitrary unidentifiable helper
windows are not guaranteed. Multiple-display/mixed-DPI live input, Windows live
GUI, a fresh external Agent host, and clean-machine permissions are not covered
by these macOS fixture/MCP tests. Public Developer ID signing/notarization and
cross-platform CI remain separate gates before publication.

## Candidate integration status

The local candidate passes the runtime, GUI, packaging and source gates above.
The integration PR runs macOS and Windows source/package/MCP gates. Release
promotion additionally requires the existing Developer ID/notarization workflow
and checksum verification of its downloaded artifacts; no release has yet been
published by this task. [Machine-readable local evidence](rc9-acceptance.json)
records exact source hashes separately from the retained checkpoint evidence.
