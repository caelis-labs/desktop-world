# Integrating the DTW app runtime

DTW currently provides a source-built executable and a versioned single-app MCP
protocol. There is no rewritten Agent plugin archive, Skill payload, installer
or embedded-runtime exporter. Hosts integrate the explicit CLI entry below;
launching bare `dtw` displays help rather than serving MCP.

## Approval and startup

The trusted Host owns the approval decision and process configuration:

1. Run `dtw app catalog` to discover app instances. Catalog access does not grant
   window reads, screenshots or input.
2. Present the exact app identity, PID, generation and requested capabilities to
   the Host's approval policy. Freeze the approved proposal before execution.
3. Create a private grant file with that exact instance, selected capabilities
   and an absolute expiry. Start one context with:

   ```sh
   /absolute/path/to/dtw app serve --grant /absolute/host-grant.json
   ```

4. Initialize MCP, discover its one `exec` tool and use `describe` to verify the
   context's app, capabilities and limits before submitting work.

The grant schema and loader rules are specified in [app protocol v1](app-protocol-v1.md).
On macOS the file must be a current-user-owned regular file with mode `0600`,
at most 16 KiB, and an expiry within 24 hours. A grant file that is itself a
symlink, or a grant that has expired, is rejected. Agent arguments cannot supply approval, change the instance or extend
capabilities/expiry. File permissions separate OS users; they are not a sandbox
against arbitrary code running as the trusted Host's user.

## Context and result ownership

One MCP process owns one approved app context, its script state, observations and
request records. Use separate contexts for independent grants. The runtime admits
one active request; a script uses the same slot and checks every Host Call through
Core. The Host still coordinates access to the shared physical desktop.

Use `submit` once with a unique request ID, then `result` with that same ID.
Completed means execution ended, not that the requested business effect was
confirmed. Preserve the action receipt and provider evidence; uncertain outcomes
must not cause an automatic replay. Scripts may have applied earlier actions
before an error; JSON state commit is not a rollback of desktop effects.

Use the protocol's `cancel` or `close` operations when withdrawing work or
revoking a context. MCP request cancellation alone does not cancel native work.
An accepted cancellation is terminal for the context, and a script's total timeout
also closes it. An expired grant cannot admit further work. Retain the original
receipts and perform a fresh approved observation before deciding on further input.
The Host owns MCP stdin/stdout and must close the process boundary when replacing
or revoking it; EOF and parent loss also initiate runtime shutdown. Cleanup errors
must remain visible. See [protocol lifecycle](app-protocol-v1.md) and
[script semantics](app-script-v1.md) for the exact receipt and shutdown rules.

DTW starts its own supervised native worker from the same executable. It does
not locate or unpack an upstream CUA worker. Native macOS execution is the only
implemented platform; consult the [product matrix](product-matrix.md) for actual
GUI coverage rather than inferring support from MCP initialization.

## Product integration and distribution

The Caelis production Host path has been exercised locally through real approval
and native GUI calls. Its accompanying changes remain local and unpublished;
that evidence does not establish availability in current Caelis/Bot releases or
other Agent hosts. A reference Host's prewritten grant is also not a substitute
for validating a product's actual approval flow.

Before distributing an integration, define the rewritten package contract and
verify the final bytes, permissions, child-process identity, lifecycle and actual
Host workflow on each advertised platform. No historical plugin layout or
installer is carried forward. The prior embedding contract is retained at
[`c553486`](https://github.com/caelis-labs/desktop-world/blob/c55348610071e2b26e10ce490744065b0cdd08b7/docs/embedding.md).
See [release checkpoints](releasing.md).
