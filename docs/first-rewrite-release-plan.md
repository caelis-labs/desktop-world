# First rewrite release: v0.2.0

Historical v0.2.0 scope and delivery plan, recorded for #70 review. v0.2.0 has
since been published; current CI and release gates are in [releasing.md](releasing.md).
The original #63/#64 audit is preserved in Git; the current
[product matrix](product-matrix.md) and [real-task evidence](real-task-closure-acceptance.md)
describe implementation and actual validation separately.

## Scope and responsibilities

The first version is the rewrite only: standard rmcp stdio + Skill, stable app ID,
`exec({app, source})`, window inventory/capture, semantic and coordinate input,
text/shortcuts/scroll, bounded discovery and text reading. Agents can alternate
between apps and retain their contexts. DTW owns native identity, observations,
focus, input coordination and cleanup. The Harness owns approval. CUA capability
source is ported in bounded groups with thin adapters; no SDK/worker compatibility
path or full-upstream CI is restored.

| Platform | Current state | Release gate |
| --- | --- | --- |
| macOS arm64, minimum 14.2 | Native implementation and signed development candidates; DSH Chrome/WPS/Obsidian tasks completed on candidate Z; final AA received targeted native checks | Build/sign/notarize the final package; run from its installed path with its Skill and checksum inventory, then publish those same bytes |
| Windows x64 | Native UIA/GDI/SendInput provider; #70 review fixes normal UIA tree termination, retained region constraints and text matching | One same-source ZIP; user independently runs the bundled blocking checks, desktop matrix and three real applications |
| Linux X11 | Experimental source/type-check baseline | No formal desktop support or release commitment; independent validation later |
| macOS x86_64 / Windows ARM64 | Outside the initial supported pair | Separate evidence before extending support |

Windows background input, an independent Windows cursor, lock-screen operation,
remote MCP, DOM/browser bridge, drag and app launch/quit remain deferred. They do
not block the agreed foreground Windows route. No current candidate establishes
all macOS versions, fresh-machine TCC behavior or all helper/modal configurations.

## Remaining delivery sequence

1. **Review fixes and Windows handoff.** Correct successful-null UIA results while
   retaining real read failures as partial. Revalidate capture geometry/DPI and
   retained element-region constraints before semantic input and text reads.
   Match label/name/value independently. Protect these with native boundary tests
   and an expanded WinForms smoke. Bundle the UTF-8 Chrome workbench, launch and
   independent readback instructions. Trigger one explicit Windows candidate build;
   ordinary PR CI uses the bounded checks in [releasing.md](releasing.md).
2. **Independent Windows acceptance.** First test a unique selector/one button
   increment, moved-window/control stale refs with zero effect, Unicode text pages
   and two app instances without cross-writing. Then test DPI/multiple screens,
   cancellation/parent exit/key release/worker cleanup and WPS/Obsidian/Chrome.
   Use [the bundled matrix](../tests/acceptance/windows/README.md). Compilation and
   mock results never substitute for these desktop checks.
3. **Caelis plugin integration and final macOS package.** Connect the same package
   through Caelis's existing plugin manager. Core asks on the first `exec.app` per
   Session and reuses that app's grant within that Session; denial dispatches
   nothing, and a new Session starts without those grants. No DTW authorization
   API is added. Validate `deepseek-flash` and `gpt-6.1-terra` for tool/Skill/image
   delivery, same/new app decisions, separate Sessions and real task readback.
   Run the final macOS three-app suite with `--installed-package`, checking the
   original executable path, package Skill and unchanged manifest before/after.
4. **Publish verified bytes.** Record source/tree, toolchain, dependency notices,
   hashes, signature/notarization and platform results. Only after Windows and
   macOS gates pass may the same archives be promoted as dual-platform v0.2.0.
   A later runtime or Skill change requires updated candidate evidence.

## Agent acceptance policy

Target Agents are **Caelis, DSH and agy**. DSH's ordinary MCP + Skill route requires
no dedicated DTW plugin. Caelis's per-app Session policy is implemented in Core #187, with standard
plugin and large-result fixes from #185; it is not DTW authorization logic. Caelis's two named models and DSH must
complete the selected workflows on their recorded installed candidate. agy remains
an interoperability/usability sample: reproducible DTW defects block and must be
fixed; model exploration or judgment inefficiency alone does not. Preserve every
original failed attempt and its cost. Historical Claude results remain evidence,
but the old mandatory Claude/agy six-run gate is replaced by this target matrix.

For each app record total time, MCP/exec calls, failures, read-only refreshes,
images/JSON bytes, business readback and process/input cleanup. Unknown effects are
not replayed. The general Skill documents APIs and reusable forms/chooser patterns,
never task-specific WPS/Obsidian recipes. The earlier generic fresh-capture failure
still has an unproven natural cause; only the narrow proven geometry-recovery
contract is established. See [release construction](releasing.md) and
[artifact-specific evidence](rewrite-candidate-acceptance.md).
