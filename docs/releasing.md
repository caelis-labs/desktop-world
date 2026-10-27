# Releasing Desktop World

The next release will contain the usable rewrite only. It will not ship the old
script facade, CUA worker, or a compatibility runtime. Existing tags and released
assets remain historical versions.

## Current development checks

PR and main CI run two bounded jobs:

| Check | Scope |
| --- | --- |
| Source checks | Version consistency, rustfmt, dependency-free Core/IPC tests, Python syntax and rewrite acceptance negative controls |
| macOS inventory controls | Compile and run the Swift inventory classification controls without GUI access |

Neither job downloads CUA source, builds Cargo dependencies, packages DTW, or
operates a desktop. Each has a five-minute timeout. The required checks are
`Source checks` and `macOS inventory controls`; strict up-to-date branch checking
remains enabled. The old platform package check names must be removed from branch
protection when the replacement checks pass on the CI transition PR.

This is intentionally an early development gate. Protocol, QuickJS and Rust
native-bridge code is feature-gated and is not compiled by the std-only tests.
Changes to those areas require the relevant local build/tests and checkpoint
acceptance. Add CI coverage when a concrete, stable behavior needs protection;
do not recreate the old full-package matrix.

## Next release checkpoint

1. Choose the supported workflows and platforms from the [product matrix](product-matrix.md).
   Record unsupported and unverified cases explicitly.
2. Remove the old default runtime, SDK/worker dependencies and installation/package
   paths. Verify that the single rewritten entry builds and runs without them.
   This cleanup is still pending; removing old CI does not establish it.
3. Complete the selected real-app and Harness workflows, including independent
   effect readback, authorization refusal and uncertain/cancelled outcomes.
4. Define the new package/install contract and implement a candidate workflow for
   those products. Validate final installed bytes, dependencies, signing,
   permissions/TCC and child-process identity on supported platforms.
5. Record source revision, CI scope, artifact hashes and actual native acceptance.
   Publish the same reviewed bytes without rebuilding.

The old `release-please.yml` and `release.yml` workflows have been removed.
Successful development CI no longer opens a version PR, creates a tag/draft, or
builds/uploads old CUA-based candidates. There is no replacement legacy workflow.
Release automation will be added with the rewritten package contract.

Version files remain consistent through `python3 scripts/release-artifacts.py check`;
the existing release metadata and old build scripts are cleanup inputs, not a
supported release route. The historical package guarantees are recorded in
[release readiness](release-readiness.md); they do not certify the rewrite.
