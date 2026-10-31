# DTW tool loading evaluation — 2026-10-31

Recommendation: let an enabled plugin request `auto`, `eager`, or `deferred`
loading, with Host/user policy making the final decision. DTW's four small
entrypoints are a good eager candidate. This is an assessment, not an implemented
manifest extension; no loading or approval policy is changed by the Windows PR.

## Measured DTW surface

The standard `tools/list` of the reviewed Windows package contains only `apps`,
`api`, `exec`, and `result`. Detailed native APIs are returned on demand by `api`,
not expanded into dozens of function schemas. Using Caelis PR #182's
`EstimateDefinitionPromptTokens`, projected names, the external-description
prefix and the listed schemas gives these **estimates**, not provider tokenizer
measurements:

| Tool | Estimated prompt tokens |
| --- | ---: |
| apps | 139 |
| api | 181 |
| exec | 397 |
| result | 278 |
| Sum of individual estimates | 995 |

The raw declarations before Caelis's external-description prefix total 910.
The three reviewed Flash tasks each used one ToolSearch and then respectively
37, 58 and 30 MCP calls (Chrome, WordPad, Explorer). The PR #182 WordPad integration
used one ToolSearch and 47 MCP calls. This supports an eager default for a user
who has enabled DTW, while providing no evidence for making all MCP catalogs eager.
These are observations from the acceptance run, not an eager/deferred A/B result.

## Caelis and cache implications

At Core `3c3e09f3`, `agent-sdk/tool/visibility.go` initially defers MCP definitions
when ToolSearch is available. Discovery admits exact registered bindings;
`NewToolSearchDiscoveredTool` returns names/bindings and the next model request
supplies the schema. `ModelSpecs` and the retirement/exposure state own the
visible subset. Thus revealing a new tool changes the model-facing tools list.
The existing catalog lifecycle pins each response/batch, rejects obsolete
handles, preserves grants/revocation, and rebuilds exposure from durable events.
An eager policy must reuse those owners rather than bypassing them.

