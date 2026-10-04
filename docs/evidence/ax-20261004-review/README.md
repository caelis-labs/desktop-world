# PR #7 review follow-up evidence (2026-10-04)

This follow-up changed the shared cumulative outline-output cap and the macOS
native scan-capacity policy. It did not add Windows traversal resume support or
change the public coverage schema. All commands used the branch
`fix/large-ax-discovery` on the connected macOS arm64 GUI.

## Commands

```sh
export GOWORK=off GOCACHE=/tmp/desktop-world-go-cache
go test ./tests/contract -run 'Test(Large|OutlineWithoutNativeCursor|PaginationStableAndEnvelopeBounded|NativeTraversalRootSurvivesFirstPageAndURIIsOptIn)' -count=1 -v
./scripts/check.sh
go build -o bin/desktop-world ./cmd/desktop-world
go test -c -o bin/native-regressions.test ./tests/acceptance
./script/build_and_run.sh --build-only
open -n "$PWD/bin/DWNativeFixture.app" --args --title 'Desktop World AX6 Review' --log "$PWD/artifacts/issue6-review-native.jsonl" --slow-count 200
DW_NATIVE_FIXTURE_TITLE='Desktop World AX6 Review' \
  DW_NATIVE_FIXTURE_LOG="$PWD/artifacts/issue6-review-native.jsonl" \
  DW_NATIVE_HELPER_PATH="$PWD/bin/desktop-world" \
  ./bin/native-regressions.test -test.v \
  -test.run '^TestNative(ScanCapacityPreservesCursor|ObserveTimeoutFixture|ValueFixture)$'
# After the capacity fault's retry class was finalized, rebuild the helper
# and native test binary, then run the focused final check:
go build -o bin/desktop-world ./cmd/desktop-world
go test -c -o bin/native-regressions.test ./tests/acceptance
open -n "$PWD/bin/DWNativeFixture.app" --args --title 'Desktop World AX6 Review Final' --log "$PWD/artifacts/issue6-review-final-native.jsonl" --slow-count 200
DW_NATIVE_FIXTURE_TITLE='Desktop World AX6 Review Final' \
  DW_NATIVE_FIXTURE_LOG="$PWD/artifacts/issue6-review-final-native.jsonl" \
  DW_NATIVE_HELPER_PATH="$PWD/bin/desktop-world" \
  ./bin/native-regressions.test -test.v \
  -test.run '^TestNativeScanCapacityPreservesCursor$'
```

The app launch and native test ran in the approved GUI execution environment
using the existing Accessibility grant. No permission or browser setting was
changed. The fixture app was closed after the run. The full check includes Go
race tests, vet, Windows cross-build/vet, 9 protocol JSON examples and 15
JavaScript tests; all passed. The Go module stat-cache write warning did not
affect its exit status.

## Results

- [Focused contract output](contracts.txt): a normal non-resumable, unfiltered
  outline of 161 nodes stopped with `ax_output_limit` for page sizes 1 and 20
  (23 calls / 19,597 compact bytes and 8 calls / 16,744 bytes respectively).
  The 20,000-node delayed cursor and existing small-pagination tests passed.
- [Full check output](check.txt): exit 0.
- [Native fixture output](native.txt): 16 retained cursors; the seventeenth
  new scan returned `ax_scan_capacity`; the first cursor still advanced from
  one to two visited nodes. Timeout, value, and EndTurn tests passed.
- [Final native capacity rerun](native-capacity-final.txt): after the capacity
  fault gained `RetryClass:never_automatically`, the same 16-cursor and
  first-cursor-resume assertions passed using the rebuilt helper.
- [Fixture build output](fixture-build.txt): local AppKit bundle rebuilt and
  signed for the opt-in run.

No public-site timing run was repeated in this follow-up. The original real
Chrome comparison remains in [the issue #6 evidence](../ax-20261004/README.md).
