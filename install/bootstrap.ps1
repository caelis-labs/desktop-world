# Run the repository bootstrap in Windows PowerShell 5.1 or PowerShell 7.
# Supply INSTALLED_DTW only from the host's actual, trusted DTW registration.
[CmdletBinding()]
param(
  [Parameter(Position = 0)][string]$HostName,
  [Parameter(Position = 1)][string]$Profile,
  [Parameter(Position = 2)][string]$InstalledDtw
)

function Test-DtwAbsoluteWindowsPath {
  param([string]$Path)
  return $Path -match '^(?:[A-Za-z]:[\\/]|\\\\[^\\]+\\[^\\]+\\?)'
}

function ConvertTo-DtwWindowsArgument {
  param([string]$Argument)
  # Windows argv parsing doubles backslashes before quotes, including the closing quote.
  $escaped = [regex]::Replace($Argument, '(\\*)"', '$1$1\"')
  $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
  return '"' + $escaped + '"'
}

function Invoke-DtwRuntime {
  param([string]$Executable, [string[]]$Arguments, [string]$Staging)

  $store = Join-Path $env:LOCALAPPDATA 'DesktopWorld'
  $location = "runtime '$Executable', store '$store'"
  if ($Staging) { $location += ", staging '$Staging'" }
  # Avoid shell resolution and retain native Windows launch error codes.
  $start = New-Object Diagnostics.ProcessStartInfo
  $start.FileName = $Executable
  $start.Arguments = ($Arguments | ForEach-Object { ConvertTo-DtwWindowsArgument $_ }) -join ' '
  $start.UseShellExecute = $false
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $start.StandardOutputEncoding = [Text.Encoding]::UTF8
  $start.StandardErrorEncoding = [Text.Encoding]::UTF8
  try { $child = [Diagnostics.Process]::Start($start) }
  catch {
    $errorDetail = $_.Exception.GetBaseException()
    $code = if ($errorDetail -is [ComponentModel.Win32Exception]) { $errorDetail.NativeErrorCode } else { $errorDetail.HResult }
    throw "DTW launch failed ($code) for $location. $($errorDetail.Message)"
  }
  try {
    $stdout = $child.StandardOutput.ReadToEndAsync()
    $stderr = $child.StandardError.ReadToEndAsync()
    $child.WaitForExit()
    if ($child.ExitCode -ne 0) {
      throw "DTW exited $($child.ExitCode) from $location. $($stderr.Result.Trim())"
    }
    return $stdout.Result
  } finally {
    $child.Dispose()
  }
}

function Test-DtwSetupCapability {
  param([string]$Executable)

  $help = Invoke-DtwRuntime $Executable @('--help')
  if ($help -cmatch '(?m)^[ \t]*dtw setup\b') {
    $capability = Invoke-DtwRuntime $Executable @('setup', '--help') | ConvertFrom-Json
    if (($capability.setup_schema -isnot [int] -and $capability.setup_schema -isnot [long]) -or
        $capability.setup_schema -ne 1) {
      throw "Unsupported DTW setup schema from '$Executable'."
    }
    return $true
  }
  if ($help -cmatch '(?m)^[ \t]*dtw app catalog\b' -and
      $help -cmatch '(?m)^[ \t]*dtw mcp\b' -and $help -cmatch '(?m)^[ \t]*dtw package\b') {
    return $false
  }
  throw "Unrecognized DTW help from '$Executable'."
}

function Get-DtwSetupState {
  param([string]$Json, [string]$HostName, [string]$Profile, [string]$PackageId)

  $reply = ConvertFrom-Json -InputObject $Json
  if ($reply.host -isnot [string] -or $reply.host -cne $HostName -or
      $reply.profile -isnot [string] -or $reply.profile -cne $Profile -or
      ($PackageId -and ($reply.package_id -isnot [string] -or $reply.package_id -cne $PackageId))) {
    throw 'DTW setup response identity does not match this request.'
  }
  if ($reply.state -isnot [string]) { throw 'DTW setup response is missing a valid state.' }
  return $reply.state
}

function Get-DtwStreamSha256 {
  param([IO.Stream]$Stream)

  $hasher = [Security.Cryptography.SHA256]::Create()
  try { return ([BitConverter]::ToString($hasher.ComputeHash($Stream))).Replace('-', '').ToLowerInvariant() }
  finally { $hasher.Dispose() }
}

function Get-DtwFileSha256 {
  param([string]$LiteralPath)

  $stream = [IO.File]::OpenRead($LiteralPath)
  try { return Get-DtwStreamSha256 $stream }
  finally { $stream.Dispose() }
}