The user's cache concern is valid for this request construction. Tool definitions
and ordering affect OpenAI's cached prefix; changing them can invalidate that
prefix. The precise lost region and monetary impact depend on the provider's
serialization and cache state. [OpenAI's caching guide](https://developers.openai.com/cookbook/examples/prompt_caching_201)
supports keeping the toolkit stable. DeepSeek documents matching persisted
prefix units and exposes hit/miss usage; this run does not establish a universal
"one search destroys the entire cache" rule or a measured savings percentage.
[DeepSeek cache contract](https://api-docs.deepseek.com/guides/kv_cache/).

ToolSearch also has selection/round-trip cost. Caelis's default selector is a
private model operation, in addition to the main model choosing to call it.
Eager loading trades that cost and first-reveal list mutation for a small fixed
schema cost on requests where DTW is unused. After discovery, current Caelis does
not search before every `exec`; repeated tool calls are not repeated searches.

## Open-source comparison

| Harness snapshot | Observed implementation | Implication |
| --- | --- | --- |
| Codex CLI main `67f0994b532b8740f14b5b738af7b7b2a915b1d6` | Host computes direct/deferred/code-mode exposure. `omit_tools_from` on MCP/app configuration removes surfaces; model capability and strict Code Mode can override the preference. | Exposure is Host policy, not an unconditional remote-server command. |
| Same Codex snapshot | Discovered schemas are emitted as typed `ToolSearchOutput` history items. | Do not equate this path with Caelis adding schemas to its top-level function list. Search still has cost; this architecture does not by itself prove a cache miss. |
| OpenCode dev `bf544cd419c5f17535c0d5af929f7ecd41188320` | MCP tools default into Code Mode; `codemode:false` makes them direct. `Tool.snapshot` sorts direct definitions and caches the Code Mode inventory/definition by registry revision and visible names; without Code Mode it exposes active tools directly. | There is an explicit direct-versus-aggregated choice, distinct from Caelis's ToolSearch reveal. |

Pinned primary sources:
[Codex exposure policy](https://github.com/openai/codex/blob/67f0994b532b8740f14b5b738af7b7b2a915b1d6/codex-rs/core/src/tools/spec_plan.rs#L192),
[Codex typed discovery result](https://github.com/openai/codex/blob/67f0994b532b8740f14b5b738af7b7b2a915b1d6/codex-rs/core/src/tools/context.rs),
[OpenCode MCP registration](https://github.com/anomalyco/opencode/blob/bf544cd419c5f17535c0d5af929f7ecd41188320/packages/core/src/tool/mcp.ts#L50),
[OpenCode snapshot](https://github.com/anomalyco/opencode/blob/bf544cd419c5f17535c0d5af929f7ecd41188320/packages/core/src/tool.ts#L265),
[OpenCode nested Code Mode defaults](https://github.com/anomalyco/opencode/blob/bf544cd419c5f17535c0d5af929f7ecd41188320/packages/core/src/plugin/mcp-codemode-defaults.ts).
OpenCode's public MCP documentation describes automatic tool availability and
context cost; it is insufficient to infer that this newer dev snapshot injects
every individual MCP schema directly. [Public documentation](https://docs.opencode.ai/docs/mcp-servers/).

## Standard declaration entrypoints

Agent Plugins **1.0.0** supplies an appropriate carrier, not standardized loading
semantics: `plugin.json` has reverse-domain `extensions`, with object values owned
by each client. Unrecognized namespaces must be ignored. Root core fields and
the standard `mcp.json` server objects are closed; adding `deferred` there is not
conformant. [Manifest extension rules](https://agent-plugins.org/specification#81-manifest-extension-data),
[MCP configuration schema](https://agent-plugins.org/schemas/1.0.0/mcp.schema.json).

MCP **2026-07-28** has no core `auto/eager/deferred` tool property. Vendor-namespaced
`Tool._meta` can carry custom metadata, but provides no portable loading behavior.
OpenAI's `defer_loading` is a provider API field, not that MCP property.
[MCP schema](https://modelcontextprotocol.io/specification/2026-07-28/schema#tool),
[MCP metadata](https://modelcontextprotocol.io/specification/2026-07-28/basic/index#meta),
[OpenAI Tool Search](https://developers.openai.com/api/docs/guides/tools-tool-search).

Prefer **one** declaration source for v1: the recognized client extension in the
installed plugin, plus an explicit Host/user override. Do not also interpret a
remote MCP `_meta` loading hint in the first implementation. Besides its weaker
authority, changing such a hint may change the remote definition fingerprint,
entangling a visibility preference with source refresh. Pure standalone MCP
servers can use Host configuration; standard interoperability stays unchanged.

A proposed extension payload, nested under the client's controlled reverse-domain
namespace (not a new top-level field), is:

```json
{
  "toolLoading": {
    "version": 1,
    "servers": {
      "desktop-world": {
        "default": "auto",
        "tools": {
          "apps": "eager",
          "api": "eager",
          "exec": "eager",
          "result": "eager"
        }
      }
    }
  }
}
```

Keys match the plugin-local server key and exact remote tool name, before Host
name projection. Unknown names do not create tools. The example is a proposed
Caelis extension schema; current Caelis does not interpret it yet.

## Proposed Caelis contract

- A versioned, namespaced **plugin loading preference**, server default plus
  exact per-tool overrides, with three values `auto/eager/deferred`. This is a
  Caelis extension, not a standard MCP `defer_loading` field. Prefer a typed
  preference over a boolean so users retain the Host's automatic choice.
- Precedence: user/organization policy, Host capacity/model capability, then the
  enabled plugin's preference. A remote `tools/list` annotation or `_meta` value
  cannot force eager loading. Enforce existing definition limits and a separate
  eager schema budget; explain a declined preference in status diagnostics.
- For DTW, request all four entrypoints eager. Keep detailed `api` descriptions
  demand-loaded. DTW already implements a compact app-scoped execution surface;
  wrapping it in another code interpreter adds little value.
- Resolve preference against the accepted catalog and persist/reconstruct it
  through the existing exposure mechanism. Keep order and identical definitions
  stable across requests. Pin the callable and schema for a complete response
  batch; catalog changes/revocation still invalidate stale handles. A late-ready
  server or genuine schema change may still require a list update.
- Loading affects visibility only. Source approval, per-Session app grants,
  explicit Ask/Deny, resource identity, cancellation and result truncation keep
  their existing authority. The plugin cannot grant itself access.
- `eager` controls **initial exposure**, not permanent pinning or retirement
  immunity. The existing exposure owner must commit and replay that initial
  admission. It must also distinguish "bootstrap not applied" from "bootstrap
  applied and the active set is now empty". An empty post-retirement set must
  never trigger re-seeding. Do not keep a second registry of active tools.

### Default behavior and lifecycle isolation

For v1, absent configuration is exactly `auto`, and `auto` preserves the current
Host behavior: MCP tools are deferred when ToolSearch is available, otherwise
exposed through the existing compatible direct path. It is not a new heuristic
that quietly changes old plugins. Unknown extension versions or invalid loading
values should produce a localized diagnostic and fall back to `auto`, without
disabling otherwise valid standard plugin components. Explicit per-tool choice
overrides the server default; Host/user choice overrides the plugin. A requested
deferred mode must not leave tools unreachable on a Host without discovery.

Keep three responsibilities separate:

| Owner | Owns | Loading preference must not do |
| --- | --- | --- |
| Plugin/Host policy resolver | Versioned preference and schema budget for initial exposure | Invent capabilities, grant app access, change remote source identity |
| MCP Manager | Current accepted catalog, source versions, ready/unavailable state, connection handles | Change its refresh/reconnect behavior because a tool is eager |
| Existing Session exposure lifecycle | Exact active/pending bindings, durable retirement, compaction/replay | Resurrect retired tools from the loading default |

Apply the initial policy at a new Session's bounded startup/admission boundary,
using the accepted ready catalog. Persist its one-time application through the
existing exposure owner. Resume restores durable exposure rather than rerunning
bootstrap; old Sessions are not retroactively opted in. Configuration changes
take effect for a new Session or an explicit future policy-reapply operation,
not through `tools/list_changed` or every model-request preparation.

An ordinary refresh keeps its existing semantics for already exposed tools and
uses current definitions/handles. New unexposed tools continue normal discovery
for that Session; an eager server default is not a request to inject every later
addition. Removal/disconnection makes a tool unavailable immediately. A same-name
replacement never inherits the previous callable or grant from its loading
preference. Retirement still commits only at its existing durable checkpoint;
later refresh, reconnect, replay or a changed `policyRevision` cannot silently
undo it. Rediscovery follows the existing explicit ToolSearch path.

Loading policy has its own revision. It must not be mixed into remote MCP schema
fingerprints, emit catalog-change notifications, restart servers, revoke source
grants, or rewrite past tool events solely because the preference changed.
Security-relevant source/schema changes continue their existing invalidation.
Stable ordering is preserved when the effective tool set is unchanged; actual
additions/removals may still alter the cached prefix.

Required integration checks include: absent extension behaves identically;
one-time eager admission; retirement followed by empty exposure, compaction,
resume, unrelated refresh and reconnect stays retired; add/remove/re-add with the
same name; schema change while a batch or approval is pending; startup timeout;
budget refusal; unknown extension version; and unchanged Ask/Deny/app grants.
These are design acceptance conditions, not claims that loading-policy code has
already been written or validated.

Before enabling the default, compare the current path with eager DTW on identical
tasks: cold/warm startup, enabled-but-unused DTW, long GUI runs, many unrelated
MCP servers, reconnect/refresh, replay/compaction and revoked app access. Record
provider input/cache-hit/cache-miss tokens, selector calls, first-action latency,
request tool-list hashes/order, and total cost. This assessment justifies that
focused change; it does not fabricate a cache-performance result.
