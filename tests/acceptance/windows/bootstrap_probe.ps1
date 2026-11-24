# Detached bootstrap probe. HTTP and DTW calls consume owned fixtures only.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../../../install/bootstrap.ps1')
$launchRuntime = ${function:Invoke-DtwRuntime}

function Assert-DtwProbe {
  param([bool]$Condition, [string]$Message)
  if (!$Condition) { throw $Message }
}

function Assert-DtwCalls {
  param([string[]]$Expected)
  $actual = $script:calls -join ', '
  $expectedCalls = $Expected -join ', '
  Assert-DtwProbe ($actual -ceq $expectedCalls) "Expected calls [$expectedCalls], got [$actual]."
}

function Assert-DtwFailure {
  param([scriptblock]$Run, [string]$Message)
  try { [void](& $Run) }
  catch {
    if ($_.Exception.Message.Contains($Message)) { return }
    throw
  }
  throw "Expected failure containing '$Message'."
}

function Reset-DtwFixture {
  param([string]$Version = '0.6.1', [switch]$Legacy, [switch]$WrongBinary)

  $script:calls = @()
  $script:resumeState = 'no_pending'
  $script:cacheState = 'needs_download'
  $script:sourceState = 'verify'
  $script:setupSchema = 1
  $script:oldRuntime = $false
  $script:prepareOutput = $null
  $script:reportedSource = $null
  $script:sidecarName = $archiveName
  $script:zipBytes = [IO.File]::ReadAllBytes($archiveFixture)
  $platform = [ordered]@{target=$target; archive=$archiveName; archive_sha256=$archiveSha; binary_sha256=$binarySha}
  if ($WrongBinary) { $platform.binary_sha256 = '0' * 64 }
  if (!$Legacy) { $platform.package_id = $packageId }
  $providedManifest = @{version=$Version; tag="v$Version"; platforms=@($platform)} | ConvertTo-Json -Depth 5
  $script:manifestBytes = [Text.Encoding]::UTF8.GetBytes($providedManifest)
  $stream = [IO.MemoryStream]::new($script:manifestBytes)
  try { $manifestSha = Get-DtwStreamSha256 $stream }
  finally { $stream.Dispose() }
  $base = "https://github.com/caelis-labs/desktop-world/releases/download/v$Version/"
  $script:release = [pscustomobject]@{
    draft=$false; prerelease=$false; tag_name="v$Version"; assets=@(
      [pscustomobject]@{name='RELEASE.json'; browser_download_url="${base}RELEASE.json"; digest="sha256:$manifestSha"}
      [pscustomobject]@{name=$archiveName; browser_download_url="$base$archiveName"; digest="sha256:$archiveSha"}
      [pscustomobject]@{name="$archiveName.sha256"; browser_download_url="$base$archiveName.sha256"}
    )
  }
}

function Invoke-RestMethod {
  param([string]$Uri, [hashtable]$Headers)
  Assert-DtwProbe ($Uri -ceq 'https://api.github.com/repos/caelis-labs/desktop-world/releases/latest') "Unexpected API $Uri."
  Assert-DtwProbe ($Headers['User-Agent'] -ceq 'desktop-world-install') 'Expected an installation user agent.'
  $script:calls += 'latest'
  return $script:release
}

function Invoke-WebRequest {
  param([switch]$UseBasicParsing, [string]$Uri, [string]$OutFile)
  Assert-DtwProbe ($UseBasicParsing.IsPresent) 'Expected plain HTTP response parsing.'
  Assert-DtwProbe ($Uri.StartsWith("https://github.com/caelis-labs/desktop-world/releases/download/$($script:release.tag_name)/")) "Unexpected download origin $Uri."
  $name = ([uri]$Uri).Segments[-1]
  $script:calls += $name
  switch -CaseSensitive ($name) {
    'RELEASE.json' {
      Assert-DtwProbe (!$OutFile) 'Manifest must not be staged.'
      return [pscustomobject]@{RawContentStream=[IO.MemoryStream]::new($script:manifestBytes)}
    }
    $archiveName {
      [IO.File]::WriteAllBytes($OutFile, $script:zipBytes)
      return
    }
    "$archiveName.sha256" {
      [IO.File]::WriteAllText($OutFile, "$archiveSha *$($script:sidecarName)`r`n")
      return
    }
    default { throw "Unexpected download $Uri." }
  }
}

