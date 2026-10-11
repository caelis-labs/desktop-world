# v0.1.0-rc.4 release gate

This release ships the thin Rust/CUA implementation merged in PR #32, with
Windows permission/focus adaptation, portable protocol probes, live Windows
fixtures and complete dependency attribution. The former Go helper and
`dtw.app` script API are removed; scripts now use the packaged `cua` Skill
through the sole MCP `exec`. Reinstall the complete plugin package, including
its private worker and `LICENSING/` directory. An upstream published worker
without `run_script` is not interchangeable with this pinned source build.

## Responsibility and exclusions

DTW's Windows-specific permission parameter and compact focus summary were
adapter bugs and are fixed with regression coverage. Screenshot capture,
UIA Invoke dispatch, window enumeration and post-click foreground checks are
implemented by CUA. Direct tests of `cua-driver mcp --direct --embedded`
without any DTW process reproduced both remaining native limitations:

| Issue | Direct CUA evidence | Fix direction | rc.4 treatment |
| --- | --- | --- | --- |
| DPI-unaware window PNG/geometry mismatch at 125% | PNG `[198,206,155,39]` versus UIA frame `[250,257,194,50]` | Capture in a consistent DPI/pixel space or select the correct capture backend | Known upstream limitation; excluded from release gate by user instruction |
| Semantic Invoke opens a modal but does not return promptly | Direct `run_script` timed out after 8 seconds; fixture independently logged `picker_opened` | Handle modal/provider Invoke without blocking the native action receipt | Known upstream limitation; excluded |
| Foreground pixel click opens a modal then reports `foreground_unavailable` | Direct worker returned the same error; a bounded subsequent read discovered the picker | Verify the new owned foreground window after the asynchronous transition | Known upstream limitation; excluded |

All three are repairable in CUA's native implementations. This release keeps
CUA unchanged at `ac967b9006aafa10870aadb5a645b038cba9721a`; no speculative
coordinate or input fallback was added to the thin adapter.

An immediate post-action window listing in the initial harness was too early.
That was an acceptance-harness timing issue, now corrected with bounded,
read-only PID-scoped polling. A new DTW fixture then discovered the picker,
verified its exact focus, selected its preset owned file and independently
logged `OK`, with **zero replays** of the opening action. The original opening
receipt was still failed, so that check remains a known upstream failure;
UI effect reconciliation does not relabel the failed receipt as successful.

Direct evidence is retained locally in
`artifacts/rc4-release/upstream-direct-{58a1f3dd,b885242f,2266f835}`. The
completed picker reconciliation is
`artifacts/rc4-release/windows-live-4c54766d/summary.json`. Raw GUI artifacts
are not bundled in public releases. [Windows acceptance](windows-acceptance.md)
records the earlier baseline and exact scope. Full unrestricted Windows GUI
acceptance is not claimed.

## Dependency/license audit

Packaging resolves both locked graphs: the DTW adapter and the actual CUA
worker, on Windows x64 and macOS arm64. Normal and build dependencies,
including proc macros, are included; dev-only and unused optional workspace
members are excluded. The local audit found **384 package/version records**,
**697 original notice files**, and six UniFFI 0.31.0 MPL components. Each CI
builder recomputes its inventory, including its build-host dependencies.

The original texts and copyright notices are carried in `LICENSING/licenses/`;
their hashes and exact versions/origins are in `DEPENDENCIES.json`. Dual
license alternatives are preserved, and combined-license components such
as native crypto retain all original attribution. Published crates that
omit workspace notices use 28 reviewed, hash-verified copies from their
published VCS revisions. QuickJS-ng and vendored native dependencies retain
their own recursive notices, in addition to CUA's derived-code credits.

Six unmodified UniFFI source distributions and their exact-revision MPL
license accompany the binaries in `LICENSING/MPL-SOURCES.tar.gz`; no separate
download is required to obtain that corresponding source. This addresses
the source-availability requirement in [MPL-2.0 section 3.2](https://spdx.org/licenses/MPL-2.0.html).
DTW/CUA's MIT licensing does not replace dependent-library terms.

The embedded **Inter font** was separately identified and its full **OFL-1.1**
notice included; the default CUA cursor theme remains MIT. Rust's linked
standard-library copyright catalog and complete accompanying license texts
are supplied separately. Windows VC++ and Apple frameworks/Swift runtime
are system dependencies, not redistributed binaries. The optional perception
extension, models, Node runtime, and Cua Spaces are not in these packages.

`licenses_probe.py` independently checks every notice hash, registry checksum
presence, embedded-font attribution, and the six source archive prefixes,
Rust source files, original manifests and licenses. `SHA256SUMS` now covers
**every packaged file**, including the license inventory and source archive.
CI and release builds fail on an omitted notice, unknown license expression,
missing MPL source or mismatched attribution bytes.

## Publication validation

The PR must pass macOS and Windows source/package/MCP/contract/license checks
and adapter regression tests before merge. The merged tag is built by
`Release candidates` on native macOS arm64 and Windows x64 runners. Archive
hashes, executable hashes and source/pin identity are retained in their
`BUILD-*.json` evidence. Candidate artifacts are downloaded and checked
before GitHub publication; `ACCEPTANCE.json` records the actual published
asset hashes and validation scope.

The Windows candidate is additionally exercised on this local interactive
machine with the native core fixture and isolated Chrome. macOS live evidence
from the earlier same-pin acceptance remains historical; the rc.4 Mac asset
has native CI/protocol/package validation, not a new macOS Agent GUI run.
Signing remains Windows unsigned / macOS ad hoc, without notarization.
The release is a prerelease, with the upstream limitations explicitly listed.
