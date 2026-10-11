# DTW thin CUA kernel POC

## Goal and selection

Useful desktop task completion takes priority over compatibility with today's DTW implementation. Preserve the product shape: App → Window → Element discovery, one `exec` MCP tool, scripts and session state, compact results with exact result lookup, background input when CUA can deliver it and controlled foreground input otherwise, no Node requirement, isolated Agent sessions, and a small installable plugin. The public script API is CUA's `cua` API for this POC; no `dtw.app` compatibility promise.

Pin upstream CUA Driver source at `ac967b9006aafa10870aadb5a645b038cba9721a` (MIT). The prototype links `cua-driver-sdk` as an `rlib`, calls a trusted immutable Session, and delegates JavaScript to CUA `run_script`. It does not port native AX/capture/input, expose the 59 upstream MCP tools, embed a second JavaScript engine, or use a Node runtime. A single private `cua-driver` worker is spawned directly by the SDK with inherited pipes, no reusable socket and no CuaDriver App. CUA owns native execution, native references, authorization and input routing. The adapter owns MCP framing, compact output/result retention and the optional JSON `state` handoff between script calls. Session ids are scoped to one MCP connection; a separate MCP process is the trust boundary for separate Agent hosts.

The private worker is selected over same-process `CuaDriver::create`: CUA explicitly returns `facility_unavailable` for the macOS cursor overlay in an arbitrary direct runtime. The worker is the upstream-supported overlay path. Its current SDK request path serializes worker calls behind one mutex. A cancellation request can be recorded immediately by the adapter, but this does **not** prove prompt interruption of a running worker action. The POC must report `cancel_requested` until CUA returns a terminal receipt; if prompt cancellation is a product gate, CUA needs a worker cancel channel or an SDK extension. Killing the shared worker would lose both Sessions and could leave action outcome unknown.

The adapter has no authority to grant macOS Accessibility or Screen Recording. In Bot integration Bot must directly spawn and own its private child, and the actual responsible Bot process must receive its own OS grants. In standalone Agent use the actual Agent host's responsibility chain and grants govern access. Direct `posix_spawn`/`Command` is required; launching a standalone CUA app through LaunchServices is outside this topology. TCC inheritance and overlays must be confirmed on a real signed host before migration.

## POC gates

Evidence labels: **source** means inspected behavior, **fixture** means automated isolated test, **live** means real app effect read back independently, **Agent** means a fresh Agent used this plugin and completed the task. No lower label implies a higher one.

| Gate | Required observation |
| --- | --- |
| Build/distribution | One `dtw` binary plus private `cua-driver`, one MCP `exec`, short Skill, MIT notice; starts with Node absent and without CuaDriver App. |
| Discovery | Real app/window/element discovery, exact window identity and screenshot; stale element rejected and fresh observation recovers. |
| Input | Semantic click/type, keys, menus, drag, background route and controlled foreground fallback have independent effect readback. No action on WeChat or private content. |
| Cursor | Visible virtual cursor succeeds via private worker, or exact `facility_unavailable`/permission blocker is reported. |
| Sessions | Two sessions maintain independent CUA snapshots and adapter JSON state. Shared foreground actions are serialized by CUA. A cancel request retains original execution id and terminal outcome; unknown is never replayed. |
| Output | Large AX tree and screenshot are bounded in model text, with explicit truncation and retrievable full result; key error code/message never removed. |
| Permissions/lifecycle | Refusal has no desktop action, host attribution is measured, process exit reaps its private worker, and no second App is needed. |
| Original failed demos | Fresh Agent session with only this thin plugin attempts isolated Chrome `READY-20261011` → `CLICKED-20261011`, Obsidian discovery/screenshot/Cmd+P without notes, and temporary WPS DOCX marker/save/textutil readback. These were planned but not performed in the earlier CUA evaluation because TCC was pending. |
| Platforms | macOS real app acceptance is required for this POC. Windows cross-build is only build evidence; no Windows GUI pass without a real Windows machine. |

Source pin and development location are temporary POC inputs. Proposed eventual artifact is `bin/dtw`, `libexec/cua-driver`, `skills/desktop-world/SKILL.md`, `mcp.json`, `plugin.json`, and `THIRD_PARTY_NOTICES.md`. Rebuild/sign nested worker under host packaging. Product migration, push, PR and release are out of scope.
