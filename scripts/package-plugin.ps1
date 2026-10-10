param([Parameter(Mandatory=$true)][string]$Version)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
$old = @($env:GOWORK, $env:GOOS, $env:GOARCH, $env:CGO_ENABLED)
try {
  function Invoke-Checked { param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed ($LASTEXITCODE)" }
  }
  if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[1-9][0-9]*)?$') { throw 'stable or numbered release candidate version required' }
  if (-not $IsWindows -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'build on Windows amd64 with PowerShell 7' }
  if (git status --porcelain) { throw 'clean checkout required' }
  $revision = (git rev-parse HEAD).Trim()
  $out = Join-Path $repo "artifacts/release/$Version"
  if (Test-Path -LiteralPath $out) { throw "output already exists: $out" }
  $stage = Join-Path $out 'staging'
  New-Item -ItemType Directory -Force -Path $stage | Out-Null
  $env:GOWORK, $env:GOOS, $env:GOARCH, $env:CGO_ENABLED = 'off', 'windows', 'amd64', '0'
  $helper = Join-Path $stage 'dtw.exe'
  Invoke-Checked go @('build', '-trimpath', '-buildvcs=true', '-ldflags', "-X main.releaseVersion=$Version", '-o', $helper, './cmd/dtw')
  Push-Location (Join-Path $repo 'clients/mcp')
  try { Invoke-Checked npm @('ci'); Invoke-Checked npm @('run', 'build') } finally { Pop-Location }
  $nodeVersion = 'v24.21.0'
  $archive = "node-$nodeVersion-win-x64.zip"
  $archivePath = Join-Path $stage $archive
  Invoke-WebRequest -Uri "https://nodejs.org/dist/$nodeVersion/$archive" -OutFile $archivePath
  $hashes = Join-Path $stage 'SHASUMS256.txt'
  Invoke-WebRequest -Uri "https://nodejs.org/dist/$nodeVersion/SHASUMS256.txt" -OutFile $hashes
  $expected = ((Get-Content -LiteralPath $hashes | Where-Object { $_ -match "  $([regex]::Escape($archive))$" }) -split '\s+')[0].ToLowerInvariant()
  $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
  if (-not $expected -or $expected -ne $actual) { throw 'official Node archive checksum mismatch' }
  Expand-Archive -LiteralPath $archivePath -DestinationPath $stage
  $nodeRoot = Join-Path $stage "node-$nodeVersion-win-x64"
  $archives = @()
  foreach ($flavor in @('full', 'lite')) {
    $name = "desktop-world-plugin-$Version-$flavor-windows-amd64"
    $payload = Join-Path $out $name
    Invoke-Checked node @('scripts/build-plugin-package.mjs', $Version, 'windows-amd64', $flavor, $helper, (Join-Path $nodeRoot 'node.exe'), (Join-Path $nodeRoot 'LICENSE'), $actual, $payload)
    Invoke-Checked node @('clients/mcp/verify-package.mjs', $payload)
    $zip = Join-Path $out "$name.zip"
    Compress-Archive -LiteralPath $payload -DestinationPath $zip -CompressionLevel Optimal
    $archives += $zip
  }
  $lines = @($archives | ForEach-Object { "$((Get-FileHash -Algorithm SHA256 -LiteralPath $_).Hash.ToLowerInvariant())  $(Split-Path -Leaf $_)" })
  [IO.File]::WriteAllText((Join-Path $out 'SHA256SUMS'), ($lines -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
  Remove-Item -LiteralPath $stage -Recurse -Force
  Write-Output "revision=$revision full=$($archives[0]) lite=$($archives[1])"
} finally {
  $env:GOWORK, $env:GOOS, $env:GOARCH, $env:CGO_ENABLED = $old
  Pop-Location
}
