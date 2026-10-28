#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$Dtw,
    [string]$OutputDirectory,
    [switch]$CompileOnly,
    [switch]$IncludePointerTests
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSEdition -ne 'Desktop' -or $env:OS -ne 'Windows_NT') {
    throw 'Use the built-in Windows PowerShell 5.1 (powershell.exe), not pwsh. No GUI was started.'
}
$utf8 = New-Object Text.UTF8Encoding($false)
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path ([IO.Path]::GetTempPath()) ('dtw-windows-' + [guid]::NewGuid().ToString('N'))
}
$output = [IO.Path]::GetFullPath($OutputDirectory)
if ($output.StartsWith('\\') -or (Test-Path -LiteralPath $output)) {
    throw 'OutputDirectory must be a new local directory; existing evidence is never overwritten.'
}
[void][IO.Directory]::CreateDirectory($output)
function Write-Json($Path, $Value) {
    [IO.File]::WriteAllText($Path, (ConvertTo-Json -InputObject $Value -Depth 64), $script:utf8)
}
function Require($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Js($Value) { ConvertTo-Json -InputObject $Value -Compress -Depth 64 }
function Normalize([string]$Value) { $Value.Replace("`r`n", "`n") }
$fixtures = New-Object 'Collections.Generic.List[object]'
$client = $null
$serial = 0
$lastToolImageCount = 0
$executionImageCount = 0
$lastToolImageSizes = @()
$executionImageSizes = @()
$caseNames = New-Object 'Collections.Generic.List[string]'
$summary = [ordered]@{
    status = 'running'; passed = $false; evidence_kind = 'owned Windows fixture via standard stdio MCP'
    started_utc = [DateTime]::UtcNow.ToString('o'); compile_only = [bool]$CompileOnly
    pointer_tests = [bool]$IncludePointerTests; os = [Environment]::OSVersion.VersionString
    powershell = $PSVersionTable.PSVersion.ToString(); is_64_bit = [Environment]::Is64BitProcess
    failure = $null; cases = @(); cleanup = @(); source_hashes = @{}
    manual_pending = @('Cancellation during input and UIA blocking', 'Timeout and unknown-effect preservation',
        'Concurrent independent MCP owners', 'App exit/restart and surviving sibling refs',
        'EOF/parent exit native worker reclamation', 'Mixed/per-monitor DPI and monitor movement',
        'Permission/elevation/secure-desktop denial', 'Real application and live Agent workflows')
}
if (-not $IncludePointerTests) { $summary.manual_pending += 'Left/right/double click and wheel steps' }

function Read-State($Fixture) {
    $path = Join-Path $Fixture.directory 'state.json'
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    # The fixture replaces snapshots atomically. Allow replacement while reading
    # this immutable version, rather than making File.Replace fail on Windows.
    $stream = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read,
        ([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader = $null
    try {
        $reader = New-Object IO.StreamReader($stream, $script:utf8)
        $value = ConvertFrom-Json ($reader.ReadToEnd())
    } finally {
        if ($null -ne $reader) { $reader.Dispose() } else { $stream.Dispose() }
    }
    Require ($value.pid -eq $Fixture.process.Id) 'Fixture state PID does not match the owned process'
    return $value
}
function Wait-State($Fixture, [string]$Name, [scriptblock]$Predicate, [int]$Milliseconds = 4000) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $samples = New-Object 'Collections.Generic.List[object]'
    try {
        while ($watch.ElapsedMilliseconds -lt $Milliseconds) {
            Require (-not $Fixture.process.HasExited) ('Owned fixture exited: ' + $Fixture.label)
            $state = Read-State $Fixture
            if ($null -ne $state) {
                $samples.Add($state)
                if (& $Predicate $state) { return $state }
            }
            Start-Sleep -Milliseconds 25 # Independent state reads only; no DTW/input retries.
        }
        throw ('Independent fixture condition timed out: ' + $Name)
    } finally {
        Write-Json (Join-Path $script:output ($Name + '-samples.json')) @{
            elapsed_ms = $watch.ElapsedMilliseconds; limit_ms = $Milliseconds; samples = @($samples.ToArray())
        }
    }
}
function Wait-Released($Fixture, [string]$Name) {
    $track = @{ sequence = 0; first = $null; previous = $null; count = 0 }
    Wait-State $Fixture $Name {
        param($s)
        if ($s.sequence -le $track.sequence) { return $false }
        $track.sequence = $s.sequence
        if (@($s.keys_down_global).Count -ne 0 -or
            ($null -ne $track.previous -and $s.elapsed_ms - $track.previous -gt 150)) {
            $track.first = $null; $track.count = 0
        }
        $track.previous = $s.elapsed_ms
        if (@($s.keys_down_global).Count -eq 0) {
            if ($null -eq $track.first) { $track.first = $s.elapsed_ms }
            $track.count++
            return ($track.count -ge 3 -and $s.elapsed_ms - $track.first -ge 100)
        }
        return $false
    }
}
function Rpc([string]$Method, $Parameters) {
    $script:serial++
    $id = $script:serial
    $request = @{ jsonrpc = '2.0'; id = $id; method = $Method; params = $Parameters }
    $raw = Js $request
    [IO.File]::WriteAllText((Join-Path $script:output ("rpc-$id-request.json")), $raw, $script:utf8)
    $script:client.Send($raw)
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while ($watch.ElapsedMilliseconds -lt 20000) {
        $line = $script:client.Receive([Math]::Max(1, 20000 - [int]$watch.ElapsedMilliseconds))
        $message = ConvertFrom-Json $line
        if (-not $message.PSObject.Properties['id']) { continue }
        Require ($message.id -eq $id) 'Unexpected RPC identity; no request will be replayed'
        [IO.File]::WriteAllText((Join-Path $script:output ("rpc-$id-response.json")), $line, $script:utf8)
        if ($message.PSObject.Properties['error']) { throw ('JSON-RPC error: ' + (Js $message.error)) }
        return $message.result
    }
    throw 'MCP call timed out; preserve this request and inspect effects, never replay it'
}
function Tool([string]$Name, $Arguments) {
    $result = Rpc 'tools/call' @{ name = $Name; arguments = $Arguments }
    if ($result.PSObject.Properties['isError'] -and $result.isError -and $Name -notin @('exec', 'result')) {
        throw ('MCP tool returned isError: ' + (Js $result))
    }
    Require ($null -ne $result.PSObject.Properties['structuredContent']) 'Missing MCP structuredContent'
    $images = @($result.content | Where-Object { $_.type -eq 'image' })
    $script:lastToolImageCount = $images.Count
    $script:lastToolImageSizes = @()
    $index = 0
    foreach ($image in $images) {
        Require ($image.mimeType -eq 'image/png') 'Unexpected image type'
        $bytes = [Convert]::FromBase64String($image.data)
        Require ($bytes.Length -gt 8 -and [BitConverter]::ToString($bytes[0..7]) -eq '89-50-4E-47-0D-0A-1A-0A') 'Malformed PNG'
        $stream = New-Object IO.MemoryStream
        $decoded = $null
        try {
            $stream.Write($bytes, 0, $bytes.Length); $stream.Position = 0
            $decoded = [Drawing.Image]::FromStream($stream)
            $script:lastToolImageSizes += @{ width = $decoded.Width; height = $decoded.Height }
        } finally {
            if ($null -ne $decoded) { $decoded.Dispose() }; $stream.Dispose()
        }
        [IO.File]::WriteAllBytes((Join-Path $script:output ("rpc-$($script:serial)-image-$index.png")), $bytes)
        $index++
    }
    return $result.structuredContent
}
function Execute([string]$Name, [string]$Source) {
    $compact = Tool 'exec' @{ app = $script:appId; source = $Source }
    $script:executionImageCount = $script:lastToolImageCount
    $script:executionImageSizes = $script:lastToolImageSizes
    Write-Json (Join-Path $script:output ($Name + '-compact.json')) $compact
    if (-not $compact.PSObject.Properties['execution_id']) {
        throw ('Execution was not accepted: ' + (Js $compact))
    }
    Require ($compact.app -ceq $script:appId) 'Execution returned a different app identity'
    Require (-not [string]::IsNullOrWhiteSpace($compact.execution_id)) 'Missing execution_id'
    # Read the original ledger even for a failed execution; never execute a recovery retry.
    $full = Tool 'result' @{ app = $script:appId; execution_id = $compact.execution_id }
    Write-Json (Join-Path $script:output ($Name + '-ledger.json')) $full
    Require ($full.execution_id -ceq $compact.execution_id) 'Result changed the execution identity'
    Require ($full.status -eq 'completed' -and $null -eq $full.error) ('Execution failed: ' + (Js $full.error))
    Require ($full.script.state_committed -eq $true -and $null -eq $full.script.failure -and $null -eq $full.script.shutdown_error) 'Script did not finish cleanly'
    foreach ($step in $full.script.steps) {
        Require ($step.status -eq 'completed' -and $null -eq $step.error) ('A step failed: ' + (Js $step))
        if ($null -ne $step.receipt) {
            Require ($step.receipt.outcome.dispatch -eq 'dispatched' -and
                $step.receipt.outcome.evidence.kind -ne 'mismatch' -and
                $null -eq $step.receipt.outcome.error -and $null -eq $step.receipt.interruption) ('Uncertain action receipt: ' + (Js $step.receipt))
            if ($null -ne $step.receipt.provider_outcome) {
                Require ($step.receipt.provider_outcome.dispatch -eq 'dispatched' -and
                    $step.receipt.provider_outcome.evidence.kind -ne 'mismatch' -and
                    $null -eq $step.receipt.provider_outcome.error) ('Provider reported an action issue: ' + (Js $step.receipt))
            }
        }
    }
    $script:caseNames.Add($Name)
    return $full.value.result
}
function Peer-Fingerprint($State) {
    Js @($State.text, $State.saved, $State.saves, $State.increments, $State.left_up,
        $State.right_up, $State.double_clicks, $State.wheel_delta)
}
function Verify-Saved($Fixture, [string]$Expected, [int]$Saves, [string]$Name) {
    $state = Wait-State $Fixture $Name { param($s)
        $s.saves -eq $Saves -and $s.editor_focused -and (Normalize $s.saved) -ceq (Normalize $Expected)
    }
    $disk = [IO.File]::ReadAllText((Join-Path $Fixture.directory 'saved.txt'), $script:utf8)
    Require ((Normalize $disk) -ceq (Normalize $Expected)) 'Saved disk contents differ from expected Unicode text'
    Require ((Normalize $state.text) -ceq (Normalize $Expected)) 'Editor contents differ from saved text'
}
$find = @'
async function one(window_ref,role,name,action){
 let p=await app.observe({window_ref,role,text:name,limit:10}); const found=[];
 for(;;){found.push(...p.elements.filter(e=>e.name===name&&e.enabled!==false&&e.actions.includes(action)));
 if(!p.next)break;p=await app.observe({cursor:p.next});}
 if(!p.coverage.complete||found.length!==1)throw Error('Need one proven owned control: '+name);
 return {window_ref,observation_ref:p.observation_ref,element_ref:found[0].element_ref};
}
'@
try {
    foreach ($name in @('Fixture.cs', 'McpPipe.cs', 'smoke.ps1')) {
        $summary.source_hashes[$name] = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $PSScriptRoot $name)).Hash
    }
    $fixtureExe = Join-Path $output 'DtwOwnedFixture.exe'
    Add-Type -AssemblyName System.Drawing
    Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'Fixture.cs'), $utf8)) -Language CSharp `
        -ReferencedAssemblies @('System.dll','System.Windows.Forms.dll','System.Drawing.dll','System.Web.Extensions.dll') `
        -OutputAssembly $fixtureExe -OutputType WindowsApplication
    Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'McpPipe.cs'), $utf8)) -Language CSharp
    $summary.fixture_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $fixtureExe).Hash
    if ($CompileOnly) {
        $summary.status = 'compiled_only'; $summary.evidence_kind = 'Windows compiler only; no GUI or DTW execution'
    } else {
        Require (-not [string]::IsNullOrWhiteSpace($Dtw)) '-Dtw is required unless -CompileOnly is selected'
        $dtwPath = (Get-Item -LiteralPath $Dtw).FullName
        $summary.dtw_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $dtwPath).Hash
        foreach ($label in @('A', 'B')) {
            $directory = Join-Path $output $label
            [void][IO.Directory]::CreateDirectory($directory)
            $process = New-Object Diagnostics.Process
            $info = New-Object Diagnostics.ProcessStartInfo
            $info.FileName = $fixtureExe; $info.UseShellExecute = $false
            $info.Arguments = [DtwWindowsSmoke.McpPipe]::QuoteArgument($directory) + ' ' + [DtwWindowsSmoke.McpPipe]::QuoteArgument($label)
            $process.StartInfo = $info
            [void]$process.Start()
            $fixture = @{ label = $label; directory = $directory; process = $process }
            $fixtures.Add($fixture)
            $state = Wait-State $fixture ("ready-$label") { param($s)
                $s.ready -and $s.visible -and $s.bounds_ok -and $s.dpi_awareness_set
            }
            Require ($state.working_area.width -ge 900 -and $state.working_area.height -ge 520) 'Need a primary work area of at least 900 x 520 physical pixels'
            Require ($state.bounds.x + $state.bounds.width -le $state.working_area.x + $state.working_area.width -and
                $state.bounds.y + $state.bounds.height -le $state.working_area.y + $state.working_area.height) 'Owned fixture is not entirely on screen'
            [void](Wait-Released $fixture ("initial-release-$label"))
        }
        $client = New-Object DtwWindowsSmoke.McpPipe($dtwPath, $output)
        [void](Rpc 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'dtw-owned-windows-smoke'; version = '1' } })
        $client.Send((Js @{ jsonrpc = '2.0'; method = 'notifications/initialized' }))
        $catalog = Tool 'apps' @{}
        Write-Json (Join-Path $output 'catalog.json') $catalog
        $owned = @($catalog.apps | Where-Object { [StringComparer]::OrdinalIgnoreCase.Equals($_.app, $fixtureExe) })
        Require ($owned.Count -eq 1 -and $owned[0].instances -eq 2) 'The two owned processes were not aggregated as one app with two instances'
        $appId = $owned[0].app
        $inventory = Execute 'inventory' @'
