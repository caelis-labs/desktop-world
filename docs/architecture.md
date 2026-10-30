# DTW architecture

The canonical architecture is [next-architecture-baseline.md](next-architecture-baseline.md). This page is the short implementation map.

```text
Agent / Harness tool policy
    | standard MCP tools + optional Agent Skill
    v
DTW Agent interface: apps / api / exec / result
    | explicit stable app ID + script source
    v
Agent session: multiple app contexts, JSON state, jobs, receipts, cancellation
    | typed operations bound to the selected window and running app instance
    v
Computer Core: target/reference checks, dispatch and effect evidence
    | native-provider contract
    v
DTW app workers (AX) + short-lived scene workers (capture/input)
```

DTW does not own approval or authorization policy. Its boundary is concrete target identity and execution correctness. The public `app` field is a stable platform ID string. PID and process generation belong to internal instance resolution, not the Agent interface. Providers retain their native objects and prove window ownership; Core rejects wrong-instance, stale or incompatible references before dispatch.

CUA is a source donor, not a runtime dependency. The fixed source baseline is recorded in [native-source-provenance.md](native-source-provenance.md). DTW uses its own operation types and script facade. It does not load the CUA SDK, registry or upstream worker, and does not expose a generic native-tool escape.

The script methods cover window discovery, bounded semantic observation, capture, press, coordinate click, focus, Unicode text, key chords, scroll and string setters. Methods are Promise-returning and execute through the same Core boundary. Script state commits only on success; desktop effects are not transactional. See [script v1](app-script-v1.md).

Logical app state belongs to the session, independently of native worker lifetime and historical receipts. Switching apps preserves state and refs. Completed executions use a sliding retention window; eviction never resets the latest app state or re-executes old work. Active work stays owned until it settles. See [script lifecycle](app-script-v1.md).

On macOS, an instance lease serializes writes to one process incarnation across MCP sessions. Directed CUA input and semantic AX writes for different apps can run independently. Keyboard validation follows app/window and key-target identity, not pointer movement or window position. Pointer-coordinate input additionally validates its geometric binding. A shared physical-input lease covers foreground fallback and its cleanup; the scene lease separately covers SCK lifetime. Background eligibility is checked before any preparation or input. Only a side-effect-free preflight decline permits foreground fallback. Once preparation or delivery starts, failure is reported without replay. Temporary foreground ownership is returned when still owned; external focus changes end that borrow.

WindowSurface and SemanticBinding are independent: inventory/capture/coordinate input retain system-surface identity even when an AX tree is unavailable. Native AX discovery is filtered and paged. Background input is conditional per operation/app, with the source closure and differences documented in [the CUA input port](background-input-port.md). Locked-session operation, cursor overlays, dialogs and remote transports remain separate, unverified capability groups.

C6 extends complete capability groups with their state ownership and cleanup. It does not require a new plugin framework or a split into more crates. The [product matrix](product-matrix.md) records actual support; dated acceptance reports establish evidence only for their named revisions and binaries.

The SDK/worker architecture formerly described at this path is historical. Its source is retained at [the last C5 baseline](https://github.com/caelis-labs/desktop-world/blob/d0d8279130b74a58496e13b7f6a4c4565f6f00cf/docs/architecture.md).
