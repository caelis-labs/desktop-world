# Install Desktop World

DTW is one runtime, a standard stdio MCP server and a bundled Agent Skill. Target
Agents are **Caelis, DeepSeek Harness (DSH), and agy**. The public execution input
is always `exec({app, source})`. Approval belongs to the Agent/Harness; DTW does
not require an approval token or custom Host adapter. The package includes
standard and host-specific plugin manifests around that same executable.

## Select and extract a package

Download the **latest stable** package from [Releases](https://github.com/caelis-labs/desktop-world/releases/latest). Use its version in the asset name:

- macOS (Apple Silicon): `desktop-world-<version>-aarch64-apple-darwin.zip`.
- Windows x64: `desktop-world-<version>-x86_64-pc-windows-msvc.zip`.

Use the adjacent `.sha256` for the selected ZIP. Historical v0.1.x assets use the
retired runtime and are not substitutes. Linux desktop support is experimental.
Release notes identify the exact platform sources, accepted bytes and test scope.

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

## Connect your Agent

Install the package once in a permanent directory. Register that same executable
and its bundled Skill through your Agent's standard extension points:

| Agent | Setup instructions |
|---|---|
| Caelis | [Install the package as a plugin](docs/install-caelis.md) |
| DeepSeek Harness (DSH) | [Install the bundled Cordis adapter, or use the manual fallback](docs/install-dsh.md) |
| Antigravity App / agy CLI | [Install the package as one plugin](docs/install-agy.md) |

Several Agents can point to the same installed package; each MCP connection has
its own runtime state. Do not duplicate the package for each model or register
both plugin and manual MCP routes in one session. The optional Cordis adapter
only composes DSH's existing MCP and Skill extensions.

## Check the installation

1. Check the installed `bin/dtw --version` (`bin/dtw.exe --version` on Windows).
2. In the Agent, discover `apps`, `api`, `exec`, and `result`, then call `apps`.
3. Choose a returned app ID unchanged and call `exec` with that `app` and
   `source: "return await app.windows();"`. Read the bundled Skill before input.
4. With a test window open, capture it through the documented screenshot API.
   Tool discovery alone does not prove native GUI access works.

On macOS, Accessibility and Screen Recording are separate OS permissions. macOS
may attribute requests to the launching **host App**, such as DeepSeek Harness,
Caelis or Terminal, rather than showing a separate `dtw` entry. Follow the system
prompt and check that host in **System Settings → Privacy & Security**. A host
name reported by DTW is a troubleshooting hint, not proof of macOS's TCC decision.
After a Screen Recording change, fully quit and reopen the host if access still
fails or macOS requests it, then retry the read-only checks. A permission granted
in Terminal does not establish that the same binary launched by a desktop Agent
has access. [DSH-specific troubleshooting](docs/install-dsh.md#macos-permissions)
covers that distinction.

DTW manages foreground access when an operation requires it. Windows may use
targeted native delivery for supported controls; independent background input
and secure-desktop operation are not promised. Supplementary-plane text (including
emoji) needs readable literal text and a selection in the focused control;
unsupported/password fields are rejected before dispatch. If a write returns an
unknown outcome, read its original receipt using `result` instead of replaying it.

## Upgrade or remove

Finish active operations and stop the old MCP connections before replacing
package files. Extract a new verified package, update the Agent registration and
Skill together, then open a new session. Follow your Agent's linked instructions
for changing a registered path. Preserve unrelated settings and plugins.
To uninstall, remove DTW's registration, installed Skill and owned package
directory after its connections have stopped.

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
its optional layout is described in [the staging guide](https://github.com/caelis-labs/desktop-world/blob/main/plugins/claude/README.md).
Neither client is a new required release matrix. Do not run the removed
`dtw install` command or install both plugin and manual server into one session.
