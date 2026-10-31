# Building Desktop World from source

The source tree contains the rewrite only. Its supported slices and remaining work
are recorded in the [product matrix](product-matrix.md) and
[implementation checkpoints](implementation-checkpoints.md).

## Fast development checks

With Rust 1.98.0 (including rustfmt) and Python 3.11+:

```sh
sh scripts/check-source.sh
```

This checks the Cargo/lockfile/version.txt versions, formatting, std-only Core/IPC
and Python acceptance negative controls. It needs no CUA checkout, Node, Cargo
dependency build, package or desktop. macOS contributors with Xcode can also run
the pure Swift inventory and input controls:

```sh
python3 tests/ci/run_native_inventory_tests.py
python3 tests/ci/run_native_input_tests.py
```

The existing macOS CI job runs both; non-macOS local runs explicitly skip them.
These controls do not compile the complete protocol, script engine or Rust native
bridge. Relevant changes also require the local build/tests below.

## Rewrite development build

No feature flag selects the rewrite. Build and test the normal package:

```sh
cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --locked
cargo +1.98.0 build --manifest-path cmd/dtw/Cargo.toml --locked --bin dtw
cmd/dtw/target/debug/dtw --help
python3 tests/ci/run_native_worker_tests.py cmd/dtw/target/debug/dtw
```

Cargo uses the locked DTW dependencies, including rquickjs; it does not fetch or
build the CUA SDK or upstream worker. Use a persistent `CARGO_TARGET_DIR` for local
iteration and adjust the executable path above when setting it. Native macOS
builds require macOS 14.2+ and Xcode/Swift. Native Windows and experimental Linux X11
providers are present. `python3 tests/native/check_provider_types.py --target x86_64-pc-windows-msvc`
(or `--target x86_64-unknown-linux-gnu`) narrowly
type-checks those providers and their tests using the production lockfile, without
linking the product or building QuickJS/CUA. This does not establish desktop support. The worker runner
checks TERM, EOF, malformed input and parent death using only release of an
unknown binding; it does not touch AX, screenshots or desktop input. It requires
the already-built executable and is a local integration check, not a CI build.

The MCP product entry is `dtw mcp`; `dtw app catalog` is a diagnostic CLI.
`dtw api` prints TypeScript generated from the same Rust schemas advertised by the
`apps` index and detailed `api` tool; `docs/app-script-api.d.ts` is the checked-in generated contract.
Connect it using standard Agent MCP configuration; see [installation](install.md).
Bare `dtw` and `--help` display help instead of starting an old MCP executor.

Library tests do not replace native acceptance through the current `dtw mcp` entry.
The historical [app protocol](app-protocol-acceptance.md),
[app script](app-script-acceptance.md), and
[window inventory](window-inventory-acceptance.md) reports define useful scenarios,
but their retired grant/Host commands are not current setup instructions.
Run relevant GUI checks when changing native behavior, on an owned test desktop
and with independent effect readback. Previous evidence applies to its recorded
source and binary, not automatically to a newly built executable.
The [C5 report](c5-completion-acceptance.md) also includes an opt-in mouse-pair
cleanup probe and real TextEdit window/Space matrix. These require owned apps,
serial desktop access and independent OS/effect observations; do not run them
automatically in CI.

## Current external-client GUI probe

On a macOS test desktop, the opt-in standard probe builds two uniquely identified
owned fixtures and launches the selected binary through ordinary stdio MCP:

```sh
python3 tests/acceptance/standard_agent_probe.py \
  --dtw "$PWD/cmd/dtw/target/debug/dtw" \
  --output /tmp/dtw-standard-agent-new-run
```

It records structured semantic press and independent fixture counters, PNG
capture, original receipts, stale/cross-app rejection, standard cancellation and
committed-state recovery, A→B→A retained window refs, and concurrent captures from two independent MCP sessions. Add `--pixel-click` only when the owned window can stay
foreground and unobscured. Output must be a new directory. The script closes its
exact server and fixture PIDs and retains evidence even on failure. It does not
install Agent configuration or retry actions. This proves a standard external
MCP client plus native GUI, separately from live LLM-host behavior. Historical
grant probes are marked in their file headers and are not current entry tests.

## Distribution status

A local Claude Code plugin can be staged with `scripts/stage-agent-plugin.py`;
see [installation](install.md). It copies the supplied binary and canonical Skill
without building, installing or publishing. Explicit Windows and signed/notarized macOS candidate workflows now exist;
a public rewrite release is still pending final installed-package acceptance. The old CUA package scripts
and plugin payload have been removed. Their historical procedure remains in Git at
[`c553486`](https://github.com/caelis-labs/desktop-world/tree/c55348610071e2b26e10ce490744065b0cdd08b7).
Do not use those old packages as rewrite acceptance evidence.
See [releasing](releasing.md) for the remaining release work.


## First release core usability loop

The existing fast lane also executes the std-only input sequencing regressions and
pure Swift surface/readiness/input controls. It still never compiles upstream CUA.
The optional provider checks above type-check Windows/Linux source and tests only.

Run the native input probe on the owned macOS test desktop before model tasks:

```sh
python3 tests/acceptance/native_input_probe.py --dtw /absolute/path/to/dtw \
  --output /tmp/dtw-native-input-new-case --case all
```

Then use `tests/acceptance/browser_workbench_probe.py` with `--client dsh` or
`--client agy` (the existing `claude` adapter remains available), `--dtw` and a fresh
`--output` directory. DSH takes `--acp-adapter` pointing to its configured ACP launcher. Claude additionally takes
`--acp-adapter /absolute/path/to/claude-agent-acp`; use the current official adapter
and installed native Claude executable. This probe opens only its disposable
Chrome workbench and TextEdit document, uses ordinary MCP/Skills, checks saves
independently, and retains failures. It must run serially with other GUI probes.
The exact task, metrics and progression gate are in the
[core acceptance record](release-core-acceptance.md). A local development stage
is not a signed release candidate or an installed-artifact acceptance result.


## Final installed three-app acceptance (macOS)

Use `tests/acceptance/real_app_tasks.py --task chrome|wps|obsidian --client dsh|agy|claude`
with a fresh `--output` per attempt and both `--dtw /absolute/package/bin/dtw` and
`--installed-package /absolute/package`. DSH/Claude additionally use their ACP
launcher. This keeps the installed executable path, stages only package-owned
Skill/metadata for the isolated Agent and rechecks the full package manifest after
cleanup. Omitting `--installed-package` is explicitly development evidence.
The runner is macOS-specific; Windows uses the bundled Windows matrix/workbench.
Caelis plugin/model acceptance is separate Core integration work.
