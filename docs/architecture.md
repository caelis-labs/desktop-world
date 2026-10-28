# DTW architecture

The canonical architecture is [next-architecture-baseline.md](next-architecture-baseline.md). This page is the short implementation map.

```text
Agent / Harness tool policy
    | standard MCP tools + optional Agent Skill
    v
DTW Agent interface: apps / exec / result
    | explicit stable app ID + script + request ID
    v
Execution context: bounded VM, jobs, JSON state, receipts, cancellation
    | typed operations bound to the same running App instance
    v
Computer Core: target/reference checks, dispatch and effect evidence
    | native-provider contract
    v
DTW supervised native worker: macOS AX + capture/input capability groups
```

DTW does not own approval or authorization policy. Its boundary is concrete target identity and execution correctness. The public `app` field is a stable platform ID string. PID and process generation belong to internal instance resolution, not the Agent interface. Providers retain their native objects and prove window ownership; Core rejects wrong-instance, stale or incompatible references before dispatch.

CUA is a source donor, not a runtime dependency. The fixed source baseline is recorded in [native-source-provenance.md](native-source-provenance.md). DTW uses its own operation types and script facade. It does not load the CUA SDK, registry or upstream worker, and does not expose a generic native-tool escape.

The current script subset is window discovery, semantic observation, window capture, semantic press and pixel click. Methods are Promise-returning and execute through the same Core boundary. Script state commits only on success; desktop effects are not transactional. See [script v1](app-script-v1.md).

C6 extends complete capability groups with their state ownership and cleanup. It does not require a new plugin framework or a split into more crates. The [product matrix](product-matrix.md) records actual support; dated acceptance reports establish evidence only for their named revisions and binaries.

The SDK/worker architecture formerly described at this path is historical. Its source is retained at [the last C5 baseline](https://github.com/caelis-labs/desktop-world/blob/d0d8279130b74a58496e13b7f6a4c4565f6f00cf/docs/architecture.md).
