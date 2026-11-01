# Releasing Desktop World

The v0.2.0 release contains the usable rewrite only. The old default runtime,
CUA SDK/worker dependency, script facade, plugin payload and installation/package
paths have been removed from the source tree. Existing tags and released assets
remain historical versions.

The rewrite candidate builder is `scripts/build-rewrite-candidate.py`. It exports
a clean Git commit and builds one native target; it does not publish a release.
A development build is not an installed, signed or notarized release artifact.
Retain candidate identity, original logs and final-install acceptance with the
release PR/artifacts. Keep development-binary evidence separate from evidence for
the immutable archives selected for publication.

## Release Please and accepted-package publication

Release Please maintains the version PR and changelog from Conventional Commits.
Its manifest, `version.txt`, Cargo.toml and the DTW entry in Cargo.lock advance
together; dependency versions are not bumped. Installation pages select the
latest stable release, so an unaccepted draft is never advertised as the download.
The bootstrap starts at v0.2.1; retired 0.1.x changes are not released again.

`release-please.yml` follows successful main CI and also supports manual dispatch.
It uses the existing `RELEASE_PLEASE_TOKEN` repository/organization secret, as
Caelis does. A separate bot token lets its PRs trigger ordinary CI; copying a
developer's local credentials is unnecessary. The action is pinned to v5.0.0's
commit. Merging its version PR creates a **draft and tag**, never a public release.

Build candidates for the stable version PR head using the existing Windows/macOS
candidate workflows (manual dispatch or their build labels). Complete GUI and
Harness acceptance on those exact ZIPs. Keep the original outcomes and archive
hashes. Then merge the version PR and run **Publish accepted release** once:

```sh
gh workflow run release-promote.yml --ref main \
  -f tag=vX.Y.Z -f macos_run_id=MAC_RUN -f windows_run_id=WIN_RUN \
  -F gui_accepted=true -f acceptance_notes='Actual per-platform GUI/Harness scope and retained failures'
```

This explicit attestation is bound to the selected candidate run IDs and ZIP
hashes. Compilation or notarization alone never supplies it. The workflow checks
successful same-repository candidate workflows and their artifact digests, keeps
the original ZIP/BUILD/CANDIDATE/notarization bytes, and generates the 13-asset
release inventory. It verifies signatures natively on macOS, uploads missing
assets, downloads and audits the draft, and only then publishes those same bytes.
It downloads and audits again after publication because `GITHUB_TOKEN` writes
do not trigger a second release-event workflow. No executable from an artifact is
run, and nothing is rebuilt, re-signed or recompressed.

An existing complete draft keeps its original RELEASE.json and SHA256SUMS.
Partial uploads can resume only when every existing byte matches; no `--clobber`
or replacement is permitted. A rerun on an already published release only audits
it. Unexpected or changed assets fail rather than being repaired automatically.
The promotion report is retained as an Actions artifact with the selected sources,
hashes, explicit acceptance notes, preflight and publication audit results.

## Current development checks

PR CI selects checks from the changed paths:

| Check | Scope |
| --- | --- |
| Portable checks | Version consistency, rustfmt, dependency-free Core/IPC/Linux inventory controls, Python syntax, packaging/acceptance/CI negative controls |
| macOS inventory controls | Full rewrite Rust script/MCP/CLI contracts, Swift inventory/input/overlay controls, and native/scene worker shutdown without desktop access |
| Windows adapter contracts | Windows UIA/geometry/input/cursor contracts, CLI integration, and compilation of the real-machine acceptance fixtures |
| Source checks | Required aggregate: every selected check must succeed; cancellation or an unexpected skip fails |

Documentation-only changes avoid native compilation. Platform-only changes select
that platform; shared Rust or CI changes select both. Rust runs in the shipping
`release` profile, with a fixed toolchain and dependency/build caches. This also
keeps QuickJS stack behavior consistent with the distributed binary. Shared
script/MCP tests run once on macOS; Windows adds its platform-specific contracts.
No job builds upstream CUA, signs a package, or operates a desktop. The existing
required names `Source checks` and `macOS inventory controls` remain unchanged,
with strict up-to-date branch checking; the aggregate also protects Windows.

