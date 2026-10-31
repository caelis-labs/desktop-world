#requires -Version 5.1
param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
if ($PSVersionTable.PSEdition -ne 'Desktop') { throw 'Use Windows PowerShell 5.1' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a new output directory' }
[void][IO.Directory]::CreateDirectory($OutputDirectory)
$utf8=New-Object Text.UTF8Encoding($false)
$references=@('System.dll','System.Windows.Forms.dll','System.Drawing.dll','System.Web.Extensions.dll')
foreach ($source in @('Fixture','CursorObserver')) {
    Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $PSScriptRoot ($source+'.cs')),$utf8)) -Language CSharp `
        -ReferencedAssemblies $references -OutputAssembly (Join-Path $OutputDirectory ($source+'.exe')) -OutputType WindowsApplication
}
