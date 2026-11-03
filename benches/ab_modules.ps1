# Interleaved A/B of built module folders through wall_clock.ps1: two,
# or three with -C, which is how a null control (the same build at a
# second path) runs in the same rounds as the change it controls for.
# Each cell runs in its own host process, because the folders export the
# same command names and cannot be loaded side by side. The order of the
# sides rotates every round so a drifting machine costs each side
# equally. Report the median of each case, and, for each side after the
# first, the median, minimum and maximum of that side over A taken within
# each round, which a machine that drifts between rounds moves less than
# it moves the medians.
param(
    [Parameter(Mandatory)] [string] $A,
    [Parameter(Mandatory)] [string] $B,
    [string] $C,
    [string] $ALabel = 'A',
    [string] $BLabel = 'B',
    [string] $CLabel = 'C',
    [int] $Iterations = 50000,
    [int] $Reps = 5,
    [int] $Warmup = 5000,
    # A multiple of the number of sides: every round moves each side one
    # place on, so any other count gives some side a place once more.
    [int] $Rounds = 6,
    [string] $Pwsh = 'pwsh',
    # Passed to wall_clock.ps1: only the cases matching these patterns.
    [string[]] $Case
)

$ErrorActionPreference = 'Stop'
$harness = Join-Path $PSScriptRoot 'wall_clock.ps1'
if (-not (Test-Path $harness)) { throw "wall_clock.ps1 not found beside $PSCommandPath" }

$sides = @(@($ALabel, $A), @($BLabel, $B))
if ($C) { $sides += , @($CLabel, $C) }

function Invoke-Side([string] $Module) {
    $only = if ($Case) { @('-Case', ($Case -join ',')) } else { @() }
    $out = & $Pwsh -NoProfile -NonInteractive -File $harness -Module $Module -Iterations $Iterations -Reps $Reps -Warmup $Warmup -Csv @only
    if ($LASTEXITCODE -ne 0) { throw "harness failed for $Module with exit code $LASTEXITCODE" }
    $out | ConvertFrom-Csv
}

$samples = @{}
# Each side's milliseconds per case in each round, for the ratios.
$byRound = @{}
function Add-Sample([string] $Side, [string] $Case, [int] $Ms, [int] $Round) {
    $key = "$Side|$Case"
    if (-not $samples.ContainsKey($key)) { $samples[$key] = [System.Collections.Generic.List[int]]::new() }
    $samples[$key].Add($Ms)
    $byRound["$key|$Round"] = $Ms
}

$cases = [System.Collections.Generic.List[string]]::new()
for ($r = 1; $r -le $Rounds; $r++) {
    # A list, since a pipeline would unroll each side's two-element array.
    $order = [System.Collections.Generic.List[object]]::new()
    for ($i = 0; $i -lt $sides.Count; $i++) { $order.Add($sides[($i + $r - 1) % $sides.Count]) }
    foreach ($side in $order) {
        Write-Host ("round {0} {1}" -f $r, $side[0])
        foreach ($row in Invoke-Side $side[1]) {
            if (-not $cases.Contains($row.Case)) { $cases.Add($row.Case) }
            Add-Sample $side[0] $row.Case ([int] $row.Ms) $r
        }
    }
}

function Get-Median($Values) {
    $sorted = @($Values | Sort-Object)
    $n = $sorted.Count
    if ($n -eq 0) { return $null }
    if ($n % 2 -eq 1) { return $sorted[[int](($n - 1) / 2)] }
    return ($sorted[$n / 2 - 1] + $sorted[$n / 2]) / 2
}

$report = foreach ($case in $cases) {
    $a = Get-Median $samples["$ALabel|$case"]
    $row = [ordered] @{ Case = $case; "$ALabel" = $a }
    foreach ($side in $sides | Select-Object -Skip 1) {
        $label = $side[0]
        $other = Get-Median $samples["$label|$case"]
        $ratios = @(for ($r = 1; $r -le $Rounds; $r++) {
            $ra = $byRound["$ALabel|$case|$r"]
            $ro = $byRound["$label|$case|$r"]
            if ($ra -gt 0 -and $null -ne $ro) { $ro / [double] $ra }
        })
        $ratios = @($ratios | Sort-Object)
        $mid = Get-Median $ratios
        $row[$label] = $other
        $row["$label change"] = if ($a -gt 0) { "{0:P1}" -f (($other - $a) / [double] $a) } else { 'n/a' }
        $row["$label ratio"] = if ($null -ne $mid) { '{0:N3} ({1:N3}-{2:N3})' -f $mid, $ratios[0], $ratios[-1] } else { 'n/a' }
    }
    [pscustomobject] $row
}

$others = ($sides | Select-Object -Skip 1 | ForEach-Object { $_[0] }) -join ' and '
"iterations={0} reps={1} rounds={2} median ms per case; each ratio is the median, then the least and greatest, of {3} over {4} within a round" -f $Iterations, $Reps, $Rounds, $others, $ALabel
$report | Format-Table -AutoSize | Out-String
@'
The C# and ps fn rows are the same code on every side, and a null
control is the same build on two; their change and ratio are this run's
noise floor, against which every other row is read.
'@
