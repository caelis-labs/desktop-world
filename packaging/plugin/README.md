# Desktop World Agent Plugin

This directory is an [Agent Plugins 1.0](https://agent-plugins.org/specification)
package. It contains one Skill and one stdio MCP server with exactly two tools:
`desktop_exec` and `desktop_status`. Each platform release has two archives of
the same version and implementation. **Full** is the default: it includes a
fixed private Node LTS and the native `dtw` helper, so it needs no system Node,
npm, Go, Python, Docker or browser extension. **Lite** contains the same Skill,
MCP server and helper but omits Node. Lite requires an explicitly configured
absolute `DTW_NODE_PATH` to a compatible Node 24.x executable. The helper checks
this at startup and reports missing or incompatible Node; it never searches
`PATH`, downloads Node or changes the system environment.

Install this directory in a local Agent that supports Agent Plugins. Its client
must provide a persistent writable `PLUGIN_DATA` directory and expand
`${PLUGIN_ROOT}` and `${PLUGIN_DATA}` in `mcp.json` as specified. For a client
that supports Skills and stdio MCP separately, install the entire
`skills/desktop-world/` directory as a Skill and register the same bundled MCP
server with absolute paths. Registering MCP alone does not load the Skill.

macOS Full direct MCP example (replace both paths):

```sh
codex mcp add desktop-world -- "/absolute/plugin/root/runtime/node" "/absolute/plugin/root/mcp/server.mjs" --data-dir "/absolute/writable/data"
```

Windows Full direct MCP example (PowerShell; replace paths):

```powershell
codex mcp add desktop-world -- 'C:\absolute\plugin\root\runtime\node.exe' 'C:\absolute\plugin\root\mcp\server.mjs' --data-dir 'C:\absolute\writable\data'
```

For Lite, install the same complete Skill directory, then pass an explicit
compatible Node path. The adapter is the packaged helper, not another MCP
implementation:

```sh
codex mcp add --env DTW_NODE_PATH="/absolute/path/to/node" desktop-world -- "/absolute/lite/root/bin/dtw" plugin-node --data-dir "/absolute/writable/data"
```

For Windows, use `dtw.exe` and an absolute `node.exe` path with the same
`--env DTW_NODE_PATH=...` option. A standard Plugin client may instead supply
`DTW_NODE_PATH` in its trusted process environment before loading Lite.

Preserve existing client configuration and its approval policy. `desktop_exec`
is a local code execution tool and can change the desktop. Review the complete
script in the client before approval. The worker and `node:vm` are not an
arbitrary JavaScript security boundary. The native `dw` path starts read only
without an APP grant. Only a trusted startup configuration may pass repeated
`--write-app NAME` after user authorization. Once the user authorizes a
pointer action for that APP, the standard shared input path may move the real
cursor. A trusted host may choose `--input-policy no_shared_input` to refuse
all shared pointer actions, or `--input-mode cooperative` to borrow the
foreground briefly and attempt to restore the cursor afterward. Cooperative
mode still moves the real pointer temporarily.
For dynamic grants, a trusted owner uses the `owner_file`
from `desktop_status` with `bin/dtw auth`; the model has no authorization tool.
The descriptor disappears on session close. APP grants last for this MCP
connection or until explicitly revoked. Do not approve OS prompts on behalf of
the user. `bin/dtw doctor` only reports current permissions.

The cursor overlay is a separate click-through, nonactivating native process.
It draws only a point from a successfully delivered native pointer action; it
does not post mouse events. It does not replace system input: if an authorized
host enables shared pointer actions, the native backend also moves the real
mouse cursor. The overlay stays above normal windows but below the OS cursor;
its subtle pastel halo remains visible when both arrows occupy the same point.
It hides after five seconds without a delivered pointer action
and exits on MCP cleanup. A `window_content`
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

Each archive includes `manifest.json`, `SHA256SUMS`, MPL-2.0 license and
third-party notices. Verify the outer archive SHA256 against the official
release's `SHA256SUMS` before installation. The source checkout's
`packaging/plugin/` contains standard manifests, Skill, installation guidance
and a link to canonical MCP source. It does not contain a runnable release
payload: repository or marketplace installation must locate and verify the
matching official platform archive; it must not compile on the user's machine.
