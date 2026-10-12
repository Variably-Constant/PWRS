# Runs a Pester suite against a built module in whichever host runs
# this script. Pester 4 or later; PesterPath points at a saved copy
# when the host's own module path has none it can load.
param(
    [Parameter(Mandatory)] [string] $ModulePath,
    [Parameter(Mandatory)] [string] $TestsPath,
    [string] $PesterPath = ''
)

$ErrorActionPreference = 'Stop'
$env:PWRS_MODULE = $ModulePath

# A Pester that will not load is reported as that, naming what was
# tried and the variable that overrides it, on one line written
# straight to stderr so no formatter wraps the path in it.
$saved = $PesterPath -and (Test-Path $PesterPath)
try {
    if ($saved) {
        Import-Module $PesterPath -ErrorAction Stop
    } else {
        Import-Module Pester -MinimumVersion 4.0 -ErrorAction Stop
    }
} catch {
    if ($saved) {
        $tried = $PesterPath
    } else {
        $found = @(Get-Module -ListAvailable Pester | Where-Object { $_.Version -ge [version]'4.0' } | ForEach-Object { "Pester $($_.Version) at $($_.ModuleBase)" })
        $tried = if ($found.Count -eq 0) { 'no Pester 4 or later on this host''s module path' } else { $found -join '; ' }
    }
    $why = ($_.Exception.Message -replace '\s+', ' ').Trim().TrimEnd('.')
    [Console]::Error.WriteLine(("pwrs: Pester would not load in {0} {1}. Tried: {2}. It said: {3}. Set PWRS_PESTER_PATH to a Pester module that loads in this host, such as one saved with Save-Module -Name Pester." -f $PSVersionTable.PSEdition, $PSVersionTable.PSVersion, $tried, $why))
    exit 2
}

$result = Invoke-Pester -Path $TestsPath -PassThru
if ($null -eq $result) { throw 'Invoke-Pester returned nothing' }
Write-Output ("pwrs pester host={0} pester={1} passed={2} failed={3}" -f $PSVersionTable.PSVersion, (Get-Module Pester).Version, $result.PassedCount, $result.FailedCount)
exit [int] $result.FailedCount
