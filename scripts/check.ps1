# Cross-platform contracts and Windows native build; no real desktop input.
$ErrorActionPreference = 'Stop'
$checkRoot = Split-Path -Parent $PSScriptRoot
Push-Location $checkRoot
$previousGoWork = $env:GOWORK
try {
    $env:GOWORK = 'off'
    function Invoke-Check {
        param([string]$Program, [string[]]$Arguments)
        & $Program @Arguments
        if ($LASTEXITCODE -ne 0) { throw "$Program check failed ($LASTEXITCODE)" }
    }
    Invoke-Check go @('test', '-race', './...')
    Invoke-Check go @('vet', './...')
    Invoke-Check go @('build', './...')
    Invoke-Check go @('run', './examples/headless')
    Invoke-Check go @('run', './examples/embodied')
    Invoke-Check python @('verify_examples.py')
    Invoke-Check node @('--test', 'clients/javascript/desktop.test.mjs')
    Invoke-Check python @('scripts/check-sdks.py')
} finally {
    $env:GOWORK = $previousGoWork
    Pop-Location
}
