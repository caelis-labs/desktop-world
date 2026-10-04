# Issue #6 native and browser evidence (2026-10-04)

Platform: connected macOS arm64 GUI; existing Accessibility/input grants were
used. The repository helper and opt-in native test binary were built locally.
No capture, anchor, permission or release-pin operation was requested. Real
Chrome used a new temporary profile under `/tmp` for the Bilibili and
Wikipedia test windows. No action was sent to the user's playing Bilibili
tab. The temporary profile and fixture application were removed after the
run. The final browser inventory showed that the original video tab was no
longer open; this run cannot establish when or why it closed. Names from the
live pages are omitted from logs; SHA-256 identifies the
same Wikipedia target in old and current helper runs.

## Build and test commands

From the repository root:

```sh
export GOWORK=off GOCACHE=/tmp/desktop-world-go-cache
go build -o bin/desktop-world ./cmd/desktop-world
go test -c -o bin/native-regressions.test ./tests/acceptance
./scripts/check.sh
go test ./tests/contract -run 'TestLarge(SlowAXScanResumesBeyondOldPrefix|AXModelBudgetIsCumulative)' -count=1 -v
```

`check.sh` executes `go test -race ./...`, `go vet ./...`, Windows
`GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build ./...` and `go vet ./...`,
`python3 verify_examples.py`, and `node --test clients/javascript/desktop.test.mjs`.
Its full raw output is
[check.txt](check.txt); 9 JSON examples and all 15 JS cases passed. The Go
module cache emitted a harmless write warning after passing.
The deterministic N=20,000 tests and 10,000-node cap are in
[fixture.txt](fixture.txt).

The following invocations used the already opened real Chrome tabs. The
Wikipedia target is the public AX link name `spent $954 billion on its
military`; it was selected in the later part of the page using the repository
helper, matched to CDP AX by SHA-256, and kept in a temporary local file for
the exact old/new comparison:

```sh
printf %s 'spent $954 billion on its military' > /tmp/desktop-world-issue6-target-name
DW_LARGE_SITE_TITLE='United States - Wikipedia' DW_LARGE_SITE_LABEL=wikipedia \
  DW_LARGE_SITE_TARGET_FILE=/tmp/desktop-world-issue6-target-name \
  DW_NATIVE_HELPER_PATH="$PWD/bin/desktop-world" \
  ./bin/native-regressions.test -test.v -test.run '^TestNativeLargeSiteDiscovery$'
DW_LARGE_SITE_TITLE='United States - Wikipedia' DW_LARGE_SITE_LABEL=wikipedia \
  DW_LARGE_SITE_BROAD_CAP=1 DW_NATIVE_HELPER_PATH="$PWD/bin/desktop-world" \
  ./bin/native-regressions.test -test.v -test.run '^TestNativeLargeSiteDiscovery$'
DW_LARGE_SITE_TITLE='哔哩哔哩 (゜-゜)つロ 干杯~-bilibili' DW_LARGE_SITE_LABEL=bilibili \
  DW_LARGE_SITE_MIN_VISITED=400 DW_LARGE_SITE_CHUNK=100 \
  DW_LARGE_SITE_SURVEY_CONTAINS='AI' DW_NATIVE_HELPER_PATH="$PWD/bin/desktop-world" \
  ./bin/native-regressions.test -test.v -test.run '^TestNativeLargeSiteDiscovery$'
```

Raw sanitized output: [Wikipedia exact](wikipedia.txt),
[Wikipedia broad output cap](wikipedia-broad-cap.txt),
[Bilibili SPA](bilibili.txt). The temporary-profile CDP count was 35,247
AX nodes for the Wikipedia article; CDP was diagnostic and did not supply
the native result Ref. No screenshot or anchor was used.

After the final `check.sh` and helper rebuild, the real-site tests were run
again in another isolated Chrome profile. The final helper's raw outputs are
[Wikipedia exact](wikipedia-final.txt),
[Wikipedia broad output cap](wikipedia-broad-cap-final.txt), and
[Bilibili SPA](bilibili-final.txt). Their results are the ones in the
[comparison table](../../large-ax-discovery.md#evidence-and-comparison).
The first attempt at that rerun used the restricted CLI sandbox and its
native AX call returned `permission_denied`, reproduced on the AppKit fixture
in [sandbox-native-denial.txt](sandbox-native-denial.txt). The successful
rerun used the same existing Accessibility grant in the approved GUI execution
environment. No system permission was changed.

For the old baseline, `cc50357` was exported into `/tmp` without changing
the working branch or release pin. The temporary test source is preserved as
[baseline_test.go.txt](baseline_test.go.txt); it makes one exact role/name
request through the old `cmd/desktop-world` helper. Its
[raw output](wikipedia-old.txt) shows 0 hits at 2,243 visited after 7.683 s,
with `ax_timeout` and no continuation. The target hash equals the current
Wikipedia test's target hash.

For the AppKit fixture, `./script/build_and_run.sh --build-only` built the
bundled app, then it was opened in the background with a unique title and
200 delayed AX controls. The exact opt-in test command was:

```sh
DW_NATIVE_FIXTURE_TITLE='Desktop World AX6 Final2' \
  DW_NATIVE_FIXTURE_LOG="$PWD/artifacts/issue6-final2-native.jsonl" \
  DW_NATIVE_HELPER_PATH="$PWD/bin/desktop-world" \
  ./bin/native-regressions.test -test.v \
  -test.run '^TestNative(ObserveTimeoutFixture|ValueFixture)$'
```

[Native fixture output](native-fixture.txt) includes independent value
witnesses and EndTurn revocation. A 200 ms timeout edge was repeated five
times after the short-deadline margin fix; all passed in
[native-timeout-repeat.txt](native-timeout-repeat.txt). This edge remains
sensitive to GUI scheduling: one repeat returned at 200.13 ms while still
preserving `ax_timeout` coverage. The final single-run output is
[native-timeout-final.txt](native-timeout-final.txt).