function Invoke-DtwRuntime {
  param([string]$Executable, [string[]]$Arguments, [string]$Staging)

  if (!$Staging) { Assert-DtwProbe ($Executable -ceq $installed) 'Expected the supplied registered runtime.' }
  if ($Arguments[0] -ceq '--help') {
    $script:calls += 'help'
    Assert-DtwProbe ($Arguments.Count -eq 1) 'Unexpected general help arguments.'
    $help = @('  dtw app catalog', '  dtw mcp', '  dtw package import|bind|activate|unbind|status|gc [--store <absolute-directory>]')
    if (!$script:oldRuntime) { $help += '  dtw setup [options]' }
    return $help -join "`r`n"
  }
  Assert-DtwProbe ($Arguments[0] -ceq 'setup') 'Expected only DTW setup calls.'
  if ($Arguments[1] -ceq '--help') {
    $script:calls += 'setup-help'
    Assert-DtwProbe ($Arguments.Count -eq 2) 'Unexpected setup help arguments.'
    return @{setup_schema=$script:setupSchema; manager_version='0.6.1'; store_schema=1} | ConvertTo-Json
  }

  Assert-DtwProbe ($Arguments[2] -ceq $hostName -and $Arguments[3] -ceq $profile) 'Setup host or profile was altered.'
  $reply = [ordered]@{host=$hostName; profile=$profile; manager_version='0.6.1'; store_schema=1}
  if ($Arguments[1] -ceq 'resume') {
    $script:calls += 'resume'
    Assert-DtwProbe ($Arguments.Count -eq 4) 'Expected only host and profile for resume.'
    $reply.state = $script:resumeState
    return $reply | ConvertTo-Json
  }
  Assert-DtwProbe ($Arguments[1] -ceq 'prepare' -and $Arguments[4] -ceq $packageId) 'Expected prepare for the manifest package ID.'
  $reply.package_id = $packageId
  if ($Staging) {
    $script:calls += 'prepare-source'
    Assert-DtwProbe ($Arguments.Count -eq 6) 'Expected a single SOURCE argument.'
    $script:reportedSource = $Arguments[5]
    $package = Join-Path (Join-Path $Staging 'expanded') $packageName
    $expectedExe = Join-Path (Join-Path $package 'bin') 'dtw.exe'
    Assert-DtwProbe ($Arguments[5] -ceq $package -and $Executable -ceq $expectedExe) 'Expected the verified archive runtime and extracted package source.'
    Assert-DtwProbe ((Get-Content -LiteralPath $Executable -Raw).Trim() -ceq 'detached placeholder') 'Expected fixture runtime bytes.'
    Assert-DtwProbe ((Split-Path -Parent $Staging) -ceq $stage) 'Staging must be beneath the approved root.'
    $reply.state = $script:sourceState
  } else {
    $script:calls += 'prepare-cached'
    Assert-DtwProbe ($Arguments.Count -eq 5) 'Cached prepare must omit SOURCE.'
    $reply.state = $script:cacheState
  }
  $reply.next = 'fixture host handoff'
  $script:prepareOutput = $reply | ConvertTo-Json
  return $script:prepareOutput
}

