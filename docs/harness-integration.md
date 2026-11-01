# Harness integration

DTW uses standard MCP tools and Agent Skills. Start `dtw mcp` through the Harness's normal stdio server configuration; see [installation](install.md). The same server implements `apps`, `exec` and `result` for every client. It does not need custom Host code.

## Explicit target for policy inspection

Every `exec` request contains `app: "com.google.Chrome"` or another stable platform app ID returned by `apps`. The result echoes it. Harness policy can inspect this ordinary string before dispatching the tool call. DTW resolves all running instances for that stable ID. Window discovery aggregates them, and each nested operation remains bound to the exact native owner of its `window_ref`. Process details are not Agent inputs; the same script can operate across multiple instances.

The Harness is responsible for any approval, allow/deny policy or persistence of decisions. DTW does not accept approval assertions, create grants, request approval callbacks or renew authorization. The app ID is target identity rather than a secret or a permission token. DTW's capability description reports implementation support and execution conditions.

## Standard client behavior

The `exec` tool waits for its execution result. Clients do not need to implement a custom submit/poll state machine. They can send standard MCP `notifications/cancelled` for an in-flight request and close stdin when disposing of the server. The tool's `request_id` identifies the operation receipt, while the JSON-RPC ID identifies the transport request.

Preserve returned action receipts, including uncertainty. An exception after an action may leave the action applied. A lost connection cannot prove that an action did not occur. `result` reads existing receipts; it never repeats execution. The server does not claim that a successful MCP response means a business task was completed.

The [MCP cancellation specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/utilities/cancellation) defines the standard notification and request-ID relationship. Annotations are presentation hints; use the Harness's existing policy mechanisms if a policy is required.

## Session and operating-system boundaries

One MCP connection owns multiple app contexts. A→B→A preserves each app's committed
state and still-valid references. Separate connections isolate state and receipts;
Agents sharing a connection also share that state. The current connection executes
scripts sequentially. Native resource coordination remains inside DTW; clients do
not implement private grants, callbacks, renewal or a submit/poll state machine.

Accessibility and Screen Recording permissions are OS prerequisites, not Harness
approvals. A returned app ID is target identity, never proof of user consent.
Parent exit, broken output and stdin EOF close native resources. Cancellation
cannot undo already delivered input; preserve uncertain receipts and independently
observe before deciding whether another action is appropriate.

Use ordinary tool discovery and load detailed schemas only when needed. The Skill
is a general script/capability manual: reusable guidance for file pickers, forms,
dynamic controls and large trees belongs there; task-specific application recipes
do not. Caelis, DSH and agy setup uses the linked installation guides.