On a main push, CI looks for the merged PR's successful CI receipt and compares
the **Git tree**, not just the PR number or head SHA. An identical tree reuses
that verified result without recompiling. Missing, expired, failed, or mismatched
evidence runs fresh checks. Manual `CI` dispatch always runs the full checks;
this can also seed the main-branch Cargo cache after a toolchain change. PR caches
remain subject to GitHub's branch scope. PR supersessions cancel older ordinary
CI runs; candidate signing/notarization is never cancelled by a newer trigger.

Current platform scope is in the [product matrix](product-matrix.md); reusable
build and real-application validation instructions are in [building](building.md).
Windows and experimental Linux providers are implemented; Windows native linking
and independent GUI acceptance remain separate from source/type checks. Linux desktop
support is deferred. The [v0.2.0 release](https://github.com/caelis-labs/desktop-world/releases/tag/v0.2.0)
promotes the accepted platform archives unchanged; its `RELEASE.json` identifies
the original candidate sources and hashes.

## Release checkpoint

1. Select supported workflows and platforms from the [product matrix](product-matrix.md).
   Record unsupported and unverified cases explicitly.
2. Verify the rewrite-only dependency/build boundary and the single public CLI.
   Recheck standard MCP discovery, execution and cancellation against the candidate source;
   historical GUI evidence does not certify a rebuilt executable.
3. Complete selected real-app and Harness workflows, including independent effect
   readback, explicit app ID failures and uncertain/cancelled outcomes. DTW does
   not approve operations; any Agent tool policy is validated in that Agent.
4. Build from a clean, fixed commit. Each platform ZIP must include the runtime,
   standard plugin/MCP manifests, Skill, installation references, dependency
   notices, source provenance, BUILD identity and complete checksums. The local
   plugin staging helper is a development input, not a release builder. Validate
   final installed bytes, dependencies, signing, permissions/TCC and child-process
   identity on supported platforms.
5. Record source revision, CI scope, artifact hashes and actual native acceptance.
   Publish the same reviewed bytes without rebuilding.

Ordinary development CI does not open version PRs, create tags/drafts or build
candidates. Separate `Rewrite Windows candidate` and `Rewrite macOS candidate`
workflows run only by explicit dispatch or their respective `build-windows-candidate`
and `build-macos-candidate` labels on a same-repository PR. The Windows workflow
checks out that exact PR head, builds the rewrite once with MSVC/static CRT,
runs its tests and retains the candidate plus original construction logs. A later
commit does not automatically repeat this job. It never compiles upstream CUA or
publishes a release. The macOS workflow independently builds/signs/notarizes one
fixed arm64 candidate and runs tests against its exported source. It reuses the
existing `APPLE_DEVELOPER_ID_P12_BASE64`, `APPLE_DEVELOPER_ID_P12_PASSWORD`,
`APPLE_NOTARY_APPLE_ID`, `APPLE_NOTARY_PASSWORD` secrets and `APPLE_TEAM_ID`
configuration variable. A temporary CI keychain holds that identity/profile; raw
credentials are removed from the environment before the build. Its private files
and keychain are removed and the original search list restored even on failure.
Missing credentials stop before compilation. Neither platform trigger dispatches
the other platform. The old CUA artifact automation remains retired.

`python3 scripts/release-artifacts.py check` only checks `version.txt`,
`cmd/dtw/Cargo.toml` and DTW's entry in `cmd/dtw/Cargo.lock`. It does not validate a
release tag, generate licensing inventories or create archives. Use the candidate
builder and final-asset verifier below for those contracts.

## Verify a staged release

After uploading the frozen, accepted candidates to a draft and creating its tag,
run **Release asset verification** (`release-check.yml`) with that tag. It checks
the full release/ZIP checksum inventories, both platform/version/source records,
embedded BUILD/binary/Skill/notices, actual executable formats, macOS Developer ID
signature and the original Accepted notarization receipts. It downloads existing
assets only: no compilation, re-signing, re-compression, or new notary upload.
The same check runs on `release.published` as a post-publication audit.

Require a successful draft preflight before publishing those same assets. The
publication workflow performs that gate itself; this standalone download/audit
workflow does not publish or prevent an administrator from bypassing it in the
GitHub UI. Manual draft audit needs `contents: write` solely because GitHub hides
drafts from readers. Published-event audits keep `contents: read`; both call the
same maintained verifier. It establishes artifact integrity, not GUI acceptance or
build-run provenance. Keep the independently recorded macOS and Windows
acceptance bound to the archive hashes; the existing release manifest may name
different accepted platform source commits, so source is not assumed to equal
the tag commit. A changed asset invalidates the earlier preflight.

Candidate workflows cache only Cargo dependencies/build output in a separate
release cache. Each explicit candidate is compiled once and its tests reuse that
target directory. Signed ZIPs, keychains, credentials, notarization receipts and
acceptance evidence are never taken from the build cache. The macOS candidate
also exercises shutdown using the final signed package binary without desktop
access. A failed test still retains reusable dependency compilation; cache entries
are not pass receipts. Normal PR/main runs never automatically dispatch candidate
builds.

## Build and retain a fixed candidate

Only `aarch64-apple-darwin` and `x86_64-pc-windows-msvc` are candidate targets.
Use their native host, Rust 1.98.0 with `rust-docs`, Python 3.11+, Git and the
native compiler/SDK. The Windows builder runs in an MSVC Developer shell; the
workflow uses Microsoft's documented [Developer PowerShell entry](https://learn.microsoft.com/en-us/visualstudio/ide/reference/command-prompt-powershell).
The checkout must be committed and clean, and the output directory must be new
and outside it. Failed output remains available; use a new directory after a fix.

```sh
python3 scripts/build-rewrite-candidate.py \
  --target aarch64-apple-darwin \
  --output /tmp/dtw-candidate-macos \
  --signing-identity "Developer ID Application: Your Name (TEAMID)"
```

Add `--notary-profile EXISTING_PROFILE` when an existing keychain credential profile
is available; use `--notary-keychain /absolute/path/to/keychain` when that profile
lives outside the default keychain. The builder never creates credentials. It signs the single runtime
with hardened runtime and `dev.caelis.dtw`; the native worker is the same executable.
No entitlement is inferred. An omitted profile leaves notarization unverified.
The ZIP is fixed before submission, and its exact digest and original Accepted
submission/wait/info receipts stay outside the archive in `CANDIDATE.json` and the
notarization JSON files. Submission immediately records its ID before a bounded
wait; a failed wait retains that original ID and never automatically resubmits.
Nothing is rebuilt or rewritten after submission.
Standalone ZIPs are not stapled; offline Gatekeeper behavior remains a separate check.

```powershell
python scripts/build-rewrite-candidate.py `
  --target x86_64-pc-windows-msvc `
  --output C:\Temp\dtw-candidate-windows
```

Each ZIP contains one `bin/dtw[.exe]`, the canonical Skill, normal MCP/Codex examples,
optional standard Claude metadata using that same executable, source notices,
actual target dependency notices, Rust runtime notices, `BUILD.json` and a complete
`SHA256SUMS`. The outer `.sha256` covers the ZIP. BUILD records source commit/tree,
lockfile, toolchain, build environment, binary format/dependencies, minimum target,
version and hashes. Its CLI check does not imply GUI or clean-install acceptance.

The same extracted directory can be registered as ordinary MCP + Skill or loaded
with `claude --plugin-dir`; choose one route per client. Neither route needs a custom
Host or a development toolchain at runtime. Keep the complete extracted directory.
For an upgrade, extract to a new directory, finish existing operations and stop the
old connection, update the configured executable/plugin path, then reconnect and
rediscover. Do not replay uncertain actions. Remove a stopped installation by
removing its registration and directory; the package does not edit Agent settings.

Before promotion, independently verify the final extracted bytes from a path with
spaces/non-ASCII characters, standard MCP discovery, native subprocess identity,
OS permissions, real applications, original receipts and cleanup. Use the same
binary/archive digests for publication. A Windows acceptance result applies only
to its recorded package and configurations; use the maintained
[Windows acceptance matrix](../tests/acceptance/windows/README.md) for new candidates.


## Review-to-release handoff

A Windows candidate also contains `acceptance/windows/` and the same-source
`acceptance/browser-workbench/{server.py,index.html}`. Follow the bundled Windows
README for ordered smoke checks, Chrome startup/readback and the three-app matrix.
The browser workbench explicitly reads/writes UTF-8 on localized Windows systems.
Select the artifact by its recorded source SHA and archive digest, not by a latest-run label.

The final macOS three-app runner accepts `--installed-package /absolute/package`
together with its original `--dtw /absolute/package/bin/dtw`; it uses the installed
binary and bundled Skill, retaining and revalidating the complete checksum manifest.
A copied development binary is not installed-package evidence. Target Agents and
current gates are [Caelis, DSH and agy](building.md).
Release claims must name the candidate actually used. Core app approval is tested
in Caelis, not implemented inside DTW.
