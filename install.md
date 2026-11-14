# Install Desktop World

DTW is one runtime, a standard stdio MCP server and a bundled Agent Skill. Target
Agents are **Caelis, DeepSeek Harness (DSH), and agy**. The public execution input
is always `exec({app, source})`. Approval belongs to the Agent/Harness; DTW does
not require an approval token or custom Host adapter. The package includes
standard and host-specific plugin manifests around that same executable.

## Install or update through your Agent

Copy this prompt into the Agent where DTW should be installed or updated:

> 请按 https://github.com/caelis-labs/desktop-world/blob/main/install.md 为当前 Agent 安装或更新 Desktop World。自动识别宿主和系统，使用官方最新稳定版；由你完成版本检查、下载校验、MCP/Skill 整包切换、连接刷新、实际验证和旧包回收。若已经是相同的完整版本，验证后直接结束。保留其他 Agent 的安装和配置，不要求我选择版本、填写路径或手动清理。仅在宿主确实需要用户保存工作或重启时说明具体剩余动作。

The installing Agent owns version management. Follow the
[installer workflow](docs/package-management.md#installer-agent-update-workflow),
then the host guide. Users do not run package commands or choose package IDs.

Read the current host guide linked below before switching configuration; host
reload/installation guidance can be corrected independently of binary releases.
Resolve the official latest stable tag once and obtain its complete package.
Runtime, Skill, plugin manifests and their API must come from that one release.
Use the package's own management guide/CLI for package commands; an installation
guide merged ahead of publication does not add commands to an older executable.
Current source packages use BUILD's `package_store_schema: 2`: public MCP
server/`serverInfo.name` `dtw`, Skill `dtw` at `skills/dtw`, and Caelis Skill
identity `desktop-world:dtw`. The plugin/install ID remains `desktop-world`;
repository, ZIP, Cargo and package-signing identities also retain their names.

Already-published schema 1 packages retain their `desktop-world` server and
`skills/desktop-world` Skill. Select names from the selected package's BUILD and
bundled manifests, never from whichever path exists. Verify or roll back to
those exact old bytes without rewriting them. A naming change alone does not
rebind an existing current or pending installation. The managed store's own
`store_schema` remains `1`; it is separate from BUILD's package layout schema.

For installer Agents on Windows: use this page's recipe, then the matching host
guide below. The official latest-release API is the source of the current
version; an old installation record is not. Complete one plugin route and, after the host reloads that registration, one
read-only MCP window check. A fresh verifier refreshes a conversation, not a host
plugin. If an external profile edit needs a host restart, stop there with pending
state; do not probe private host RPCs or search its binaries for a workaround.

## Select and extract a package

Download the **latest stable** package from [Releases](https://github.com/caelis-labs/desktop-world/releases/latest). Use its version in the asset name:

- macOS (Apple Silicon): `desktop-world-<version>-aarch64-apple-darwin.zip`.
- Windows x64: `desktop-world-<version>-x86_64-pc-windows-msvc.zip`.

Use the adjacent `.sha256` for the selected ZIP. Linux desktop support is experimental.
Release notes identify the exact platform sources, accepted bytes and test scope.

Verify the ZIP's adjacent `.sha256`, then extract the complete directory to a
temporary location outside both the source checkout and the Agent's session
workspace, using the host's supported download and executable-launch route.
On Windows, `%TEMP%` is suitable only when it is outside that workspace and the
host permits execution and access to the intended store there. A temporary path
is not itself an authorization. If policy denies the operation, use the host's
normal approval process or stop and report the required authorization; do not
change sandbox policy or filesystem ACLs. See [DSH Windows compatibility](docs/install-dsh.md#windows-host-execution-and-storage).
On macOS, use
`shasum -a 256 <archive.zip>`. The verified package's native import below verifies
every entry and the exact inventory in `SHA256SUMS`; do not first run a second
per-file shell verifier or print hundreds of successful checksum rows.

After transport verification, import once with the packaged executable:

```sh
"/absolute/extracted/package/bin/dtw" package import "/absolute/extracted/package"
```

Use the returned immutable `package_path`, `executable` and `cordis_url`. Bind
each host/profile to that package before configuring it; see
[package lifecycle](docs/package-management.md). This native command verifies the
whole package again, including executable, Skill and plugin versions.

`BUILD.kind` describes construction, not publication. Official stable packages
may retain `unpublished-rewrite-candidate`; use the official release tag and
draft/prerelease flags to determine release status.

### Windows x64: download and verify once

Run this whole block in one Windows PowerShell 5.1 or PowerShell 7 invocation.
It resolves the official latest stable release once, downloads its exact Windows
ZIP and adjacent checksum, and verifies the archive. Native import verifies every
packaged file and the exact inventory. It downloads into a unique child of the user's Local Application Data
`dtw-stage` directory, then returns the permanent package recipe. A failure
preserves the child for inspection. Confirm that the resolved staging directory
is outside the workspace and that your host permits executing the verified CLI
there. Set `DTW_STAGING_DIR` to an absolute host-approved directory if required.
The recipe does not use the host's redirected `TEMP` or `TMP` for staging. The host must also allow the packaged CLI to
access the default `%LOCALAPPDATA%\DesktopWorld` store. Do not run a workspace-local
copy and assume a host's broad permission label authorizes that child process.
If either path is denied, request normal host authorization before proceeding.

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
$dtwStage = $env:DTW_STAGING_DIR
if ([string]::IsNullOrWhiteSpace($dtwStage)) {
  $dtwLocal = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
  if (!$dtwLocal) { throw 'Local Application Data is unavailable. Set DTW_STAGING_DIR to an approved absolute path.' }
  $dtwStage = Join-Path $dtwLocal 'dtw-stage'
}
if ($dtwStage -notmatch '^(?:[A-Za-z]:[\\/]|\\\\[^\\]+\\[^\\]+\\?)') {
  throw 'DTW_STAGING_DIR must be an absolute Windows path.'
}
$dtwStage = [IO.Path]::GetFullPath($dtwStage)
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
$dtwRoot = Join-Path $dtwStage ('dtw-download-' + [guid]::NewGuid().ToString('N'))
try { [void](New-Item -ItemType Directory -Path $dtwRoot -Force) }
catch { throw "DTW staging failed at '$dtwRoot': $($_.Exception.GetBaseException().Message)" }
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
$dtwExe = Join-Path $dtwPackage 'bin\dtw.exe'
$dtwStore = Join-Path $env:LOCALAPPDATA 'DesktopWorld'
$dtwStart = New-Object Diagnostics.ProcessStartInfo
$dtwStart.FileName = $dtwExe
$dtwStart.Arguments = 'package import "' + $dtwPackage + '"'
$dtwStart.UseShellExecute = $false
$dtwStart.RedirectStandardOutput = $true
$dtwStart.RedirectStandardError = $true
$dtwStart.StandardOutputEncoding = [Text.Encoding]::UTF8
$dtwStart.StandardErrorEncoding = [Text.Encoding]::UTF8
try { $dtwChild = [Diagnostics.Process]::Start($dtwStart) }
catch {
  $dtwError = $_.Exception.GetBaseException()
  $dtwCode = if ($dtwError -is [ComponentModel.Win32Exception]) { $dtwError.NativeErrorCode } else { $dtwError.HResult }
  throw "DTW launch failed ($dtwCode) for '$dtwExe'; store '$dtwStore'; staging '$dtwRoot'. $($dtwError.Message)"
}
try {
  $dtwStdout = $dtwChild.StandardOutput.ReadToEndAsync()
  $dtwStderr = $dtwChild.StandardError.ReadToEndAsync()
  $dtwChild.WaitForExit()
  if ($dtwChild.ExitCode -ne 0) {
    throw "DTW import exited $($dtwChild.ExitCode) from '$dtwExe'; store '$dtwStore'; staging '$dtwRoot'. $($dtwStderr.Result.Trim())"
  }
  $dtwRecipe = $dtwStdout.Result | ConvertFrom-Json
} finally {
  $dtwChild.Dispose()
}
if ($dtwRecipe.version -cne $dtwVersion -or $dtwRecipe.manager_version -cne $dtwVersion -or
    $dtwRecipe.target -cne 'x86_64-pc-windows-msvc' -or $dtwRecipe.store_schema -ne 1) {
  throw 'Imported package/manager identity differs from the selected release.'
}
$dtwRecipe | Add-Member -NotePropertyName archive_url -NotePropertyValue $dtwZipAsset[0].browser_download_url
$dtwRecipe | Add-Member -NotePropertyName archive_sha256 -NotePropertyValue $dtwArchiveSha
$dtwRecipe | ConvertTo-Json -Depth 12
try {
  $dtwOwned = Get-Item -LiteralPath $dtwRoot
  if ($dtwOwned.Parent.FullName.TrimEnd('\') -ne $dtwStage.TrimEnd('\') -or
      $dtwOwned.Name -notmatch '^dtw-download-[0-9a-f]{32}$' -or
      ($dtwOwned.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Staging directory identity changed.'
  }
  Remove-Item -LiteralPath $dtwOwned.FullName -Recurse -Force
}
catch { Write-Warning "Verified import succeeded; retry cleanup of this owned download directory: $dtwRoot" }
```

Use the returned path in the host guide. The recipe verifies package bytes;
the host's MCP calls below establish the live connection. Run it directly in the
current PowerShell process rather than nesting another `powershell -Command`
inside a pipeline.

Keep `BUILD.json`, notices and Skill alongside `bin/dtw` or `bin/dtw.exe`.
The runtime needs no Rust, Xcode, CUA, Node or Python installation. Optional
acceptance scripts have their own Python/PowerShell requirements.

## Connect your Agent

Import the package once into the managed store. Register that same executable
and its bundled Skill through your Agent's standard extension points:

| Agent | Setup instructions |
|---|---|
| Caelis | [Install the package as a plugin](docs/install-caelis.md) |
| DeepSeek Harness (DSH) | [Install the bundled Cordis plugin](docs/install-dsh.md#bundled-cordis-plugin); the tested Desktop format uses one file-URL row |
| Antigravity App / agy CLI | [Install the package as one plugin](docs/install-agy.md) |

Several Agents can point to the same installed package; each MCP connection has
its own runtime state. Do not duplicate the package for each model or register
both plugin and manual MCP routes in one session. The optional Cordis adapter
only composes DSH's existing MCP and Skill extensions.

## Check the installation

1. Check the installed `bin/dtw --version` (`bin/dtw.exe --version` on Windows).
2. In the Agent, discover `apps` and `exec`, then search with `apps({query})`.
3. Choose a returned app ID unchanged and call `exec` with that `app` and
   `source: "await app.look();"`. Read the bundled Skill before input.
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
unknown outcome, inspect the returned evidence and read current app state before deciding on new input.

## Upgrade or remove

Follow [package lifecycle](docs/package-management.md): import → bind a pending
package for one host/profile → update its complete plugin/Skill/MCP registration
→ apply the host's supported reload/restart → verify the actual host connection
and Skill → activate. Use a fresh conversation only for a remaining stale cache.
Other hosts retain their current versions. Activation collects unpinned packages
older than seven days; current, pending, rollback and running packages are kept.
`dtw package gc` previews collection; `gc --apply` applies it only inside the
owned store. Host-managed plugin copies use that host's own removal controls.

For removal, stop that host's connections, remove its DTW registration and Skill,
then `dtw package unbind <host-profile>`. Preserve unrelated settings and plugins.

## Other standard MCP clients

Clients accepting `mcpServers` JSON can use:

```json
{
  "mcpServers": {
    "dtw": {
      "command": "/absolute/managed/package/bin/dtw",
      "args": ["mcp"]
    }
  }
}
```

Replace the example command with import's returned `executable` (on Windows,
`bin/dtw.exe`) and use the same package's bundled Skill through the client's
normal discovery directory. Codex can
use `codex mcp add dtw -- /absolute/path/to/dtw mcp`. Claude Code can
load the complete package with `claude --plugin-dir /absolute/path/to/package`;
its optional layout is described in [the staging guide](https://github.com/caelis-labs/desktop-world/blob/main/plugins/claude/README.md).
Neither client is a new required release matrix. Do not run the removed
`dtw install` command or install both plugin and manual server into one session.
