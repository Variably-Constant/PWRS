# Interleaved A/B of two built module folders through wall_clock.ps1.
# Each cell runs in its own host process, because both folders export
# the same command names and cannot be loaded side by side. The order
# of the two sides flips every round so a drifting machine costs both
# sides equally. Report the median of each case.
param(
    [Parameter(Mandatory)] [string] $A,
    [Parameter(Mandatory)] [string] $B,
    [string] $ALabel = 'A',
    [string] $BLabel = 'B',
    [int] $Iterations = 50000,
    [int] $Reps = 5,
    [int] $Warmup = 5000,
    # Keep this even: the two sides swap position every round, so an
    # odd count gives one side the first slot once more than the other.
    [int] $Rounds = 6,
    [string] $Pwsh = 'pwsh'
)

$ErrorActionPreference = 'Stop'
$harness = Join-Path $PSScriptRoot 'wall_clock.ps1'
if (-not (Test-Path $harness)) { throw "wall_clock.ps1 not found beside $PSCommandPath" }

function Invoke-Side([string] $Module) {
    $out = & $Pwsh -NoProfile -NonInteractive -File $harness -Module $Module -Iterations $Iterations -Reps $Reps -Warmup $Warmup -Csv
    if ($LASTEXITCODE -ne 0) { throw "harness failed for $Module with exit code $LASTEXITCODE" }
    $out | ConvertFrom-Csv
}

$samples = @{}
function Add-Sample([string] $Side, [string] $Case, [int] $Ms) {
    $key = "$Side|$Case"
    if (-not $samples.ContainsKey($key)) { $samples[$key] = [System.Collections.Generic.List[int]]::new() }
    $samples[$key].Add($Ms)
}

$cases = [System.Collections.Generic.List[string]]::new()
for ($r = 1; $r -le $Rounds; $r++) {
    $order = if ($r % 2 -eq 1) { @(@($ALabel, $A), @($BLabel, $B)) } else { @(@($BLabel, $B), @($ALabel, $A)) }
    foreach ($pair in $order) {
        Write-Host ("round {0} {1}" -f $r, $pair[0])
        foreach ($row in Invoke-Side $pair[1]) {
            if (-not $cases.Contains($row.Case)) { $cases.Add($row.Case) }
            Add-Sample $pair[0] $row.Case ([int] $row.Ms)
        }
    }
}

function Get-Median([System.Collections.Generic.List[int]] $Values) {
    $sorted = @($Values | Sort-Object)
    $n = $sorted.Count
    if ($n -eq 0) { return 0 }
    if ($n % 2 -eq 1) { return $sorted[[int](($n - 1) / 2)] }
    return [int](($sorted[$n / 2 - 1] + $sorted[$n / 2]) / 2)
}

$report = foreach ($case in $cases) {
    $a = Get-Median $samples["$ALabel|$case"]
    $b = Get-Median $samples["$BLabel|$case"]
    [pscustomobject]@{
        Case      = $case
        "$ALabel" = $a
        "$BLabel" = $b
        Change    = if ($a -gt 0) { "{0:P1}" -f (($b - $a) / [double] $a) } else { 'n/a' }
    }
}

"iterations={0} reps={1} rounds={2} median ms per case" -f $Iterations, $Reps, $Rounds
$report | Format-Table -AutoSize | Out-String
@'
The C# and ps fn rows are the same code on both sides. Their Change
is this run's noise floor; read every other row against it.
'@
