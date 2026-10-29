# Connecting an Agent to DTW

Use the Agent's standard stdio MCP extension point with the executable and one argument:

```text
command: /absolute/path/to/dtw
args: [mcp]
```

The Agent may also load the ordinary [desktop-world Skill](../skills/desktop-world/SKILL.md). Configuration examples and local plugin staging are in [installation](install.md). There is no required DTW Host adapter, grant file, authorization callback, approval UI or renewal protocol.

## Responsibility boundary

The Harness owns tool approval and task policy. DTW is a tool: it receives a request carrying a stable platform app ID, validates its running instances, binds each window operation to its exact native owner, and reports the execution result. An app ID identifies a target; it is not a permission token or evidence of user consent.

The `app` argument is a string such as `com.google.Chrome`. `apps` discovers available IDs, `exec.app` requires one, and results echo it. Harness implementations can inspect that ordinary argument with their existing policy mechanisms. DTW resolves process instances internally; the Agent never supplies a PID, process generation or composite identity token. One app ID can resolve to multiple native instances: `app.windows()` aggregates their windows and each `window_ref` keeps its exact native owner. The script can operate across those windows without an instance parameter.

OS permissions, such as macOS Accessibility and Screen Recording, remain prerequisites of the native APIs. Their errors are execution diagnostics. DTW does not create a second tool-approval workflow around them.

## Ordinary MCP lifecycle

1. Initialize the stdio connection and discover tools with `tools/list`.
2. Call `apps` for app IDs, method descriptions and support conditions.
3. Call `exec` with only the chosen `app` ID and `source` script. It returns when the bounded execution settles.
4. Use `result` to inspect an original receipt after interruption or uncertainty. It is not required for normal successful execution.

Standard `notifications/cancelled` addresses the in-flight MCP request. EOF, broken output and parent loss close native resources. The server retains context-local state and references for app operations; closing the server invalidates them. Cancellation cannot undo desktop effects. If the server dies before the effect is known, preserve any returned execution identity and independently observe the application before deciding what to do next.

Independent MCP connections isolate their app state and receipts, but still share a physical desktop. DTW currently coordinates scene resources; full input coordination across its sessions is remaining DTW implementation work, tracked in the [release plan](first-rewrite-release-plan.md). The Harness owns approval policy, not DTW’s internal input scheduling. DTW does not claim to arbitrate unrelated desktop automation tools.

See the [protocol](app-protocol-v1.md) and [script semantics](app-script-v1.md) for precise request, receipt and reference behavior.

## Historical integration

The C4/C5 Host-grant prototype and local Caelis integration established evidence for their recorded revisions. They are not prerequisites for this entry. The prior requirement to implement app approval, write a grant and start a private single-app MCP process has been removed from the product contract. Historical reports remain available in [C4 acceptance](app-protocol-acceptance.md) and [C5 acceptance](c5-completion-acceptance.md).
