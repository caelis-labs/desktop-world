# Releasing Desktop World

Release Please opens a version PR from Conventional Commits on `main`. Merging
that PR creates a `vX.Y.Z` tag and a **draft** GitHub Release, then calls the same
signed candidate workflow used by manual releases. Both platform jobs must pass
before candidate assets are attached to the draft. Publishing remains a maintainer
action after acceptance; an empty or failed draft is not a shipped release.

## One-time repository setup

Make the organization Actions secret `RELEASE_PLEASE_TOKEN` available to this
repository, or configure a repository secret with a dedicated GitHub App
installation token or a fine-grained PAT authorized for this repository, with
Contents and Pull requests write access (and Issues write access for labels).
Include Workflows write access when the generated PR contains workflow changes.
App installation tokens must be refreshed by the caller before use; do not store
a short-lived installation token as a permanent secret.
The checked-in workflow expects a PAT stored in that secret unless you adapt it
to mint an App token. Keep the existing Apple signing/notary secrets and team ID
available to the reusable candidate workflow.

The dedicated token lets generated PRs trigger normal pull-request CI. The
default `GITHUB_TOKEN` suppresses those events. The dedicated token does not
depend on enabling PR creation by the repository's default Actions token. See the
[Release Please Action credential guidance](https://github.com/googleapis/release-please-action#github-credentials).

## Local configuration checks

Run `python3 scripts/release-artifacts.py check` to check all six version files
without creating a tag; CI runs the same guard. The root `simple` strategy is
intentional: limiting a Rust release component to `cmd/dtw` would omit Skill-only
commits under `packaging/`. Extra-file updaters keep the Rust and Plugin versions
in sync. The Cargo.lock JSONPath uses `name.value` because Release Please's TOML
parser wraps scalar values with source positions; it targets only DTW's entry,
leaving dependency versions untouched. Verify this updater when upgrading
Release Please, including an actual increment beyond the current version.

## Normal release

1. Merge reviewed changes using Conventional Commit titles. Before 1.0,
   `feat` and `fix` increment the patch; breaking changes increment the minor.
   The root component includes Skill, packaging and documentation changes.
2. Review the generated release PR: `version.txt`, Cargo package/lock versions,
   all three Plugin manifests, the release manifest and `CHANGELOG.md` must agree
   on the intended release. The manifest records the previous version until the
   release PR updates it. Do not bump these files by hand for normal releases.
3. Merge the version PR. The `Release please` workflow creates the tag/draft,
   builds the exact tag, signs/notarizes macOS binaries, runs package/contract
   probes, checks asset hashes, and uploads both platform candidates to the draft.
4. Download the draft assets, compare each `BUILD-*.json` source revision to the
   tag, verify checksums, and complete the native/real-host checks in
   [release readiness](release-readiness.md#promotion). Record actual coverage
   and limitations in the draft notes.
5. Publish that draft with its accepted bytes. Do not rebuild accepted assets.
   Update README/install download links to the published version. Bot/Core
   dependency updates are separate work with their own artifact pins.

Do not merge another release PR while the previous draft is awaiting acceptance.
After a candidate failure, rerun the failed jobs in the original workflow run.
Uploads intentionally omit `--clobber`: existing assets are never silently
replaced. If the attach job partially succeeds, inspect existing asset hashes
before uploading only the missing files.

## Existing and manual releases

When onboarding an existing repository, seed `.release-please-manifest.json`
with the most recent public version. A local version bump or local-only tag is
not a published release. Keep stale local tags out of pushes: Release Please
creates the remote tag from the reviewed release PR. Never reuse a public tag
for different source.

## Manual candidates and recovery

`Release candidates` still accepts an existing tag through `workflow_dispatch`.
It verifies tag/package version consistency and uploads `candidate-darwin-arm64`
and `candidate-windows-amd64` workflow artifacts. Manual dispatch does not create
or publish a GitHub Release. Use this path for existing/manual tags or recovery;
verify exact source and hashes before attaching files to a draft. A successful
workflow or notarization check alone is not native desktop acceptance.