function Get-DtwReleaseAsset {
  param($Release, [string]$Name)

  $asset = @($Release.assets | Where-Object { $_.name -ceq $Name })
  if ($asset.Count -ne 1) { throw "Missing or duplicate release asset '$Name'." }
  $url = "https://github.com/caelis-labs/desktop-world/releases/download/$($Release.tag_name)/$Name"
  if ($asset[0].browser_download_url -isnot [string] -or $asset[0].browser_download_url -cne $url) {
    throw "Unexpected release asset URL for '$Name'."
  }
  return $asset[0]
}

function Get-DtwReleaseManifest {
  param($Release)

  $asset = Get-DtwReleaseAsset $Release 'RELEASE.json'
  if ($asset.digest -isnot [string] -or $asset.digest -cnotmatch '^sha256:[0-9a-f]{64}\z') {
    throw 'Missing or invalid RELEASE.json API digest.'
  }
  $response = Invoke-WebRequest -UseBasicParsing -Uri $asset.browser_download_url
  $stream = $response.RawContentStream
  try {
    $stream.Position = 0
    if ($asset.digest -cne ('sha256:' + (Get-DtwStreamSha256 $stream))) { throw 'RELEASE.json API digest mismatch.' }
    return ConvertFrom-Json -InputObject ([Text.Encoding]::UTF8.GetString($stream.ToArray()))
  } finally {
    $stream.Dispose()
  }
}

function Get-DtwStageRoot {
  # Use the user's real Local Application Data when TEMP/TMP are host-redirected.
  $root = $env:DTW_STAGING_DIR
  if ([string]::IsNullOrWhiteSpace($root)) {
    $localAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    if (!$localAppData) { throw 'Local Application Data is unavailable. Set DTW_STAGING_DIR to an approved absolute path.' }
    $root = Join-Path $localAppData 'dtw-stage'
  }
  if (!(Test-DtwAbsoluteWindowsPath $root)) { throw 'DTW_STAGING_DIR must be an absolute Windows path.' }
  return [IO.Path]::GetFullPath($root)
}

