# Desktop World Agent Plugin

This directory is an [Agent Plugins 1.0](https://agent-plugins.org/specification)
package. It contains one Skill and one stdio MCP server with exactly two tools:
`desktop_exec` and `desktop_status`. The private Node LTS runtime and native
`dtw` helper are included. Running the package needs no system Node, npm, Go,
Python, Docker or browser extension.

Install this directory in a local Agent that supports Agent Plugins. Its client
must provide a persistent writable `PLUGIN_DATA` directory and expand
`${PLUGIN_ROOT}` and `${PLUGIN_DATA}` in `mcp.json` as specified. For a client
that supports Skills and stdio MCP separately, install
`skills/desktop-world/SKILL.md` as a Skill and register the same bundled MCP
server with absolute paths. Registering MCP alone does not load the Skill.

macOS direct MCP example (replace both paths):

```sh
codex mcp add desktop-world -- "/absolute/plugin/root/runtime/node" "/absolute/plugin/root/mcp/server.mjs" --data-dir "/absolute/writable/data"
```

Windows direct MCP example (PowerShell; replace paths):

```powershell
codex mcp add desktop-world -- 'C:\absolute\plugin\root\runtime\node.exe' 'C:\absolute\plugin\root\mcp\server.mjs' --data-dir 'C:\absolute\writable\data'
```

Preserve existing client configuration and its approval policy. `desktop_exec`
is a local code execution tool and can change the desktop. Review the complete
script in the client before approval. The worker and `node:vm` are not an
arbitrary JavaScript security boundary. The native `dw` path starts read only.
Only a trusted startup configuration may pass repeated `--write-app NAME`,
`--input-mode cooperative`, or `--input-policy no_shared_input` arguments after
user authorization. For dynamic grants, a trusted owner uses the `owner_file`
from `desktop_status` with `bin/dtw auth`; the model has no authorization tool.
The descriptor disappears on session close. APP grants last for this MCP
connection or until explicitly revoked. Do not approve OS prompts on behalf of
the user. `bin/dtw doctor` only reports current permissions.

The cursor overlay is a separate click-through, nonactivating native process.
It draws only a point from a successfully delivered native pointer action; it
does not post mouse events. It exits on MCP cleanup. A `window_content`
capture is target local, while `visible_region` tiles carry separate desktop
transforms. Never use a cursor mark or image coordinates as input authority.

The plugin installation root is immutable package content. `PLUGIN_DATA` holds
per-connection `sessions/<uuid>/` assets, owner descriptor and metadata audit.
One MCP process owns one native session and worker. Cancellation, timeout and
disconnect fence new desktop input, request owner EndTurn and stop the worker.
The original native session and receipt IDs are retained until process close;
after disconnect they cannot be queried over the closed stdio connection.
Cleanup uncertainty is reported as `close_incomplete`. No cross-restart script
or exactly-once guarantee exists. Do not automatically replay an unknown call.

The package includes `manifest.json`, `SHA256SUMS`, MPL-2.0 source license and
third-party notices. Verify the outer archive SHA256 against the official
release's `SHA256SUMS` before installation.