$caseRoot = Join-Path ([IO.Path]::GetTempPath()) ('dtw-bootstrap-probe-' + [guid]::NewGuid().ToString('N'))
$environment = @{}
foreach ($key in @('TEMP', 'TMP', 'LOCALAPPDATA', 'DTW_STAGING_DIR')) {
  $environment[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
}
try {
  $hostName = 'opencode'
  $profile = 'profile with spaces and "quotes"'
  $installed = 'C:\trusted registration\bin\dtw.exe'
  $target = 'x86_64-pc-windows-msvc'
  $packageName = "desktop-world-0.6.1-$target"
  $archiveName = "$packageName.zip"
  $packageId = 'd' * 64
  $stage = Join-Path $caseRoot 'approved stage'
  $env:LOCALAPPDATA = Join-Path $caseRoot 'isolated store root'
  $env:TEMP = $env:TMP = Join-Path $caseRoot 'redirected temp'
  $env:DTW_STAGING_DIR = $stage
  [void][IO.Directory]::CreateDirectory($env:TEMP)
  $missingExe = Join-Path $caseRoot 'owned missing executable.exe'
  Assert-DtwFailure { & $launchRuntime $missingExe @('--help') $stage } 'DTW launch failed ('
  if ($env:OS -eq 'Windows_NT') {
    Assert-DtwProbe ((Get-DtwStageRoot) -ceq $stage) 'Expected the approved absolute staging override.'
    $env:DTW_STAGING_DIR = ''
    $localAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    Assert-DtwProbe ((Get-DtwStageRoot) -ceq (Join-Path $localAppData 'dtw-stage')) 'Default staging must use real Local Application Data.'
    $env:DTW_STAGING_DIR = $stage
  } else {
    # The flow is tested on PowerShell 7 elsewhere with an owned native filesystem path.
    function Get-DtwStageRoot { return $stage }
  }
  $packageFixture = Join-Path $caseRoot $packageName
  $fixtureBin = Join-Path $packageFixture 'bin'
  [void][IO.Directory]::CreateDirectory($fixtureBin)
  $fixtureExe = Join-Path $fixtureBin 'dtw.exe'
  Set-Content -LiteralPath $fixtureExe -Value 'detached placeholder'
  $binarySha = Get-DtwFileSha256 $fixtureExe
  $archiveFixture = Join-Path $caseRoot $archiveName
  Compress-Archive -LiteralPath $packageFixture -DestinationPath $archiveFixture
  $archiveSha = Get-DtwFileSha256 $archiveFixture

  Assert-DtwProbe ((ConvertTo-DtwWindowsArgument 'C:\path with spaces\') -ceq '"C:\path with spaces\\"') 'Trailing backslash quoting is incorrect.'
  Assert-DtwProbe ((ConvertTo-DtwWindowsArgument 'a\"b') -ceq '"a\\\"b"') 'Embedded quote escaping is incorrect.'
  Reset-DtwFixture
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile 'dtw.exe' } 'absolute Windows path'
  Assert-DtwCalls @()
  $script:setupSchema = '1'
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile $installed } 'Unsupported DTW setup schema'
  Assert-DtwCalls @('help', 'setup-help')

  foreach ($pendingState in @('verify','restart_required','host_action_required')) {
    Reset-DtwFixture
    $script:resumeState = $pendingState
    $result = Invoke-DtwBootstrap $hostName $profile $installed | ConvertFrom-Json
    Assert-DtwProbe ($result.state -ceq $pendingState) 'Expected the pending resume result.'
    Assert-DtwCalls @('help', 'setup-help', 'resume')
  }
  Reset-DtwFixture
  $script:resumeState = 'unexpected'
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile $installed } 'Unexpected DTW setup resume state'
  Assert-DtwCalls @('help', 'setup-help', 'resume')

  Reset-DtwFixture
  $script:cacheState = 'restart_required'
  $result = Invoke-DtwBootstrap $hostName $profile $installed
  Assert-DtwProbe ($result -ceq $script:prepareOutput) 'Expected the unchanged cached setup receipt.'
  Assert-DtwCalls @('help', 'setup-help', 'resume', 'latest', 'RELEASE.json', 'prepare-cached')

  Reset-DtwFixture '0.6.0' -Legacy
  $script:oldRuntime = $true
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile $installed } 'blob/v0.6.0/install.md#windows-x64-download-and-verify-once'
  Assert-DtwCalls @('help', 'latest', 'RELEASE.json')
  Reset-DtwFixture '0.6.1' -Legacy
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile } 'Invalid Windows package identity'
  Assert-DtwCalls @('latest', 'RELEASE.json')

  Reset-DtwFixture
  $script:release.prerelease = 'false'
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile } 'Expected an official stable version tag'
  Assert-DtwCalls @('latest')
  Reset-DtwFixture
  $script:manifestBytes = [Text.Encoding]::UTF8.GetBytes('{}')
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile } 'RELEASE.json API digest mismatch'
  Assert-DtwCalls @('latest', 'RELEASE.json')
  Reset-DtwFixture
  $script:release.assets[2].browser_download_url = 'https://example.invalid/sidecar'
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile } 'Unexpected release asset URL'
  Assert-DtwCalls @('latest', 'RELEASE.json')
  Reset-DtwFixture
  $script:release.assets[1].digest = 'sha256:' + ('0' * 64)
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile } 'Archive release API digest does not match'
  Assert-DtwCalls @('latest', 'RELEASE.json')
  Assert-DtwProbe (!(Test-Path -LiteralPath $stage)) 'Early returns and metadata failures must leave staging untouched.'

  Reset-DtwFixture
  $result = Invoke-DtwBootstrap $hostName $profile $installed
  Assert-DtwProbe ($result -ceq $script:prepareOutput) 'Expected the unchanged prepare receipt after acquisition.'
  Assert-DtwCalls @('help', 'setup-help', 'resume', 'latest', 'RELEASE.json', 'prepare-cached', $archiveName, "$archiveName.sha256", 'prepare-source')
  Assert-DtwProbe (!(Test-Path -LiteralPath $script:reportedSource)) 'Successful acquisition left its source on disk.'
  Assert-DtwProbe (@(Get-ChildItem -LiteralPath $stage -Force).Count -eq 0) 'Successful acquisition left a staging child.'

  Reset-DtwFixture
  $result = Invoke-DtwBootstrap $hostName $profile
  Assert-DtwProbe ($result -ceq $script:prepareOutput) 'Expected the unchanged prepare receipt on first installation.'
  Assert-DtwCalls @('latest', 'RELEASE.json', $archiveName, "$archiveName.sha256", 'prepare-source')
  Assert-DtwProbe (@(Get-ChildItem -LiteralPath $stage -Force).Count -eq 0) 'First installation left a staging child.'

  Reset-DtwFixture
  $script:sidecarName = 'unexpected.zip'
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile } 'Invalid archive checksum sidecar'
  Assert-DtwCalls @('latest', 'RELEASE.json', $archiveName, "$archiveName.sha256")
  Reset-DtwFixture
  $script:zipBytes[0] = $script:zipBytes[0] -bxor 1
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile } 'Archive checksum does not match RELEASE.json'
  Assert-DtwCalls @('latest', 'RELEASE.json', $archiveName, "$archiveName.sha256")
  Reset-DtwFixture -WrongBinary
  Assert-DtwFailure { Invoke-DtwBootstrap $hostName $profile } 'Packaged DTW executable checksum does not match RELEASE.json'
  Assert-DtwCalls @('latest', 'RELEASE.json', $archiveName, "$archiveName.sha256")
  Assert-DtwProbe (@(Get-ChildItem -LiteralPath $stage -Force).Count -eq 3) 'Expected failed acquisition directories for diagnostics.'
  Assert-DtwProbe (!(Test-Path -LiteralPath $env:LOCALAPPDATA)) 'Detached test touched its isolated runtime store.'
  Write-Output 'Detached Windows bootstrap probe passed.'
} finally {
  foreach ($key in $environment.Keys) {
    [Environment]::SetEnvironmentVariable($key, $environment[$key], 'Process')
  }
  if (Test-Path -LiteralPath $caseRoot) { Remove-Item -LiteralPath $caseRoot -Recurse -Force }
}
