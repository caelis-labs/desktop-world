# Harness integration

DTW exposes one MCP `exec` tool. The Harness owns user approval, policy, tool-call
cancellation and task isolation. DTW does not add a second approval UI or infer
human consent from Agent parameters. Treat this tool as computer-operation
capability: its conservative MCP annotations cover reads, writes and downloads.
A Harness that needs finer policy can inspect the nested operation/arguments;
MCP annotations alone cannot enforce that policy.

## Browser profiles and file scope

Standard startup supports desktop operations and explicitly requested isolated
browsers. Existing-profile access requires CUA's native bounded startup manifest.
The host selects the target and grants; the Agent discovers that configured target
through `exec {"operation":"describe"}`. It does not need a PID/window scout.

Launch with both native approval arguments:

```sh
dtw --capability-manifest /absolute/path/approved.yaml --approve-capability-manifest
```

Example native CUA v2 manifest (replace all example bindings/paths with the
host-selected live resources):

```yaml
version: 2
mode: bounded
expires_after: 20m
idle_timeout: 10m
allow:
  tools: [start_session, end_session, browser_prepare, get_browser_state,
          browser_type, browser_click, browser_download, health_report]
resources:
  browser:
    existing_profiles: [{pid: 12345, window_id: 67890}]
    origins: ["https://example.com"]
  files:
    write: [{dir: /absolute/task/downloads, recursive: false}]
```

DTW freezes the approved bytes before native worker initialization. CUA validates
and enforces tool/resource scope, live identity and expiry. Configuration discovery
is a convenience view, not fresh permission. Replacing the original manifest does
not widen an active Runtime. Custom DTW SCK `app`/`click` are unavailable in bounded
mode because they do not enforce CUA manifests. Bounded browser tasks with origins
also cannot use native `run_script` as a workaround.

A download `tools/call` admitted by the Harness carries the same trusted native
approval evidence as CUA's own MCP server. There is no DTW elicitation requirement,
no `approve` tool argument and no permission cache. CUA still validates exact
bindings, fresh refs and file scope. `destination_root` accepts a normal existing
absolute directory; DTW handles native path canonicalization, including Windows.

## Lifetime and current limits

One MCP process has one serialized job queue and script state. Independent tasks
need independent Runtimes or a host-managed shared-session policy. Idle context
recovery discards handles and requests re-observation; it never reloads the frozen
manifest or renews native grants. Cancelling an admitted action stops the worker
and leaves its effect unknown; pre-admission cancellation leaves it usable.

CUA 0.38's private-worker protocol cannot carry the embedded
`DriverAuthorizationHost` callback. Dynamic existing-profile dialogs therefore
remain unavailable on this route. The tested native manifest path can attach an
already available DevTools endpoint; automatic setup of ordinary Chrome still has
a native exact-AX-control matching failure on the test machine. See
[cua-native-authorization.md](cua-native-authorization.md) for evidence and limits.

Caelis trusts tools contributed by its installed `desktop-world` plugin while
retaining explicit policy and OS permission checks. A raw MCP server with a similar
name is not that plugin. Our acceptance uses an isolated Store and a host-configured
temporary package copy, without altering the user's installed plugin or settings.
