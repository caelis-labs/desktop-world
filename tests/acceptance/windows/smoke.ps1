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
$snapshotSerial = 0
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
# Direct effect comparisons require a complete publication, using the same
# bounded, independent readiness reads as Wait-State.
# This retries only a snapshot read, never a DTW request or application action.
function Read-Snapshot($Fixture) {
    $script:snapshotSerial++
    Wait-State $Fixture ("snapshot-" + $script:snapshotSerial + "-" + $Fixture.label) { param($s) $true }
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
function Execute([string]$Name, [string]$Source, [switch]$ExpectStale) {
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
    if ($ExpectStale) {
        # A settled script exception has status completed. The step's error and
        # original receipt carry the rejection; status is not effect success.
        Require ($full.status -eq 'completed' -and $null -ne $full.error -and
            $full.error.code -eq 'script_exception' -and $null -ne $full.script.failure -and
            $full.script.failure.code -in @('stale_observation', 'target_changed') -and
            $full.script.state_committed -eq $false -and $null -eq $full.script.shutdown_error) 'Expected a settled stale-reference script exception'
        $failedSteps = @($full.script.steps | Where-Object { $null -ne $_.error })
        Require ($failedSteps.Count -eq 1 -and @($full.script.steps).Count -eq 1 -and
            $failedSteps[0].status -eq 'completed') 'Expected one settled stale-reference rejection'
        Require ($failedSteps[0].error.code -in @('stale_observation', 'target_changed')) 'Reference failed for a reason other than changed geometry'
        foreach ($step in $full.script.steps) {
            if ($null -ne $step.receipt) {
                Require ($step.receipt.outcome.dispatch -eq 'not_dispatched') 'Rejected reference dispatched or has an unknown effect'
                if ($null -ne $step.receipt.provider_outcome) {
                    Require ($step.receipt.provider_outcome.dispatch -eq 'not_dispatched') 'Provider dispatched a rejected reference'
                }
            }
        }
        $script:caseNames.Add($Name)
        return $full
    }
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
function Change-Layout($Fixture, [int]$WindowOffset, [int]$ControlOffset, [string]$Name) {
    $requested = (Read-Snapshot $Fixture).layout_sequence + 1
    $path = Join-Path $Fixture.directory 'layout.json'
    Write-Json ($path + '.next') @{ sequence = $requested; window_offset = $WindowOffset; control_offset = $ControlOffset }
    # Windows PowerShell 5.1 coerces $null to an empty string for this .NET
    # String parameter. File.Replace needs an actual null backup filename.
    if (Test-Path -LiteralPath $path) { [IO.File]::Replace(($path + '.next'), $path, [NullString]::Value) }
    else { [IO.File]::Move(($path + '.next'), $path) }
    [void](Wait-State $Fixture $Name { param($s) $s.layout_sequence -eq $requested })
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
            $beforePeer = Peer-Fingerprint (Read-Snapshot $peer)
            $window = 'state[' + (Js $label) + ']'
            [void](Execute ("press-$label") ("const window_ref=$window;await app.focus({window_ref});const selector={role:'button',text:'button Increment'};const f=await app.find({window_ref,selector,timeout_ms:0});if(f.element.name!=='Increment')throw Error('Wrong unique button');return await app.press({window_ref,selector,timeout_ms:0});"))
            [void](Wait-State $f ("increment-$label") { param($s) $s.increments -eq 1 })
            $setter = 'Setter ' + $label
            [void](Execute ("setter-$label") ("const window_ref=$window;await app.setValue({window_ref,selector:{role:'text_field',text:'Message'},value:" + (Js $setter) + ",timeout_ms:0});return await app.press({window_ref,selector:{role:'button',text:'button Save'},timeout_ms:0});"))
            Verify-Saved $f $setter 1 ("setter-disk-$label")
            $unicode = $label + ' ' + ([char]0x4E2D).ToString() + ([char]0x6587).ToString() + ' ' + [char]::ConvertFromUtf32(0x1F600) + "`r`nSecond line"
            [void](Execute ("unicode-$label") ("const window_ref=$window;await app.focus({window_ref});await app.keypress({window_ref,keys:['CTRL','A']});await app.typeText({window_ref,text:" + (Js $unicode) + "});return await app.keypress({window_ref,keys:['CTRL','S']});"))
            Verify-Saved $f $unicode 2 ("unicode-disk-$label")
            $readback = Execute ("unicode-pages-$label") ("const window_ref=$window;const ref=await app.find({window_ref,selector:{role:'text_field',text:'Message'},timeout_ms:0});let offset=0,text='',pages=0;for(;;){const p=await app.readText({window_ref,observation_ref:ref.observation_ref,element_ref:ref.element_ref,offset,limit:3});pages++;if(p.offset!==offset)throw Error('Offset changed');text+=p.text;if(p.next_offset===null){if(text.length!==p.total_length)throw Error('Wrong UTF16 length');return {text,pages};}if(p.next_offset<=offset)throw Error('Pagination did not advance');offset=p.next_offset;}")
            Require ($readback.pages -gt 1 -and (Normalize $readback.text) -ceq (Normalize $unicode)) 'Native Unicode pages differ from the owned editor text'

            [void](Wait-Released $f ("release-$label"))
            Require ((Peer-Fingerprint (Read-Snapshot $peer)) -ceq $beforePeer) 'Input or semantic action changed the other owned instance'
            $shot = Execute ("capture-$label") ("const window_ref=$window;await app.focus({window_ref});return await app.capture({window_ref});")
            Require ($shot.image.width -gt 0 -and $shot.image.height -gt 0) 'Capture has no pixel dimensions'
            Require ($executionImageCount -eq 1) 'Capture did not return one PNG in MCP content'
            Require ($executionImageSizes[0].width -eq $shot.image.width -and $executionImageSizes[0].height -eq $shot.image.height) 'PNG dimensions differ from the capture descriptor'
        }
        $f = $fixtures[0]
        foreach ($case in @(
            @{ name = 'moved-window-press'; field = 'increment_bounds'; role = 'button'; text = 'Increment'; operation = 'press'; window = 12; control = 0 },
            @{ name = 'moved-control-press'; field = 'increment_bounds'; role = 'button'; text = 'Increment'; operation = 'press'; window = 0; control = 200 },
            @{ name = 'moved-control-setValue'; field = 'editor_bounds'; role = 'text_field'; text = 'Message'; operation = 'setValue'; window = 0; control = 200 },
            @{ name = 'moved-window-readText'; field = 'editor_bounds'; role = 'text_field'; text = 'Message'; operation = 'readText'; window = 12; control = 0 },
            @{ name = 'moved-control-readText'; field = 'editor_bounds'; role = 'text_field'; text = 'Message'; operation = 'readText'; window = 0; control = 200 }
        )) {
            Change-Layout $f 0 0 ($case.name + '-reset')
            $before = Read-Snapshot $f
            $beforePeer = Peer-Fingerprint (Read-Snapshot $fixtures[1])
            $bounds = $before.($case.field)
            $region = @{ x = $bounds.x - $before.bounds.x; y = $bounds.y - $before.bounds.y; width = $bounds.width; height = $bounds.height }
            $watch = [Diagnostics.Stopwatch]::StartNew()
            $source = "const inv=await app.windows();const w=inv.windows.filter(w=>w.title==='DTW Windows Smoke A');if(w.length!==1)throw Error('Owned window not unique');const window_ref=w[0].window_ref;state.A=window_ref;await app.focus({window_ref});const shot=await app.capture({window_ref});let p=await app.observe({window_ref,observation_ref:shot.observation_ref,region:" + (Js $region) + ",role:" + (Js $case.role) + ",text:" + (Js $case.text) + ",limit:10});const rows=[];for(;;){rows.push(...p.elements);if(!p.next)break;p=await app.observe({cursor:p.next});}if(!p.coverage.complete||rows.length!==1)throw Error('Region is not complete and unique');state.regionRef={window_ref,observation_ref:p.observation_ref,element_ref:rows[0].element_ref};return state.regionRef;"
            [void](Execute ($case.name + '-observe') $source)
            Change-Layout $f $case.window $case.control ($case.name + '-changed')
            $source = 'return await app.' + $case.operation + '({...state.regionRef' + $(if ($case.operation -eq 'setValue') { ",value:'MUST NOT BE WRITTEN'" } else { '' }) + '});'
            [void](Execute $case.name $source -ExpectStale)
            Require ($watch.ElapsedMilliseconds -lt 12000) 'Refusal exceeded the retention control budget; expiry could mask the geometry regression'
            $after = Read-Snapshot $f
            Require ($after.increments -eq $before.increments -and $after.text -ceq $before.text -and $after.saves -eq $before.saves) 'Old region reference changed the owned application'
            Require ((Peer-Fingerprint (Read-Snapshot $fixtures[1])) -ceq $beforePeer) 'Region test changed the peer instance'
        }
        Change-Layout $f 0 0 'region-final-reset'
        if ($IncludePointerTests) {
            $f = $fixtures[0]
            $beforePeer = Peer-Fingerprint (Read-Snapshot $fixtures[1])
            foreach ($item in @(@{ name = 'left'; button = 'left'; count = 1 }, @{ name = 'right'; button = 'right'; count = 1 }, @{ name = 'double'; button = 'left'; count = 2 })) {
                $before = Read-Snapshot $f
                $x = ($before.pad_point.x - $before.bounds.x) / [double]$before.bounds.width
                $y = ($before.pad_point.y - $before.bounds.y) / [double]$before.bounds.height
                $source = 'const window_ref=state.A;await app.focus({window_ref});const p=await app.capture({window_ref});return await app.click({window_ref,observation_ref:p.observation_ref,x:p.image.width*' + (Js $x) + ',y:p.image.height*' + (Js $y) + ',button:' + (Js $item.button) + ',count:' + $item.count + '});'
                [void](Execute ('pointer-' + $item.name) $source)
                [void](Wait-State $f ('pointer-state-' + $item.name) { param($s)
                    if ($item.name -eq 'right') { return $s.right_up -eq $before.right_up + 1 }
                    return ($s.left_up -eq $before.left_up + $item.count -and ($item.name -ne 'double' -or $s.double_clicks -eq $before.double_clicks + 1))
                })
            }
            $before = Read-Snapshot $f
            [void](Execute 'wheel' "return await app.scroll({window_ref:state.A,delta_y:3,delta_x:0});")
            [void](Wait-State $f 'wheel-state' { param($s) $s.wheel_delta -eq $before.wheel_delta - 360 })
            [void](Wait-Released $f 'pointer-release')
            Require ((Peer-Fingerprint (Read-Snapshot $fixtures[1])) -ceq $beforePeer) 'Pointer input changed the peer instance'
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
    [IO.File]::WriteAllText((Join-Path $output 'failure.txt'), (($_ | Out-String) + $_.ScriptStackTrace), $utf8)
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
