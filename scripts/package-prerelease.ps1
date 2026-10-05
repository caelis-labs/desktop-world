param([Parameter(Mandatory=$true)][string]$Version)
$ErrorActionPreference = 'Stop'
$packageRoot = Split-Path -Parent $PSScriptRoot
Push-Location $packageRoot
$savedGoWork, $savedGOOS, $savedGOARCH, $savedCGO = $env:GOWORK, $env:GOOS, $env:GOARCH, $env:CGO_ENABLED
try {
    function Invoke-Package { param([string]$Program, [string[]]$Arguments)
        & $Program @Arguments
        if ($LASTEXITCODE -ne 0) { throw "$Program failed ($LASTEXITCODE)" }
    }
    if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+-[a-zA-Z0-9.-]+$') { throw 'prerelease version required' }
    if (-not $IsWindows -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'build on Windows amd64 with PowerShell 7' }
    if (git status --porcelain) { throw 'clean checkout required' }
    $packageRevision = git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'cannot resolve HEAD' }
    git show-ref --verify --quiet "refs/tags/$Version"
    if ($LASTEXITCODE -eq 0) {
        $tagRevision = git rev-parse "$Version^{commit}"
        if ($tagRevision -ne $packageRevision) { throw 'tag differs from HEAD' }
    }
    $packageName = "desktop-world-$Version-windows-amd64"
    $packageOut = Join-Path $packageRoot "artifacts/release/$Version"
    if (Test-Path -LiteralPath $packageOut) { throw "output already exists: $packageOut" }
    $packagePayload = Join-Path $packageOut $packageName
    New-Item -ItemType Directory -Path "$packagePayload/bin", "$packagePayload/source" | Out-Null
    $env:GOWORK, $env:GOOS, $env:GOARCH, $env:CGO_ENABLED = 'off', 'windows', 'amd64', '0'
    Invoke-Package go @('build', '-trimpath', '-buildvcs=true', '-ldflags', "-X main.releaseVersion=$Version", '-o', "$packagePayload/bin/dtw.exe", './cmd/dtw')
    $packageManifest = (& "$packagePayload/bin/dtw.exe" version | ConvertFrom-Json -AsHashtable)
    if ($LASTEXITCODE -ne 0 -or $packageManifest.version -ne $Version -or
        $packageManifest.'vcs.revision' -ne $packageRevision -or $packageManifest.'vcs.modified' -ne 'false' -or
        $packageManifest.os -ne 'windows' -or $packageManifest.arch -ne 'amd64') { throw 'binary version/commit mismatch' }
    $packageManifest.license = 'MPL-2.0'
    $packageManifest.signing = 'unsigned'
    $packageManifest.validated_windows = 'Windows 11 amd64, interactive desktop, single display'
    $packageManifest.requirements = 'Application UIA support; same or lower integrity level; OS foreground restrictions apply'
    $packageManifest.binary_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath "$packagePayload/bin/dtw.exe").Hash.ToLowerInvariant()
    $packageManifest | ConvertTo-Json | Set-Content -Encoding utf8NoBOM -LiteralPath "$packagePayload/manifest.json"
    $packageSourceZip = Join-Path $packageOut "desktop-world-$Version-source.zip"
    Invoke-Package git @('archive', '--format=zip', '--output', $packageSourceZip, 'HEAD')
    Expand-Archive -LiteralPath $packageSourceZip -DestinationPath "$packagePayload/source"
    foreach ($file in @('README.md', 'HANDOFF.md', 'LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.md')) {
        Copy-Item -LiteralPath (Join-Path $packageRoot $file) -Destination $packagePayload
    }
    foreach ($folder in @('docs', 'clients', 'skills', 'examples')) {
        Copy-Item -LiteralPath (Join-Path $packageRoot $folder) -Destination $packagePayload -Recurse
    }
    $packageZip = Join-Path $packageOut "$packageName.zip"
    Compress-Archive -LiteralPath $packagePayload -DestinationPath $packageZip -CompressionLevel Optimal
    @($packageZip, $packageSourceZip) | ForEach-Object {
        $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_).Hash.ToLowerInvariant()
        "$hash  $(Split-Path -Leaf $_)"
    } | Set-Content -Encoding ascii -LiteralPath (Join-Path $packageOut 'SHA256SUMS')
    Write-Output $packageOut
} finally {
    $env:GOWORK, $env:GOOS, $env:GOARCH, $env:CGO_ENABLED = $savedGoWork, $savedGOOS, $savedGOARCH, $savedCGO
    Pop-Location
}
