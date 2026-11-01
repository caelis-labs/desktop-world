# Building Desktop World from source

The source tree contains the rewrite only. Its supported slices and remaining work
are recorded in the [product matrix](product-matrix.md).

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

The native macOS CI lane runs both; non-macOS local runs explicitly skip them.
These controls do not compile the complete protocol, script engine or Rust native
bridge. Relevant changes also require the local build/tests below.

## Rewrite development build

No feature flag selects the rewrite. Build and test the normal package:

```sh
cargo +1.98.0 test --release --manifest-path cmd/dtw/Cargo.toml --locked
cargo +1.98.0 build --release --manifest-path cmd/dtw/Cargo.toml --locked --bin dtw
cmd/dtw/target/release/dtw --help
python3 tests/ci/run_native_worker_tests.py cmd/dtw/target/release/dtw
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
unknown binding; it does not touch AX, screenshots or desktop input. It requires the already-built executable. The shared Rust/MCP and native-worker
checks also run in CI; see [the CI/release lanes](releasing.md).

The MCP product entry is `dtw mcp`; `dtw app catalog` is a diagnostic CLI.
`dtw api` prints TypeScript generated from the same Rust schemas advertised by the
`apps` index and detailed `api` tool; `docs/app-script-api.d.ts` is the checked-in generated contract.
Connect it using standard Agent MCP configuration; see [installation](install.md).
Bare `dtw` and `--help` display help instead of starting an old MCP executor.

Library tests do not replace native acceptance through `dtw mcp`. Run affected
GUI controls on owned resources with independent effect readback. Prior evidence
applies only to its recorded source, binary and environment. Never run desktop
input probes automatically in ordinary CI.

Release-mode tests match candidate builds; unoptimized QuickJS can exhaust its
stack before deep-JSON contract checks finish. Debug builds remain useful for
local iteration, but are not the recommended full-contract test lane.

## Current external-client GUI probe

On a macOS test desktop, the opt-in standard probe builds two uniquely identified
owned fixtures and launches the selected binary through ordinary stdio MCP:

```sh
python3 tests/acceptance/standard_agent_probe.py \
  --dtw "$PWD/cmd/dtw/target/release/dtw" \
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

## Distribution

Explicit Windows and signed/notarized macOS candidate workflows build fixed
packages. Final acceptance runs those installed bytes and their package-owned
Skill. See [releasing](releasing.md) for candidate preservation and release checks.
Local staging with `scripts/stage-agent-plugin.py` is development evidence, not a
signed installed artifact or a publication.

## Native and Agent usability loop

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
A local development stage is not an installed-artifact acceptance result.


## Final installed three-app acceptance (macOS)

Use `tests/acceptance/real_app_tasks.py --task chrome|wps|obsidian --client dsh|agy|claude`
with a fresh `--output` per attempt and both `--dtw /absolute/package/bin/dtw` and
`--installed-package /absolute/package`. DSH/Claude additionally use their ACP
launcher. This keeps the installed executable path, stages only package-owned
Skill/metadata for the isolated Agent and rechecks the full package manifest after
cleanup. Omitting `--installed-package` is explicitly development evidence.
The runner is macOS-specific; Windows uses the bundled Windows matrix/workbench.
Caelis plugin/model acceptance is separate Core integration work.

## Real-task acceptance principles

Use disposable profiles, documents and vaults with unique markers. A supervisor
may prepare and independently inspect data; the acting Agent completes the task
through ordinary MCP and the package Skill, without shell editing, HTTP or DOM/CDP
shortcuts. Skill documentation covers reusable interactions, not task recipes.

| Application | Complete task | Independent oracle |
| --- | --- | --- |
| WPS Office | Create/edit multilingual paragraphs; save under a new directory/name; close and reopen | DOCX text/paragraphs, unchanged seed hash and reopened GUI readback |
| Obsidian | Create a disposable vault through its directory picker; create linked notes; search and reopen | Vault metadata, Markdown contents/links and GUI search/readback |
| Chrome | Find records across pages, edit fields/checkboxes/Unicode text, save, reopen and select a file | Server-side record/log readback, unchanged neighbouring records, selected filename/content |

Record onboarding errors and human hints; total time; MCP/exec/native calls;
output bytes; failed calls, refreshes and retries; and completion of every subgoal.
Separate model/network/environment failures from product defects without silently
removing attempts from the denominator. Native fixture, direct script and external
Agent evidence are different classes. Never replay an action with unknown effect.

Target clients are Caelis, DSH and agy; Caelis exercises deepseek-flash and
gpt-6.1-terra. Record complete tasks/attempts per application on the final installed
candidate. agy exploration efficiency alone is not a release blocker; reproducible
DTW defects remain actionable. Establish real-task cost baselines rather than
applying a tiny-fixture call limit to complex workflows. The external task runner's
900-second supervision budget is a test boundary, not a product script limit.

Batch related changes, run focused source/native controls, freeze source/binary/
Skill identity, then run real tasks. Attribute failures before changing the next
candidate; revalidate affected tasks and the final installed workflows. Recheck
package checksums after cleanup. Windows requires independent real-machine
acceptance using [its matrix](../tests/acceptance/windows/README.md); macOS evidence
never implies Windows, older OS, fresh-TCC, multi-display or Linux coverage.
