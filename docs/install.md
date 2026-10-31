# Install Desktop World

DTW is one runtime, a standard stdio MCP server and a bundled Agent Skill. Target
Agents are **Caelis, DeepSeek Harness (DSH), and agy**. The public execution input
is always `exec({app, source})`. Approval belongs to the Agent/Harness; DTW does
not require an approval token, custom Host adapter or private DSH plugin.

## Select and extract a package

**The rewrite's v0.2.0 is not published yet.** Use an explicitly supplied candidate
from [candidate construction](releasing.md) and check its source/hash against
[the current evidence](rewrite-candidate-acceptance.md). Historical v0.1.x assets
use the retired runtime and are not substitutes. macOS arm64 and Windows x64 are
the release targets; Windows desktop acceptance and final macOS installed-package
acceptance remain required. Linux desktop support is experimental.

Verify the ZIP's adjacent `.sha256`, extract the complete directory to a permanent
location outside the source checkout, then verify every entry in `SHA256SUMS`.
On macOS, use `shasum -a 256 <archive.zip>` and, inside the extracted directory,
`shasum -a 256 -c SHA256SUMS`. On Windows, compare the ZIP with
`Get-FileHash -Algorithm SHA256 <archive.zip>`. To verify the listed package files
in PowerShell, run this from inside the extracted directory:

```powershell
$ErrorActionPreference = 'Stop'
Get-Content -LiteralPath .\SHA256SUMS -Encoding UTF8 | ForEach-Object {
  $expected, $relative = $_ -split '  ', 2
  if ($expected -cnotmatch '^[0-9a-f]{64}$' -or !$relative -or
      $relative -match '^[/\\]|:|(^|[/\\])\.\.([/\\]|$)') { throw 'Invalid checksum entry' }
  $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $relative).Hash.ToLowerInvariant()
  if ($actual -cne $expected) { throw "Checksum mismatch: $relative" }
}
```

Keep `BUILD.json`, notices and Skill alongside `bin/dtw` or `bin/dtw.exe`.
The runtime needs no Rust, Xcode, CUA, Node or Python installation. Optional
acceptance scripts have their own Python/PowerShell requirements.

## Caelis

The existing package contains a compatible plugin manifest and shares one runtime
between its MCP and Skill entries. Caelis supports this layout through its normal
[plugin manager](https://github.com/caelis-labs/caelis/blob/main/docs/plugins.md):

```sh
caelis plugin install /absolute/path/to/extracted-package --json
caelis plugin status <installed-id> --json
```

Windows candidates include Agent Plugins 1.0 metadata with the stable installed
ID `desktop-world`, independent of the extracted directory name. Check them with
`caelis plugin status desktop-world --json`. An earlier Claude-format Windows
package may have registered its directory name instead; remove that exact old
ID before installing this standard format, preserving unrelated registrations.
macOS keeps its existing package format; use the ID returned by installation.

Or enter `/plugin install /absolute/path/to/extracted-package` in the Caelis TUI.
Start a new Session to load the MCP/Skill snapshot. Registration alone does not
prove tools are ready; complete the first-execution check below. Do not separately
register a second copy of the MCP server.

**Next Core integration:** parse the standard `exec.app` argument and request
approval on first use of each app in a Session. Approval for that app is reused
for subsequent scripts in the same Session; a new app or new Session needs its
own approval. Denial must stop before dispatch. This is planned Core behavior,
not a permission mechanism implemented by DTW or a claim about the current Core.
Acceptance will use `deepseek-flash` and `gpt-6.1-terra`, including screenshots,
repeated calls, denial and separate Sessions.

## DeepSeek Harness (DSH)

Use DSH's built-in Plugin Manager to add one instance of
`@deepseek-ai/dsh-mcp-client` with this configuration (use `dtw.exe` on Windows):

```json
{
  "serverName": "desktop-world",
  "transport": "stdio",
  "command": "/absolute/path/to/extracted-package/bin/dtw",
  "args": ["mcp"]
}
```

Enable the built-in Skill filesystem provider and add the package's `skills`
directory to its `customSkillDirs`, or copy the bundled `skills/desktop-world`
to the workspace's `.agents/skills/desktop-world`. Preserve other MCP servers,
Skill roots and model settings. No DTW-specific plugin code is needed. For ACP,
DSH can also receive the same stdio server from the ACP client's session setup.

The verified DSH 0.2.1-rc.5 / DeepSeek-V4.1-Flash route uses the model ID
`deepseek-flash` and accepts inline screenshots. DSH's MCP bridge is documented in
[its upstream package](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/mcp/mcp-client).
Use the installed DSH configuration schema if its version differs; do not replace
an entire profile to add this one server.

## agy

```sh
agy mcp add desktop-world /absolute/path/to/extracted-package/bin/dtw mcp
```

On Windows, pass the absolute `bin/dtw.exe` path. Copy the complete bundled
`skills/desktop-world` to the workspace's `.agents/skills/desktop-world`, then
start a session in that workspace. agy has its own ordinary MCP and Skill extension
points; do not import the Claude manifest expecting `${CLAUDE_PLUGIN_ROOT}` to
be expanded by agy. Model exploration errors are recorded separately from DTW
bugs and are not themselves a release blocker.

## Other standard MCP clients

Clients accepting `mcpServers` JSON can use:

```json
{
  "mcpServers": {
    "desktop-world": {
      "command": "/absolute/path/to/extracted-package/bin/dtw",
      "args": ["mcp"]
    }
  }
}
```

Add the bundled Skill using the client's normal discovery directory. Codex can
use `codex mcp add desktop-world -- /absolute/path/to/dtw mcp`. Claude Code can
load the complete package with `claude --plugin-dir /absolute/path/to/package`;
its optional layout is described in [the staging guide](../plugins/claude/README.md).
Neither client is a new required release matrix. Do not run the removed
`dtw install` command or install both plugin and manual server into one session.

## First execution and updates

Check that `apps`, `api`, `exec`, and `result` are discoverable. Call `apps`, choose
a returned app ID unchanged, then execute only `return await app.windows();`.
Confirm the result names that app and its windows. Read the bundled Skill before
input. `result` reads original receipts when needed; it is not routine polling.

macOS requires Accessibility and Screen Recording permissions for relevant
operations. Use the actual installed process identity in the OS permission
settings. Windows uses the public foreground keyboard/pointer route; background
Windows input and secure-desktop operation are not promised. A successful MCP
connection does not prove native GUI or permissions are ready.

For upgrades, finish active operations and stop the old connection before changing
package files. Extract a new verified package, update the registration/Skill and
start a new Session. Caelis binds a local registration to its canonical directory:
to change directories, run `caelis plugin remove desktop-world --json` before
installing the new root. Preserve unrelated configuration. Remove a stopped DTW
installation by removing its registration, installed Skill and owned package
directory. Never repeat an uncertain desktop action during installation/recovery.

From source, use [building](building.md). For installed-package and cross-platform
acceptance, use [releasing](releasing.md) and the Windows README bundled with the
same candidate. Publish the same verified bytes only after platform acceptance.
