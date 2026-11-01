# Releasing Desktop World

Merge the Release Please version PR after its required checks pass. The remaining
release process is automatic: successful main CI starts fixed-source Windows and
macOS construction, then the completed construction starts verification and
publication. No GUI acceptance, run IDs, acceptance notes or manual draft
promotion are required. Optional real-machine and Agent probes remain useful
development diagnostics, outside the publication gate.

## Automatic path

1. **CI** verifies the merged main tree. Release Please checks the exact commit's
   successful CI receipt and confirms main has not advanced before maintaining
   the version PR/release. A stale completion waits for the newer commit's own CI.
2. **Release please** updates the version/changelog from Conventional Commits.
   After a version PR merges, it creates the tag and an internal staging release.
   The planner resolves that tag's actual commit and independently verifies its
   successful main CI/tree, including a branch-advance race during the API call.
   Both reusable builders check out that same immutable source SHA.
3. **Rewrite Windows candidate** builds with MSVC/static CRT, runs release-profile
   Rust/CLI contracts, compiles the GUI test fixtures without operating a desktop,
   and checks the final packaged binary's CLI and MCP metadata/EOF lifecycle.
   **Rewrite macOS candidate** builds arm64, signs with Developer ID, submits the
   fixed ZIP for notarization and requires Accepted receipts. It runs same-source
   Rust contracts, native worker shutdown and the same final-package metadata
   check without desktop access.
4. Only when both builders succeed, a construction plan records the exact source,
   CI receipt, artifact IDs and SHA-256 digests. Artifact names include their
   attempt, so a failed platform can be retried while retaining the other
   platform's original successful bytes.
5. **Publish release automatically** consumes that successful plan. It preserves
   both ZIPs and original BUILD/CANDIDATE/notarization bytes, verifies all 13
   assets and native macOS signatures, uploads missing staged assets, downloads
   and verifies the complete remote set, then publishes it. A post-publication
   download/audit verifies the same bytes again.

The internal draft is an implementation detail, not a review or operator step.
It prevents a partial upload from becoming a public/latest release. This follows
GitHub's [complete-then-publish guidance](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases).
An older version that completes later is published without taking `latest` from
a newer stable release. Install prompts continue to select the latest stable
release and need no version edits.
Construction and publication use serialized
[multi-pending concurrency queues](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency)
so a later maintenance run cannot replace a pending publication. The publisher
also compares stable versions before changing `latest`.

The workflow chain is CI → Release please/construction → publication, within
GitHub's [workflow_run chain](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run).
The default-branch SHA of a completion event is not used as the package source.
Successful same-repository/main workflow identities, the exact CI tree receipt
and the resolved tag source are validated separately. Downloaded JSON evidence
is parsed, never executed. Publication uses a fixed trusted workflow revision.

## Configuration and retries

Keep the existing `RELEASE_PLEASE_TOKEN` bot secret for unattended version PRs and
release metadata. It needs repository contents and pull-request access. The
workflow uses the pinned
[Release Please action](https://github.com/googleapis/release-please-action);
the manifest, `version.txt`, Cargo.toml and DTW's Cargo.lock entry advance together.
Dependency versions are not bumped.

macOS construction uses the existing `APPLE_DEVELOPER_ID_P12_BASE64`,
`APPLE_DEVELOPER_ID_P12_PASSWORD`, `APPLE_NOTARY_APPLE_ID`,
`APPLE_NOTARY_PASSWORD` secrets and `APPLE_TEAM_ID` configuration variable.
Missing credentials fail before building. A temporary keychain holds the signing
identity/profile; raw credentials are removed from the builder environment. Its
private files/keychain are removed and the original search list restored even
when construction fails.

Use Actions **Re-run failed jobs** for a failed construction. Successful platform
outputs remain bound by ID/digest; different successful attempts may coexist for
the same source. The plan itself is uploaded only after both platforms succeed.
The [artifact action](https://github.com/actions/upload-artifact) treats each
uploaded artifact as immutable, so attempts use distinct platform names rather
than overwriting an earlier upload.

For a publication/upload failure, rerun the failed **Publish release
automatically** run. It downloads the original successful construction again;
it does not rebuild, re-sign, re-compress or resubmit notarization. Already
uploaded assets must match exactly. Matching missing draft assets can be added;
any changed or unexpected asset fails. An already published release is audited
without repair or replacement. Expired/missing evidence fails rather than being
interpreted as acceptance. Construction and publication reports are retained for
90 days. A previous successful construction also prevents a duplicate automatic
build from creating a different signed package for the same pending tag.

Release writes use `GITHUB_TOKEN`. Such writes do not trigger a second
release-event workflow, so the publisher performs its own final audit; see
[GitHub's token-trigger rules](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow).
No manual dispatch or GUI attestation is needed to finish the normal path.

## Integrity and evidence

The release has exactly 13 assets: two platform ZIPs, their two sidecars, two
BUILD records, two CANDIDATE records, the three original notarization receipts,
RELEASE.json and the complete SHA256SUMS. ZIP inventories cover runtime, standard
plugin/MCP manifests, Skill, installation references, dependency/source notices,
BUILD identity and all package files. Executable formats, binary/Skill hashes,
versions, target paths, plugin layouts, Developer ID signatures and original
Accepted notarization identities are verified before publication.

`RELEASE.json` records `automated_ci` and binds the source checks/construction to
the archive digests. The candidate's original `gui: unverified` and clean-install
fields remain honest construction metadata; the workflow does not fabricate a
human attestation or make them publication requirements. Historical releases
retain their original evidence and may identify different platform source
commits. New automatic construction fixes both platforms to one tag source.

The scripts `scripts/build-rewrite-candidate.py`, `scripts/release-plan.py`,
`scripts/promote-release.py` and `scripts/verify-release.py` own these contracts.
Mocked offline controls test stale/foreign/failed CI, source races, digest and
matrix mismatches, mixed attempts, partial uploads, published reruns, signature
failures and latest ordering. CI never runs real desktop input.

## Development and optional diagnostics

PR CI selects portable checks, macOS native contracts and Windows adapter
contracts from changed paths. The required aggregate fails on any selected
failure, cancellation or unexpected skip. An identical merged Git tree can reuse
its successful PR CI receipt; missing or mismatched evidence runs fresh checks.
Manual CI dispatch runs all checks. Toolchain/Cargo output caches remain outside
reserved candidate output; signing identities, profiles and archives are never
cached or restored into a candidate.

The candidate workflows still support explicit development dispatch and their
same-repository PR build labels. Those are optional diagnostics, not release
steps. A standalone build can also be made from a clean checkout:

```powershell
python scripts/build-rewrite-candidate.py `
  --target x86_64-pc-windows-msvc --output C:\Temp\dtw-candidate-windows
```

```sh
python3 scripts/build-rewrite-candidate.py --target aarch64-apple-darwin \
  --output /tmp/dtw-candidate-macos --signing-identity "Developer ID Application: Your Name (TEAMID)"
```

An output directory must be new and outside the checkout. Failed evidence is
retained. Optional local notarization uses an existing `--notary-profile` and
optional absolute `--notary-keychain`; the builder does not create credentials.
ZIP notarization is online and not stapled, so it is not a promise of offline
Gatekeeper or a substitute for users granting normal macOS permissions.

**Release asset verification** remains an optional read-only audit of an existing
tag and a post-publication event check. It does not publish anything or introduce
another gate. Real-machine troubleshooting tools and the optional Windows/native
Agent matrices are described in [building](building.md) and
[Windows acceptance](../tests/acceptance/windows/README.md). Their results can
inform fixes without requiring a manual release sign-off.
