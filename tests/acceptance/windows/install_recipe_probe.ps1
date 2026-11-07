# Run the documented Windows recipe with redirected TEMP/TMP and an owned store.
param([Parameter(Mandatory)][string]$OutputDirectory, [string]$StagingDirectory = '')
$ErrorActionPreference = 'Stop'
$caseRoot = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $caseRoot) { throw 'Use a fresh output directory.' }
[void][IO.Directory]::CreateDirectory($caseRoot)
$installDoc = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../../../install.md') -Raw -Encoding UTF8
$recipe = [regex]::Match($installDoc, '(?ms)^### Windows x64:[^\n]*\r?\n.*?^\x60{3}powershell\r?\n(.*?)^\x60{3}').Groups[1].Value
if (!$recipe) { throw 'Windows recipe block missing.' }
$knownStage = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)) 'dtw-stage'
$original = @{}
foreach ($key in @('TEMP','TMP','LOCALAPPDATA','DTW_STAGING_DIR')) {
  $original[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
}
try {
  $redirected = Join-Path $caseRoot 'host-redirected-temp'
  [void][IO.Directory]::CreateDirectory($redirected)
  $env:TEMP = $env:TMP = $redirected
  $env:LOCALAPPDATA = Join-Path $caseRoot 'owned-local'
  $env:DTW_STAGING_DIR = $StagingDirectory
  $expectedStage = if ($StagingDirectory) { [IO.Path]::GetFullPath($StagingDirectory) } else { $knownStage }
  $output = @(& ([scriptblock]::Create($recipe)) 6>&1)
  $prefix = 'DTW download directory: '
  $message = $output | Where-Object { $_ -is [Management.Automation.InformationRecord] } |
    ForEach-Object { $_.ToString() } | Where-Object { $_.StartsWith($prefix) } | Select-Object -First 1
  if (!$message) { throw 'Recipe did not report its staging path.' }
  $stage = $message.Substring($prefix.Length)
  if ((Split-Path -Parent $stage) -ne $expectedStage -or (Test-Path -LiteralPath $stage)) {
    throw "Unexpected staging directory or cleanup failure: $stage"
  }
  $result = ($output | Where-Object { $_ -is [string] }) -join "`n" | ConvertFrom-Json
  $store = Join-Path $env:LOCALAPPDATA 'DesktopWorld'
  $managedExe = [string]$result.executable
  if ($managedExe.StartsWith('\\?\')) { $managedExe = $managedExe.Substring(4) }
  if (!$managedExe.StartsWith((Join-Path $store 'packages') + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The recipe did not import into the owned store.'
  }
  $status = & $managedExe package status | ConvertFrom-Json
  if ($LASTEXITCODE -ne 0 -or @($status.bindings).Count -ne 0 -or @($status.packages).Count -ne 1) {
    throw 'Unexpected managed store status.'
  }
  [ordered]@{passed=$true; stage=$stage; temp=$redirected; store=$store;
    version=$result.version; archive_sha256=$result.archive_sha256} |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $caseRoot 'report.json') -Encoding UTF8
} finally {
  foreach ($key in $original.Keys) {
    [Environment]::SetEnvironmentVariable($key, $original[$key], 'Process')
  }
}