const inventory=await app.windows();
if(inventory.coverage.complete_within_scope!==true)throw Error('Incomplete owned inventory');
for(const label of ['A','B']){const found=inventory.windows.filter(w=>w.title==='DTW Windows Smoke '+label);
if(found.length!==1)throw Error('Owned window missing or ambiguous: '+label);state[label]=found[0].window_ref;}
if(state.A===state.B)throw Error('Distinct instances share a window ref');return inventory;
'@
        Require (@($inventory.windows).Count -eq 2) 'Unexpected extra window in the owned application'
        foreach ($i in 0..1) {
            $f = $fixtures[$i]; $peer = $fixtures[1 - $i]; $label = $f.label
            $beforePeer = Peer-Fingerprint (Read-State $peer)
            $window = 'state[' + (Js $label) + ']'
            [void](Execute ("press-$label") ($find + "`nconst w=$window;await app.focus({window_ref:w});return await app.press(await one(w,'button','Increment','press'));"))
            [void](Wait-State $f ("increment-$label") { param($s) $s.increments -eq 1 })
            $setter = 'Setter ' + $label
            [void](Execute ("setter-$label") ($find + "`nconst w=$window;await app.setValue({...await one(w,'text_field','Message','setValue'),value:" + (Js $setter) + "});return await app.press(await one(w,'button','Save','press'));"))
            Verify-Saved $f $setter 1 ("setter-disk-$label")
            $unicode = $label + ' ' + ([char]0x4E2D).ToString() + ([char]0x6587).ToString() + ' ' + [char]::ConvertFromUtf32(0x1F600) + "`r`nSecond line"
            [void](Execute ("unicode-$label") ("const window_ref=$window;await app.focus({window_ref});await app.keypress({window_ref,keys:['CTRL','A']});await app.typeText({window_ref,text:" + (Js $unicode) + "});return await app.keypress({window_ref,keys:['CTRL','S']});"))
            Verify-Saved $f $unicode 2 ("unicode-disk-$label")
            [void](Wait-Released $f ("release-$label"))
            Require ((Peer-Fingerprint (Read-State $peer)) -ceq $beforePeer) 'Input or semantic action changed the other owned instance'
            $shot = Execute ("capture-$label") ("const window_ref=$window;await app.focus({window_ref});return await app.capture({window_ref});")
            Require ($shot.image.width -gt 0 -and $shot.image.height -gt 0) 'Capture has no pixel dimensions'
            Require ($executionImageCount -eq 1) 'Capture did not return one PNG in MCP content'
            Require ($executionImageSizes[0].width -eq $shot.image.width -and $executionImageSizes[0].height -eq $shot.image.height) 'PNG dimensions differ from the capture descriptor'
        }
        if ($IncludePointerTests) {
            $f = $fixtures[0]
            $beforePeer = Peer-Fingerprint (Read-State $fixtures[1])
            foreach ($item in @(@{ name = 'left'; button = 'left'; count = 1 }, @{ name = 'right'; button = 'right'; count = 1 }, @{ name = 'double'; button = 'left'; count = 2 })) {
                $before = Read-State $f
                $x = ($before.pad_point.x - $before.bounds.x) / [double]$before.bounds.width
                $y = ($before.pad_point.y - $before.bounds.y) / [double]$before.bounds.height
                $source = 'const window_ref=state.A;await app.focus({window_ref});const p=await app.capture({window_ref});return await app.click({window_ref,observation_ref:p.observation_ref,x:p.image.width*' + (Js $x) + ',y:p.image.height*' + (Js $y) + ',button:' + (Js $item.button) + ',count:' + $item.count + '});'
                [void](Execute ('pointer-' + $item.name) $source)
                [void](Wait-State $f ('pointer-state-' + $item.name) { param($s)
                    if ($item.name -eq 'right') { return $s.right_up -eq $before.right_up + 1 }
                    return ($s.left_up -eq $before.left_up + $item.count -and ($item.name -ne 'double' -or $s.double_clicks -eq $before.double_clicks + 1))
                })
            }
            $before = Read-State $f
            [void](Execute 'wheel' "return await app.scroll({window_ref:state.A,delta_y:3,delta_x:0});")
            [void](Wait-State $f 'wheel-state' { param($s) $s.wheel_delta -eq $before.wheel_delta - 360 })
            [void](Wait-Released $f 'pointer-release')
            Require ((Peer-Fingerprint (Read-State $fixtures[1])) -ceq $beforePeer) 'Pointer input changed the peer instance'
        }
        foreach ($f in $fixtures) {
            $finalState = Wait-Released $f ('final-release-' + $f.label)
            Require ($finalState.increments -eq 1 -and $finalState.saves -eq 2) 'Unexpected final effect count in owned fixture'
            Write-Json (Join-Path $output ('final-' + $f.label + '.json')) $finalState
            $held = @{}
            foreach ($line in [IO.File]::ReadAllLines((Join-Path $f.directory 'events.jsonl'), $utf8)) {
                $event = ConvertFrom-Json $line
                if ($event.kind -eq 'key_down') {
                    $key = [string]$event.data.key
                    if (-not $held.ContainsKey($key)) { $held[$key] = 0 }; $held[$key]++
                } elseif ($event.kind -eq 'key_up') {
                    $key = [string]$event.data.key
                    Require ($held.ContainsKey($key) -and $held[$key] -gt 0) 'Fixture saw an unmatched key release'
                    $held[$key]--
                }
            }
            Require (@($held.Values | Where-Object { $_ -ne 0 }).Count -eq 0) 'Fixture saw key down without matching release'
        }
        $summary.status = 'smoke_passed'; $summary.passed = $true
    }
} catch {
    $summary.status = 'failed'; $summary.passed = $false; $summary.failure = $_.ToString()
    [IO.File]::WriteAllText((Join-Path $output 'failure.txt'), ($_ | Out-String), $utf8)
} finally {
    if ($null -ne $client) {
        try {
            $cleanup = $client.Close(); $summary.cleanup += $cleanup
            if (-not $cleanup.exited -or $cleanup.forced -or $cleanup.exit_code -ne 0 -or
                -not $cleanup.readers_joined -or $cleanup.cleanup_error -or $cleanup.transport_error) {
                $summary.status = 'failed'; $summary.passed = $false
                if (-not $summary.failure) { $summary.failure = 'MCP cleanup was not confirmed; inspect cleanup records' }
            }
        } catch {
            $summary.status = 'failed'; $summary.passed = $false
            $summary.cleanup += @{ mcp = $true; confirmed = $false; error = $_.ToString() }
            if (-not $summary.failure) { $summary.failure = 'MCP cleanup threw; process reclamation is unconfirmed' }
        }
    }
    foreach ($f in $fixtures) {
        $forced = $false; $cleanupError = $null
        try {
            if (-not $f.process.HasExited) {
                [IO.File]::WriteAllText((Join-Path $f.directory 'stop'), 'stop', $utf8)
                if (-not $f.process.WaitForExit(3000)) { $forced = $true; $f.process.Kill(); [void]$f.process.WaitForExit(1000) }
            }
        } catch { $cleanupError = $_.ToString() }
        $exited = $f.process.HasExited
        $code = $null; if ($exited) { $code = $f.process.ExitCode }
        $summary.cleanup += @{ fixture = $f.label; pid = $f.process.Id; exited = $exited; forced = $forced; exit_code = $code; error = $cleanupError }
        if (-not $exited -or $forced -or $code -ne 0 -or $cleanupError) {
            $summary.status = 'failed'; $summary.passed = $false
            if (-not $summary.failure) { $summary.failure = 'Owned fixture cleanup failed' }
        }
    }
    $summary.cases = @($caseNames.ToArray()); $summary.finished_utc = [DateTime]::UtcNow.ToString('o')
    if ($summary.status -eq 'failed' -and -not (Test-Path -LiteralPath (Join-Path $output 'failure.txt'))) {
        [IO.File]::WriteAllText((Join-Path $output 'failure.txt'), $summary.failure + "`n" + (Js $summary.cleanup), $utf8)
    }
    Write-Json (Join-Path $output 'summary.json') $summary
    Write-Host ('Evidence: ' + $output)
}
if ($summary.status -eq 'failed') { throw ('Windows smoke did not pass: ' + $summary.failure) }
Write-Host $summary.status
