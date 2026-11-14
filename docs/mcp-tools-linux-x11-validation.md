# Linux X11 MCP validation

Fresh acceptance passed in both modes on 2026-11-14 at 10:59:11 +0800. The runner and both MCP children exited `0`; each fixture counter was exactly `1`, and both entry acknowledgements matched the typed token. [Machine-readable summary](evidence/mcp-tools-linux-x11/summary.json) includes discovery results, lock samples, fixture identity, image hashes and cleanup.

Candidate: `dtw 0.4.2`, source commit `0e0f4219ee1d3fca7b7cecabc16e4ed3cb1b9f45`, executable `/tmp/dtw-atomic-release-check-7rqurg6d/bin/dtw`, SHA256 `1a44cb38a3345c144180c6ce97b19a56b8b8af0d39002aef1cc3196c24313c9d`.

The runner is [`tests/acceptance/linux_x11_probe.py`](../tests/acceptance/linux_x11_probe.py). It requires Python 3, wish/Tcl/Tk, Xvfb, KWin X11, dbus-run-session, xprop, xdpyinfo and loginctl. The exact invocation was:

```sh
python3 tests/acceptance/linux_x11_probe.py \
  --session 7 --isolated \
  --binary /tmp/dtw-atomic-release-check-7rqurg6d/bin/dtw \
  --sha256 1a44cb38a3345c144180c6ce97b19a56b8b8af0d39002aef1cc3196c24313c9d \
  --artifacts /tmp/dtw-linux-x11-final-0e0f4219-artifacts \
  > /tmp/dtw-linux-x11-final-0e0f4219.jsonl 2>&1
```

For reproduction, use a new artifact directory and output log. The runner checks the candidate hash before execution and exits nonzero on a failed round or lock check. It runs both modes by default; repeatable `--mode script` / `--mode atomic` select runner rounds. The product commands are `dtw mcp` (exactly `apps` and `exec`) and `dtw mcp --atomic` (`apps` and fourteen canonical methods: `look`, `capture`, `readText`, `wait`, `waitForNewWindow`, `click`, `setValue`, `move`, `drag`, `typeText`, `keypress`, `scroll`, `focus`, `quit`). Both discovered surfaces matched exactly.

Each round used a fresh owned Tk fixture and a separate persistent MCP child/connection. Default scripts used `await app.method({...});` to publish the normal view and effects; atomic tools received the same objects plus `app`. The unique copied wish executable, exact PID/class identity, script-first startup and Tcl/Tk libraries identified the owned fixture. Independently reported root client/widget geometry mapped capture pixels to widget centers: client bounds `[50,78,460,240]`, PNG `920×480`.

Capture, pixel input and readback occurred in separate MCP calls on the same connection. A captured button click produced `BUTTON=1`; subsequent look/capture and independent state confirmed counter `1`. A fresh entry capture, pixel click, `typeText({text:token})` and `keypress({keys:['ENTER']})` produced the exact ACK below. Final look/capture, fixture stdout and independent state agreed; the counter remained `1`. No input was replayed.

| Mode | Counter | Entry and ACK token | MCP exit |
| --- | --- | --- | --- |
| Default script | 1 | `dtw_probe_script_731` | 0 |
| Atomic | 1 | `dtw_probe_atomic_731` | 0 |

These three actual fixture PNGs were visually inspected: the initial atomic image shows an empty entry, `Count=0` and `ACK pending`; both final images show `Count=1` and the exact entry/ACK token.

| Image | SHA256 |
| --- | --- |
| [Atomic initial](evidence/mcp-tools-linux-x11/atomic-initial.png) | `48f10485f294611c16cc3ad02cdc800250a952ca0425cf86196ee8f072e32cb8` |
| [Atomic final](evidence/mcp-tools-linux-x11/atomic-final.png) | `5ccc65192769590bd19d3a64ae74a39738a6c4f443dbf1a793fb95226c13435c` |
| [Script final](evidence/mcp-tools-linux-x11/script-final.png) | `d9a2881173ba6984a2962d356cb7829f059e7805af06f08319f0e9cdae58fda0` |

Both `loginctl show-session 7` samples returned status `0` with `Id=7`, `Display=:0`, `Type=x11`, `LockedHint=yes`. Real `:0` received server/extension metadata queries only; no app discovery, window pixels or input occurred there. All app actions used fresh Xvfb `:1` with private KWin configuration/runtime and D-Bus session. Cleanup terminated only the six owned process groups, reaped all leaders, and removed `/tmp/dtw-locked-probe-w83ecku4`. MCP children exited normally; fixture/WM termination was expected cleanup. PNGs, raw MCP requests/stdout, fixture stdout/state and child logs remain in `/tmp/dtw-linux-x11-final-0e0f4219-artifacts`; the complete run log remains at `/tmp/dtw-linux-x11-final-0e0f4219.jsonl`. This frozen candidate passed its first fresh invocation; there were no failed attempts in this run.

## Contract and package checks

The same source passed `cargo test --release --manifest-path cmd/dtw/Cargo.toml --locked`: 558 library tests and 5 CLI tests. This includes canonical object-argument parity, integral JSON float/exponent handling, zero atomic JS-engine entries with a live script control, retained pixel/semantic bindings, stale-target rejection, cancellation during dispatch/cleanup, and retransmission. `sh scripts/check-source.sh` and formatting checks passed.

Package-layout/archive tests passed for both supported target layouts using explicit fixtures: `test_plugin_package.py` (5 tests) verifies canonical Skill/API/PATTERNS bytes, default manifests, archive hashes and altered-file rejection; `test_packaged_mcp.py` (9 tests) covers dual-mode metadata and negative controls. `scripts/release-artifacts.py check` confirmed the unchanged 0.4.2 version metadata.

The frozen Linux binary also passed `tests/ci/run_packaged_mcp.py --dtw <candidate> --version 0.4.2 --output <new-directory>`: default discovery listed 2 tools, atomic discovery listed 15 canonical tools without `exec`, and missing-app calls were rejected before native work. Version/default/atomic children all exited 0 with no forced termination and all pipe workers joined. The compact summary includes these results; the raw headless report is retained at `/tmp/dtw-atomic-headless-formal-packaging-81968c6b/report.json`.

These layout fixtures and Linux headless checks do not qualify native macOS/Windows packages. No macOS/Windows native build, signing, notarization, release publication or native GUI qualification was performed.

Linux look explicitly reported `semantics.complete:false`, `reason:unsupported_capability`, and no semantic elements: AT-SPI semantics remain unsupported. GUI effects exercised `look`, `capture`, `click`, `typeText` and `keypress`; discovering the remaining methods does not establish their native behavior. Pixel acceptance establishes only this owned fixture on isolated X11. Mocked semantic regressions are not GUI evidence, and this result establishes neither locked-desktop app operation nor macOS/Windows behavior. Full raw data and other PNGs are retained locally under `/tmp`; the repository contains only the compact summary and three selected PNGs.
