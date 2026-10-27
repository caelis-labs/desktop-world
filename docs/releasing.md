# Releasing Desktop World

The next release will contain the usable rewrite only. The old default runtime,
CUA SDK/worker dependency, script facade, plugin payload and installation/package
paths have been removed from the source tree. Existing tags and released assets
remain historical versions.

There is no rewritten release package or candidate workflow yet. A development
build is not an installed, signed or notarized release artifact.

## Current development checks

PR and main CI run two bounded jobs:

| Check | Scope |
| --- | --- |
| Source checks | Version consistency, rustfmt, dependency-free Core/IPC tests, Python syntax and rewrite acceptance negative controls |
| macOS inventory controls | Compile and run the Swift inventory classification controls without GUI access |

Neither job downloads CUA source, builds Cargo dependencies, packages DTW, or
operates a desktop. Each has a five-minute timeout. The required checks are
`Source checks` and `macOS inventory controls`, with strict up-to-date branch
checking.

These are early development checks. The std-only runner excludes the protocol,
QuickJS and Rust native bridge; normal Cargo builds include the rewrite without
opt-in features. Changes to these areas require the relevant local build/tests
and checkpoint acceptance. Add CI coverage when a concrete, stable behavior needs
protection; do not recreate the old full-package matrix.

## Next release checkpoint

1. Select supported workflows and platforms from the [product matrix](product-matrix.md).
   Record unsupported and unverified cases explicitly.
2. Verify the rewrite-only dependency/build boundary and the single public CLI.
   Recheck the relevant protocol and Host startup against the candidate source;
   historical GUI evidence does not certify a rebuilt executable.
3. Complete selected real-app and Harness workflows, including independent effect
   readback, authorization refusal and uncertain/cancelled outcomes.
4. Define the new package/install contract and implement candidate construction
   for those products. Include the actual dependency notices and source provenance.
   Validate final installed bytes, dependencies, signing, permissions/TCC and
   child-process identity on supported platforms.
5. Record source revision, CI scope, artifact hashes and actual native acceptance.
   Publish the same reviewed bytes without rebuilding.

Development CI does not open version PRs, create tags/drafts or build/upload
candidates. The old release automation and artifact-producing scripts are gone;
there is no legacy replacement. New automation belongs to the new package contract.

`python3 scripts/release-artifacts.py check` only checks `version.txt`,
`cmd/dtw/Cargo.toml` and DTW's entry in `cmd/dtw/Cargo.lock`. It does not validate a
release tag, generate licensing inventories or create archives. The historical
package guarantees remain in [release readiness](release-readiness.md); they do
not certify the rewrite.
