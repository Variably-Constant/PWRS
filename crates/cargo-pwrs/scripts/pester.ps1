# Runs a Pester suite against a built module in whichever host runs
# this script, under the Pester module PesterPath names: the Pester
# cargo pwrs fetched into its toolchain, or the one PWRS_PESTER_PATH names.
param(
    [Parameter(Mandatory)] [string] $ModulePath,
    [Parameter(Mandatory)] [string] $TestsPath,
    [Parameter(Mandatory)] [string] $PesterPath
)

$ErrorActionPreference = 'Stop'
$env:PWRS_MODULE = $ModulePath

# A Pester that will not load is reported as that, naming its path and
# the variable that overrides it, on one line written straight to stderr
# so no formatter wraps the path in it.
try {
    Import-Module $PesterPath -ErrorAction Stop
} catch {
    $why = ($_.Exception.Message -replace '\s+', ' ').Trim().TrimEnd('.')
    [Console]::Error.WriteLine(("pwrs: Pester would not load in {0} {1} from {2}. It said: {3}. Set PWRS_PESTER_PATH to a Pester module that loads in this host to run the suites under that one." -f $PSVersionTable.PSEdition, $PSVersionTable.PSVersion, $PesterPath, $why))
    exit 2
}

$result = Invoke-Pester -Path $TestsPath -PassThru
if ($null -eq $result) { throw 'Invoke-Pester returned nothing' }
Write-Output ("pwrs pester host={0} pester={1} passed={2} failed={3}" -f $PSVersionTable.PSVersion, (Get-Module Pester).Version, $result.PassedCount, $result.FailedCount)
exit [int] $result.FailedCount
