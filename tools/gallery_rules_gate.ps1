# Checks the script and manifest cargo-pwrs wrote for each example
# against the rules the PowerShell Gallery runs on every package it
# takes: PSScriptAnalyzer with its PSGallery settings, one file at a
# time. It builds nothing, so it runs after the examples are built, and
# it fails on any finding.
#
# The suppressed findings are listed as well, so the output shows the
# rules ran and what an attribute in a script answers for.
param(
    # The folder the examples' module folders were built into.
    [string] $Modules = [System.IO.Path]::Combine($PSScriptRoot, '..', 'target', 'pwrs'),
    # A PSScriptAnalyzer.psd1 to import. Empty imports the one on the
    # module path, where `Install-PSResource PSScriptAnalyzer` puts it.
    [string] $Analyzer = ''
)
$ErrorActionPreference = 'Stop'
if ($Analyzer) { Import-Module $Analyzer } else { Import-Module PSScriptAnalyzer }
'PSScriptAnalyzer ' + (Get-Module PSScriptAnalyzer).Version
$findings = @()
foreach ($m in 'Hello', 'Calc', 'MemFs', 'Tls') {
    foreach ($file in "$m.psm1", "$m.psd1") {
        $path = [System.IO.Path]::Combine($Modules, $m, $file)
        $one = @(Invoke-ScriptAnalyzer -Path $path -Settings PSGallery)
        "$m/$file findings $($one.Count)"
        $findings += $one
        Invoke-ScriptAnalyzer -Path $path -Settings PSGallery -SuppressedOnly | ForEach-Object { "$m/$file suppressed $($_.RuleName) at line $($_.Line)" }
    }
}
$findings | ForEach-Object { "$($_.Severity) $($_.RuleName) $($_.ScriptName):$($_.Line) $($_.Message)" }
if ($findings.Count -ne 0) { throw "$($findings.Count) findings under the Gallery's rules" }
