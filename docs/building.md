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
The public contract is the packaged `skills/desktop-world/SKILL.md` and its
`API.md` reference. MCP exposes only `apps` and `exec`; the retired `dtw api`
command is unavailable. The private atomic DSL has a test fixture under
`tests/fixtures/atomic-script-api.d.ts`; it is not an Agent interface.
Connect it using standard Agent MCP configuration; see [installation](install.md).
Bare `dtw` and `--help` display help instead of starting an old MCP executor.

Releases use the automatic CI, fixed-source construction and publication path in
[releasing](releasing.md), including headless final-package MCP/EOF checks. GUI
and Agent tasks below are optional diagnostics, not release publication gates.
Library tests do not establish GUI behavior. Run affected optional
GUI controls on owned resources with independent effect readback. Prior evidence
applies only to its recorded source, binary and environment. Never run desktop
input probes automatically in ordinary CI.

Release-mode tests match candidate builds; unoptimized QuickJS can exhaust its
stack before deep-JSON contract checks finish. Debug builds remain useful for
local iteration, but are not the recommended full-contract test lane.

## Current external-client GUI probe

On a macOS test desktop, the current app-scene probe builds a uniquely identified
owned application and exercises the two advertised tools through real stdio:

```sh
python3 tests/acceptance/app_scene_probe.py \
  --dtw "$PWD/cmd/dtw/target/release/dtw" \
  --output /tmp/dtw-app-scene-new-run
```

It checks discovery/admission, stopped-app startup, overlapping surfaces, popup
hit-testing, stale geometry rejection, normal window close, save sheets, a running
app without windows, application shortcuts, background-only refusal and restart.
Its independent AppKit state records actual events. It never retries input.
The separate `app_scene_readonly_probe.py` reads already-running Finder, Chrome
and WPS, redacting UI payloads; absence/unsupported coverage stays explicit.
These are native/script evidence, not real model/Harness A/B tests or proof that
all app families are supported. Output directories must be new.

The AX compatibility probe creates providers whose count/range methods fail,
whose children are unreadable, or whose AX window inventory is unavailable:

```sh
python3 tests/acceptance/ax_compat_probe.py \
  --dtw /absolute/path/to/dtw --output /tmp/dtw-ax-compat-new-run
```

It reads actual button, text, wheel, drag/release and normal-quit effects through
`apps`/`exec`; it also rejects foreign occlusion and stale coordinates. Optional
`--baseline /absolute/path/to/old/dtw` reproduces the original count-method
failure before input. This probe uses the current physical pointer routes and
records the real system pointer during drag; it is not a no-interference or
model/Harness benchmark. Each disposable app is a directly owned child and is
cleaned up even when a case fails. Use a fixed candidate executable so another
build cannot replace worker bytes during the run.

For a read-only independent system-cursor oracle around either current GUI probe:

```sh
xcrun swiftc tests/fixtures/cursor-observer/main.swift -o /tmp/dtw-cursor-observer
python3 tests/acceptance/cursor_audit.py \
  --observer /tmp/dtw-cursor-observer --output /tmp/dtw-cursor-new-run -- \
  python3 tests/acceptance/app_scene_probe.py \
    --dtw /absolute/path/to/frozen/dtw --output /tmp/dtw-cursor-new-run/probe
```

Keep probe output inside the new audit directory. The observer posts no input,
reads the actual global cursor throughout each original MCP call and records
sample gaps. A restored endpoint does not pass as cursor independence. A
cancelled request without a reply stays explicitly unreceipted; its cleanup
interval ends at the first successful recovery read. Sampling cannot exclude
shorter excursions or attribute ambient user motion by itself. Match each
detected change to native delivery code and fixture effects.

`directed_pointer_probe.py` is a separate owned AppKit feasibility experiment;
it is not a product route. Wrap it with the same observer, adding `--peer` to
keep another owned app frontmost. It reports click, hover and drag effects
separately and returns failure if any effect is missing. In particular, successful
click/drag delivery does not establish hover or arbitrary-app support.

The current lifecycle/LSUIElement runner uses the same two-tool protocol:

```sh
python3 tests/acceptance/app_lifecycle_probe.py \
  --dtw /absolute/path/to/dtw --output /tmp/dtw-lifecycle-new-case
```

It covers accessory application discovery/input, nested-helper exclusion, normal
quit/save prompts, restart, and cancellation followed by an ordinary read. The
Windows current-protocol entry is `tests/acceptance/windows/app_protocol_probe.py`;
see its [handoff](../tests/acceptance/windows/README.md).

The old `standard_agent_probe.py`, `browser_workbench_probe.py`,
`real_app_tasks.py` and related atomic-protocol runners retain historical evidence
and reusable fixtures/transports. They are not current acceptance commands;
their Session/result/window-ref workflows do not establish the public facade.
Internal ownership, cancellation and resource tests stay in the source/Rust suites.

## Distribution

Explicit Windows and signed/notarized macOS candidate workflows build fixed
packages. Final acceptance runs those installed bytes and their package-owned
Skill. See [releasing](releasing.md) for candidate preservation and release checks.
Local staging with `scripts/stage-agent-plugin.py` is development evidence, not a
signed installed artifact or a publication.

## Boundary acceptance after implementation

Complete the code changes first, then run the source/release-profile checks and
current native protocol probes together. Real application boundary acceptance
follows those checks. Run GUI probes serially on disposable owned windows,
profiles and documents. Preserve original failures and read back unknown effects;
never retry input solely because its response is missing.

For Chrome, explicitly exercise real translation/menu overlays, stacked windows,
keyboard destination and image fallback. A generic AppKit panel or DOM dialog
cannot substitute for that native browser surface. Real TextEdit/WPS/Obsidian
workflows require independent saved-file/control evidence. The matrix below is
a task specification, not a claim that the new protocol has passed those apps.
No model/Harness A/B is required for this protocol work. Native protocol metrics
record source/response bytes, image counts and elapsed time separately.

Fixed-source development candidates and installed-package acceptance come after
these implementation/regression steps. Caelis consumer/model integration and
final published-asset installation remain separate evidence boundaries.

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
package checksums after cleanup. Optional Windows real-machine
diagnostics use [its matrix](../tests/acceptance/windows/README.md); macOS evidence
never implies Windows, older OS, fresh-TCC, multi-display or Linux coverage.
