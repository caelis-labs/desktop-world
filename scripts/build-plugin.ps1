param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string] $Destination
)

$ErrorActionPreference = 'Stop'
$expected = 'ac967b9006aafa10870aadb5a645b038cba9721a'
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
if (Get-Item -LiteralPath $destinationPath -Force -ErrorAction SilentlyContinue) {
    throw "Destination must be new: $destinationPath"
}

function Assert-ExitCode([string] $Action) {
    if ($LASTEXITCODE -ne 0) { throw "$Action failed with exit code $LASTEXITCODE" }
}

$ownedSource = $null
$stage = $null
try {
    if ($env:CUA_SOURCE_ROOT) {
        $sourceRoot = (Resolve-Path -LiteralPath $env:CUA_SOURCE_ROOT).Path
    } else {
        $ownedSource = Join-Path ([System.IO.Path]::GetTempPath()) "dtw-cua-source-$([Guid]::NewGuid().ToString('N'))"
        New-Item -ItemType Directory -Path $ownedSource | Out-Null
        $sourceRoot = $ownedSource
        & git -C $sourceRoot init -q
        Assert-ExitCode 'git init'
        & git -C $sourceRoot remote add origin https://github.com/trycua/cua.git
        Assert-ExitCode 'git remote add'
        & git -C $sourceRoot fetch -q --no-tags --depth 1 origin $expected
        Assert-ExitCode 'git fetch'
        & git -C $sourceRoot checkout -q --detach FETCH_HEAD
        Assert-ExitCode 'git checkout'
    }

    $actual = (& git -C $sourceRoot rev-parse HEAD | Out-String).Trim()
    Assert-ExitCode 'git rev-parse'
    $dirty = (@(& git -C $sourceRoot status --porcelain) -join "`n").Trim()
    Assert-ExitCode 'git status'
    if ($actual -ne $expected -or $dirty) {
        throw "CUA source must be clean at $expected; got $actual"
    }

    $buildRoot = if ($env:DTW_BUILD_DIR) {
        [System.IO.Path]::GetFullPath($env:DTW_BUILD_DIR)
    } else {
        Join-Path ([System.IO.Path]::GetTempPath()) 'dtw-cua-build'
    }
    $dtwTarget = Join-Path $buildRoot 'dtw'
    $cuaTarget = Join-Path $buildRoot 'cua'
    New-Item -ItemType Directory -Force -Path $dtwTarget, $cuaTarget | Out-Null
    $offline = if ($env:DTW_CARGO_OFFLINE -eq '1') { @('--offline') } else { @() }

    $env:CARGO_NET_GIT_FETCH_WITH_CLI = 'true'
    $env:CARGO_TARGET_DIR = $dtwTarget
    & cargo +1.98.0 build --manifest-path (Join-Path $repoRoot 'cmd/dtw/Cargo.toml') --release --locked @offline
    Assert-ExitCode 'build DTW'

    if ($env:CUA_DRIVER_BIN) {
        if (-not $env:CUA_DRIVER_BIN_SHA256) {
            throw 'CUA_DRIVER_BIN_SHA256 is required with CUA_DRIVER_BIN'
        }
        $driverBinary = (Resolve-Path -LiteralPath $env:CUA_DRIVER_BIN).Path
        $hash = (Get-FileHash -LiteralPath $driverBinary -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne $env:CUA_DRIVER_BIN_SHA256.ToLowerInvariant()) {
            throw "CUA worker SHA-256 mismatch: expected $env:CUA_DRIVER_BIN_SHA256, got $hash"
        }
    } else {
        $env:CARGO_TARGET_DIR = $cuaTarget
        & cargo +1.98.0 build --manifest-path (Join-Path $sourceRoot 'libs/cua-driver/rust/Cargo.toml') --bin cua-driver --release --locked @offline
        Assert-ExitCode 'build CUA worker'
        $driverBinary = Join-Path $cuaTarget 'release/cua-driver.exe'
    }

    $parent = [System.IO.Path]::GetDirectoryName($destinationPath)
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    $stage = Join-Path $parent ".desktop-world-stage-$([Guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path (Join-Path $stage 'bin'), (Join-Path $stage 'libexec'), (Join-Path $stage 'skills') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $dtwTarget 'release/dtw.exe') -Destination (Join-Path $stage 'bin/dtw.exe')
    Copy-Item -LiteralPath $driverBinary -Destination (Join-Path $stage 'libexec/cua-driver.exe')
    Copy-Item -LiteralPath (Join-Path $repoRoot 'packaging/plugin/plugin.json') -Destination $stage
    $mcp = Get-Content -LiteralPath (Join-Path $repoRoot 'packaging/plugin/mcp.json') -Raw | ConvertFrom-Json
    $mcp.mcpServers.'desktop-world'.command = './bin/dtw.exe'
    $mcp | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $stage 'mcp.json') -Encoding utf8NoBOM
    Copy-Item -LiteralPath (Join-Path $repoRoot 'packaging/plugin/skills/desktop-world') -Destination (Join-Path $stage 'skills/desktop-world') -Recurse
    foreach ($name in @('LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.md')) {
        Copy-Item -LiteralPath (Join-Path $repoRoot $name) -Destination $stage
    }
    Copy-Item -LiteralPath (Join-Path $sourceRoot 'LICENSE.md') -Destination (Join-Path $stage 'CUA-MIT-LICENSE.txt')
    Copy-Item -LiteralPath (Join-Path $sourceRoot 'libs/cua-driver/rust/THIRD_PARTY_NOTICES.md') -Destination (Join-Path $stage 'CUA-THIRD_PARTY_NOTICES.md')

    $paths = @(
        'bin/dtw.exe', 'libexec/cua-driver.exe', 'plugin.json', 'mcp.json',
        'skills/desktop-world/SKILL.md', 'skills/desktop-world/api.d.ts',
        'skills/desktop-world/RECIPES.md', 'LICENSE', 'NOTICE',
        'CUA-MIT-LICENSE.txt', 'THIRD_PARTY_NOTICES.md', 'CUA-THIRD_PARTY_NOTICES.md'
    )
    $checksums = foreach ($path in $paths) {
        $file = Join-Path $stage ($path.Replace('/', [System.IO.Path]::DirectorySeparatorChar))
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $path"
    }
    [System.IO.File]::WriteAllLines((Join-Path $stage 'SHA256SUMS'), $checksums)

    & (Join-Path $stage 'bin/dtw.exe') --version
    Assert-ExitCode 'DTW version'
    if (Get-Item -LiteralPath $destinationPath -Force -ErrorAction SilentlyContinue) {
        throw "Destination now exists: $destinationPath"
    }
    Move-Item -LiteralPath $stage -Destination $destinationPath
    $stage = $null
    Write-Output "package: $destinationPath"
} finally {
    if ($stage -and (Test-Path -LiteralPath $stage)) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
    if ($ownedSource -and (Test-Path -LiteralPath $ownedSource)) {
        Remove-Item -LiteralPath $ownedSource -Recurse -Force
    }
}
