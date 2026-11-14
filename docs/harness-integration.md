# Harness integration

DTW uses standard MCP tools and Agent Skills. Start `dtw mcp` through the Harness's normal stdio server configuration; see [installation](install.md). Default startup exposes `apps` and `exec({app, source})`. Opt in with `dtw mcp --atomic` for `apps` plus fourteen flat operations without JavaScript. Discover/list actual tools: if `exec` is present use scripts; otherwise invoke the listed app operation directly. The bundled Skill/API defines the same names and object fields for both: `look`, `capture`, `readText`, `wait`, `waitForNewWindow`, `click`, `setValue`, `move`, `drag`, `typeText`, `keypress`, `scroll`, `focus` and `quit`. Atomic arguments add required `app`. See [MCP toolsets](mcp-toolsets.md). It does not need custom Host code.

## Explicit target for policy inspection

Every `exec` request and atomic app-operation call contains the exact `app` identity returned by `apps`. The result echoes it. Harness policy can inspect this ordinary string before dispatching the tool call. Do not assume all identities are bundle IDs: discovery can distinguish duplicate installed applications with explicit paths. DTW owns startup where supported, instance/window routing and native references; both surfaces use the documented app methods rather than process IDs or a public `window_ref` argument.

The Harness is responsible for any approval, allow/deny policy or persistence of decisions. DTW does not accept approval assertions, create grants, request approval callbacks or renew authorization. The app ID is target identity rather than a secret or a permission token. DTW's capability description reports implementation support and execution conditions.

## Standard client behavior

Each script or atomic tool call waits for its execution result. Clients do not need to implement a custom submit/poll state machine. They can send standard MCP `notifications/cancelled` for an in-flight request and close stdin when disposing of the server. The response's `execution_id` identifies the execution. The JSON-RPC request ID belongs to the transport, not to app-operation arguments.

Preserve returned action receipts, including uncertainty. An exception after an action may leave the action applied. A lost connection cannot prove that an action did not occur. There is no public `result` or polling tool. An identical retransmission of a retained MCP request ID refers to its existing execution; a new request ID executes new work. Do not retry uncertain input as a new call. The server does not claim that a successful MCP response means a business task was completed.

The [MCP cancellation specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/utilities/cancellation) defines the standard notification and request-ID relationship. Annotations are presentation hints; use the Harness's existing policy mechanisms if a policy is required.

Each call settles its own focus/input scope. Keep explicit focus and follow-up
input in one `exec`; on Windows, opening a native menu and choosing its item must
also share that boundary because foreground restoration can dismiss the menu.
Separate atomic calls do not retain that scope.

## Session and operating-system boundaries

One MCP connection owns multiple app contexts. A→B→A preserves each app's committed
state and still-valid references. Separate connections isolate state and receipts;
Agents sharing a connection also share that state. The current connection executes
app executions sequentially. Native resource coordination remains inside DTW; clients do
not implement private grants, callbacks, renewal or a submit/poll state machine.

On macOS, Accessibility and Screen Recording permissions are OS prerequisites,
not Harness approvals. On Windows, host execution/file policy and desktop access
also constrain the launched process; see [DSH compatibility](install-dsh.md#windows-host-execution-and-storage).
Use only supported and authorized host execution paths. A denied operation must
follow the host's normal authorization process, not a sandbox-policy or ACL change. A returned app ID is target identity, never proof of user consent.
Parent exit, broken output and stdin EOF close native resources. Cancellation
cannot undo already delivered input; preserve uncertain receipts and independently
observe before deciding whether another action is appropriate.

Use ordinary tool discovery and load detailed schemas only when needed. The Skill
is a general operation/capability manual: reusable guidance for file pickers, forms,
dynamic controls and large trees belongs there; task-specific application recipes
do not. Caelis, DSH and agy setup uses the linked installation guides.
