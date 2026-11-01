# Install Desktop World

DTW is one runtime, a standard stdio MCP server and a bundled Agent Skill. Target
Agents are **Caelis, DeepSeek Harness (DSH), and agy**. The public execution input
is always `exec({app, source})`. Approval belongs to the Agent/Harness; DTW does
not require an approval token or custom Host adapter. The package includes
standard and host-specific plugin manifests around that same executable.

For installer Agents on Windows: use this page's recipe, then the matching host
guide below. The official latest-release API is the source of the current
version; an old installation record is not. Complete one plugin route and a
read-only MCP window check. A conversation created before installation may need
a fresh verifier context; follow the host guide instead of probing private host
RPCs or searching its binaries.

## Select and extract a package

Download the **latest stable** package from [Releases](https://github.com/caelis-labs/desktop-world/releases/latest). Use its version in the asset name:

- macOS (Apple Silicon): `desktop-world-<version>-aarch64-apple-darwin.zip`.
- Windows x64: `desktop-world-<version>-x86_64-pc-windows-msvc.zip`.

Use the adjacent `.sha256` for the selected ZIP. Linux desktop support is experimental.
Release notes identify the exact platform sources, accepted bytes and test scope.

Verify the ZIP's adjacent `.sha256`, extract the complete directory to a permanent
location outside the source checkout, then verify every entry in `SHA256SUMS`.
On macOS, use `shasum -a 256 <archive.zip>` and, inside the extracted directory,
`shasum -a 256 -c SHA256SUMS`.

### Windows x64: download and verify once

Run this whole block in one Windows PowerShell 5.1 or PowerShell 7 invocation.
It resolves the official latest stable release once, downloads its exact Windows
ZIP and adjacent checksum, and verifies both the archive and every packaged
file. It creates a new directory under `%LOCALAPPDATA%\Programs\DesktopWorld\packages`
and returns the verified absolute `package_path`; it does not overwrite an
existing installation. Keep that package while an Agent uses it. A failure leaves
its new directory available for inspection.

```powershell
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') {
  throw 'This recipe requires Windows x64.'
}
function Get-DtwFileSha256 {
  param([string]$LiteralPath)
  $dtwHasher = [Security.Cryptography.SHA256]::Create()
  $dtwStream = $null
  try {
    $dtwStream = [IO.File]::OpenRead($LiteralPath)
    return ([BitConverter]::ToString($dtwHasher.ComputeHash($dtwStream))).Replace('-', '').ToLowerInvariant()
  } finally {
    if ($dtwStream) { $dtwStream.Dispose() }
    $dtwHasher.Dispose()
  }
}
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$dtwApi = 'https://api.github.com/repos/caelis-labs/desktop-world/releases/latest'
$dtwRelease = Invoke-RestMethod -Uri $dtwApi -Headers @{ 'User-Agent' = 'desktop-world-install' }
$dtwTag = [string]$dtwRelease.tag_name
if ($dtwRelease.draft -or $dtwRelease.prerelease -or $dtwTag -cnotmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') {
  throw 'Expected an official stable version tag.'
}
$dtwVersion = $dtwTag.Substring(1)
$dtwName = "desktop-world-$dtwVersion-x86_64-pc-windows-msvc.zip"
$dtwZipAsset = @($dtwRelease.assets | Where-Object { $_.name -ceq $dtwName })
$dtwShaAsset = @($dtwRelease.assets | Where-Object { $_.name -ceq "$dtwName.sha256" })
if ($dtwZipAsset.Count -ne 1 -or $dtwShaAsset.Count -ne 1) { throw 'Missing or duplicate Windows release assets.' }
$dtwBase = "https://github.com/caelis-labs/desktop-world/releases/download/$dtwTag/"
if ($dtwZipAsset[0].browser_download_url -cne "$dtwBase$dtwName" -or
    $dtwShaAsset[0].browser_download_url -cne "$dtwBase$dtwName.sha256") { throw 'Unexpected release asset URL.' }
$dtwRoot = Join-Path $env:LOCALAPPDATA ('Programs\DesktopWorld\packages\' + [guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $dtwRoot)
Write-Host "DTW download directory: $dtwRoot"
$dtwZip = Join-Path $dtwRoot $dtwName
$dtwSha = Join-Path $dtwRoot "$dtwName.sha256"
Invoke-WebRequest -UseBasicParsing -Uri $dtwZipAsset[0].browser_download_url -OutFile $dtwZip
Invoke-WebRequest -UseBasicParsing -Uri $dtwShaAsset[0].browser_download_url -OutFile $dtwSha
$dtwLine = (Get-Content -LiteralPath $dtwSha -Raw -Encoding UTF8).Trim()
$dtwMatch = [regex]::Match($dtwLine, '^([0-9a-fA-F]{64})[ \t]+\*?(.+)$')
if (!$dtwMatch.Success) { throw 'Invalid archive checksum.' }
$dtwExpected = $dtwMatch.Groups[1].Value.ToLowerInvariant()
if ($dtwMatch.Groups[2].Value -cne $dtwName) { throw 'Checksum names a different archive.' }
$dtwArchiveSha = Get-DtwFileSha256 -LiteralPath $dtwZip
if ($dtwArchiveSha -cne $dtwExpected) { throw 'Archive checksum mismatch.' }
if ($dtwZipAsset[0].digest -and $dtwZipAsset[0].digest -cne "sha256:$dtwArchiveSha") { throw 'Release API digest mismatch.' }
$dtwExpanded = Join-Path $dtwRoot 'expanded'
Expand-Archive -LiteralPath $dtwZip -DestinationPath $dtwExpanded
$dtwPackage = Join-Path $dtwExpanded "desktop-world-$dtwVersion-x86_64-pc-windows-msvc"
$dtwSeen = @{}
$dtwCount = 0
foreach ($dtwEntry in (Get-Content -LiteralPath (Join-Path $dtwPackage 'SHA256SUMS') -Encoding UTF8)) {
  $dtwExpected, $dtwRelative = $dtwEntry -split '  ', 2
  if ($dtwExpected -cnotmatch '^[0-9a-f]{64}$' -or !$dtwRelative -or
      $dtwRelative -match '^[/\\]|:|(^|[/\\])\.\.([/\\]|$)' -or $dtwSeen.ContainsKey($dtwRelative)) {
    throw 'Invalid or duplicate package checksum entry.'
  }
  $dtwSeen[$dtwRelative] = $true
  $dtwActual = Get-DtwFileSha256 -LiteralPath (Join-Path $dtwPackage $dtwRelative)
  if ($dtwActual -cne $dtwExpected) { throw "Package checksum mismatch: $dtwRelative" }
  $dtwCount++
}
if ($dtwCount -eq 0 -or $dtwCount -ne (@(Get-ChildItem -LiteralPath $dtwPackage -Recurse -File -Force).Count - 1)) {
  throw 'Package file inventory differs from SHA256SUMS.'
}
$dtwBuild = Get-Content -LiteralPath (Join-Path $dtwPackage 'BUILD.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($dtwBuild.version -cne $dtwVersion -or $dtwBuild.target -cne 'x86_64-pc-windows-msvc') {
  throw 'Package version or target differs from the release.'
}
[pscustomobject]@{ version = $dtwVersion; package_path = $dtwPackage;
  executable = (Join-Path $dtwPackage 'bin\dtw.exe'); verified_files = $dtwCount;
  cordis_url = ([uri](Join-Path $dtwPackage 'cordis\index.mjs')).AbsoluteUri;
  archive_url = $dtwZipAsset[0].browser_download_url;
  archive_sha256 = $dtwArchiveSha } | ConvertTo-Json
```

Use the returned path in the host guide. The recipe verifies package bytes;
the host's MCP calls below establish the live connection. Run it directly in the
current PowerShell process rather than nesting another `powershell -Command`
inside a pipeline.

Keep `BUILD.json`, notices and Skill alongside `bin/dtw` or `bin/dtw.exe`.
The runtime needs no Rust, Xcode, CUA, Node or Python installation. Optional
acceptance scripts have their own Python/PowerShell requirements.

## Connect your Agent

Install the package once in a permanent directory. Register that same executable
and its bundled Skill through your Agent's standard extension points:

| Agent | Setup instructions |
|---|---|
| Caelis | [Install the package as a plugin](docs/install-caelis.md) |
| DeepSeek Harness (DSH) | [Install the bundled Cordis plugin](docs/install-dsh.md#bundled-cordis-plugin); Windows 0.2.1-rc.5 uses one file-URL row |
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