function Invoke-DtwAcquisition {
  param($Release, $Platform, [string]$HostName, [string]$Profile)

  $name = $Platform.archive
  $zipAsset = Get-DtwReleaseAsset $Release $name
  $sidecarAsset = Get-DtwReleaseAsset $Release "$name.sha256"
  if ($zipAsset.digest -isnot [string] -or $zipAsset.digest -cne "sha256:$($Platform.archive_sha256)") {
    throw 'Archive release API digest does not match RELEASE.json.'
  }
  $stage = Get-DtwStageRoot
  $root = Join-Path $stage ('dtw-download-' + [guid]::NewGuid().ToString('N'))
  try { [void](New-Item -ItemType Directory -Path $root -Force) }
  catch { throw "DTW staging failed at '$root'. $($_.Exception.GetBaseException().Message)" }
  Write-Host "DTW download directory: $root"
  $zip = Join-Path $root $name
  $sidecar = Join-Path $root "$name.sha256"
  Invoke-WebRequest -UseBasicParsing -Uri $zipAsset.browser_download_url -OutFile $zip
  Invoke-WebRequest -UseBasicParsing -Uri $sidecarAsset.browser_download_url -OutFile $sidecar
  $line = (Get-Content -LiteralPath $sidecar -Raw -Encoding UTF8).Trim()
  $checksum = [regex]::Match($line, '^([0-9a-fA-F]{64})[ \t]+\*?(.+)\z')
  if (!$checksum.Success -or $checksum.Groups[2].Value -cne $name) { throw 'Invalid archive checksum sidecar.' }
  if ($checksum.Groups[1].Value.ToLowerInvariant() -cne $Platform.archive_sha256 -or
      (Get-DtwFileSha256 $zip) -cne $Platform.archive_sha256) {
    throw 'Archive checksum does not match RELEASE.json.'
  }

  $expanded = Join-Path $root 'expanded'
  Expand-Archive -LiteralPath $zip -DestinationPath $expanded
  $package = Join-Path $expanded ([IO.Path]::GetFileNameWithoutExtension($name))
  $executable = Join-Path (Join-Path $package 'bin') 'dtw.exe'
  if ((Get-DtwFileSha256 $executable) -cne $Platform.binary_sha256) {
    throw 'Packaged DTW executable checksum does not match RELEASE.json.'
  }
  $result = Invoke-DtwRuntime $executable @('setup', 'prepare', $HostName, $Profile, $Platform.package_id, $package) $root
  $state = Get-DtwSetupState $result $HostName $Profile $Platform.package_id
  if (@('verify', 'restart_required', 'host_action_required') -cnotcontains $state) {
    throw "Unexpected DTW setup prepare state '$state' after acquisition. Staging '$root'."
  }
  try {
    $owned = Get-Item -LiteralPath $root
    if ($owned.Parent.FullName.TrimEnd('\', '/') -ne $stage.TrimEnd('\', '/') -or
        $owned.Name -cnotmatch '^dtw-download-[0-9a-f]{32}\z' -or
        ($owned.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
      throw 'Staging directory identity changed.'
    }
    Remove-Item -LiteralPath $owned.FullName -Recurse -Force
  } catch {
    Write-Warning "Verified setup prepare succeeded. Retry cleanup of this owned download directory '$root'."
  }
  return $result
}

function Invoke-DtwBootstrap {
  param([string]$HostName, [string]$Profile, [string]$InstalledDtw)

  if ([string]::IsNullOrWhiteSpace($HostName) -or [string]::IsNullOrWhiteSpace($Profile)) {
    throw 'Usage: bootstrap.ps1 HOST PROFILE [INSTALLED_DTW]'
  }
  $setupCapable = $false
  if ($InstalledDtw) {
    if (!(Test-DtwAbsoluteWindowsPath $InstalledDtw) -or [IO.Path]::GetExtension($InstalledDtw) -ne '.exe') {
      throw 'INSTALLED_DTW must be the trusted registered DTW executable at an absolute Windows path.'
    }
    $setupCapable = Test-DtwSetupCapability $InstalledDtw
    if ($setupCapable) {
      $result = Invoke-DtwRuntime $InstalledDtw @('setup', 'resume', $HostName, $Profile)
      $state = Get-DtwSetupState $result $HostName $Profile
      if (@('verify', 'restart_required', 'host_action_required') -ccontains $state) { return $result }
      if ($state -cne 'no_pending') { throw "Unexpected DTW setup resume state '$state'." }
    }
  }

  $api = 'https://api.github.com/repos/caelis-labs/desktop-world/releases/latest'
  $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'desktop-world-install' }
  $tag = [string]$release.tag_name
  if ($release.draft -isnot [bool] -or $release.draft -or
      $release.prerelease -isnot [bool] -or $release.prerelease -or
      $tag -cnotmatch '^v[0-9]+\.[0-9]+\.[0-9]+\z') {
    throw 'Expected an official stable version tag.'
  }
  $version = $tag.Substring(1)
  $target = 'x86_64-pc-windows-msvc'
  $manifest = Get-DtwReleaseManifest $release
  if ($manifest.version -isnot [string] -or $manifest.version -cne $version -or
      $manifest.tag -isnot [string] -or $manifest.tag -cne $tag) {
    throw 'RELEASE.json version or tag mismatch.'
  }
  $platforms = @($manifest.platforms | Where-Object { $_.target -ceq $target })
  if ($platforms.Count -ne 1) { throw 'Missing or duplicate Windows platform in RELEASE.json.' }
  $platform = $platforms[0]
  if (!$platform.PSObject.Properties['package_id'] -and [version]$version -le [version]'0.6.0') {
    throw "Official release $tag predates DTW setup. Follow the legacy Windows install recipe and your host guide at https://github.com/caelis-labs/desktop-world/blob/$tag/install.md#windows-x64-download-and-verify-once"
  }
  if ($platform.package_id -isnot [string] -or $platform.package_id -cnotmatch '^[0-9a-f]{64}\z' -or
      $platform.archive -isnot [string] -or $platform.archive -cne "desktop-world-$version-$target.zip" -or
      $platform.archive_sha256 -isnot [string] -or $platform.archive_sha256 -cnotmatch '^[0-9a-f]{64}\z' -or
      $platform.binary_sha256 -isnot [string] -or $platform.binary_sha256 -cnotmatch '^[0-9a-f]{64}\z') {
    throw 'Invalid Windows package identity in RELEASE.json.'
  }
  if ($setupCapable) {
    $result = Invoke-DtwRuntime $InstalledDtw @('setup', 'prepare', $HostName, $Profile, $platform.package_id)
    $state = Get-DtwSetupState $result $HostName $Profile $platform.package_id
    if (@('verify', 'restart_required', 'host_action_required') -ccontains $state) { return $result }
    if ($state -cne 'needs_download') { throw "Unexpected DTW setup prepare state '$state'." }
  }
  return Invoke-DtwAcquisition $release $platform $HostName $Profile
}

if ($MyInvocation.InvocationName -ne '.') {
  $ErrorActionPreference = 'Stop'
  if ($env:OS -ne 'Windows_NT' -or
      [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') {
    throw 'This bootstrap requires Windows x64.'
  }
  $originalProtocol = [Net.ServicePointManager]::SecurityProtocol
  try {
    if ($PSVersionTable.PSVersion.Major -eq 5) {
      [Net.ServicePointManager]::SecurityProtocol = $originalProtocol -bor [Net.SecurityProtocolType]::Tls12
    }
    Invoke-DtwBootstrap $HostName $Profile $InstalledDtw
  } finally {
    if ($PSVersionTable.PSVersion.Major -eq 5) {
      [Net.ServicePointManager]::SecurityProtocol = $originalProtocol
    }
  }
}
