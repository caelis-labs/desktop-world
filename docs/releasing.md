# Releasing Desktop World

The next release will contain the usable rewrite only. The old default runtime,
CUA SDK/worker dependency, script facade, plugin payload and installation/package
paths have been removed from the source tree. Existing tags and released assets
remain historical versions.

The rewrite candidate builder is `scripts/build-rewrite-candidate.py`. It exports
a clean Git commit and builds one native target; it does not publish a release.
A development build is not an installed, signed or notarized release artifact.
The candidate and final-install evidence are tracked separately in
[the candidate record](rewrite-candidate-acceptance.md).

## Current development checks

PR and main CI run two bounded jobs:

| Check | Scope |
| --- | --- |
| Source checks | Version consistency, rustfmt, dependency-free Core/IPC tests, Python syntax and rewrite acceptance negative controls |
| macOS inventory controls | Compile and run the Swift inventory and input controls without GUI access |

Neither job downloads CUA source, builds Cargo dependencies, packages DTW, or
operates a desktop. Each has a five-minute timeout. The required checks are
`Source checks` and `macOS inventory controls`, with strict up-to-date branch
checking.

These are early development checks. The std-only runner excludes the protocol,
QuickJS and Rust native bridge; normal Cargo builds include the rewrite without
opt-in features. Changes to these areas require the relevant local build/tests
and checkpoint acceptance. Add CI coverage when a concrete, stable behavior needs
protection; do not recreate the old full-package matrix.

The current implementation audit, required work packages and macOS/Windows/Linux
validation responsibilities are in the [first rewrite release plan](first-rewrite-release-plan.md).
Windows and experimental Linux providers are implemented; Windows native linking
and independent GUI acceptance remain separate from source/type checks. Linux desktop
support is deferred. Version 0.2.0 is reserved for the rewrite candidate, not a published release.

## Next release checkpoint

1. Select supported workflows and platforms from the [product matrix](product-matrix.md).
   Record unsupported and unverified cases explicitly.
2. Verify the rewrite-only dependency/build boundary and the single public CLI.
   Recheck standard MCP discovery, execution and cancellation against the candidate source;
   historical GUI evidence does not certify a rebuilt executable.
3. Complete selected real-app and Harness workflows, including independent effect
   readback, explicit app ID failures and uncertain/cancelled outcomes. DTW does
   not approve operations; any Agent tool policy is validated in that Agent.
4. Define the release package/install contract and implement candidate construction
   for those products. The local Claude plugin staging helper is a development
   input, not a signed release builder. Include the actual dependency notices and source provenance.
   Validate final installed bytes, dependencies, signing, permissions/TCC and
   child-process identity on supported platforms.
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
release tag, generate licensing inventories or create archives. The historical
package guarantees remain in [release readiness](release-readiness.md); they do
not certify the rewrite.

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
binary/archive digests for any future publication. Windows remains pending until
its independent matrix passes; this workflow cannot mark desktop support approved.
