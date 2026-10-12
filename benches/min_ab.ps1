# Interleaved A/B of two built module folders, reduced by minimum.
#
# ab_modules.ps1 takes the median of each case, which reads a busy
# machine as a slower one. Competing load only ever adds time, so the
# minimum across rounds is the closest estimate of the cost on an idle
# core and survives a machine that is not quiet. The price is that a
# minimum needs more rounds than a median to settle.
#
# Both folders export the same command names, so each cell runs in its
# own host process, and the two sides swap position every round.
#
# The C# and advanced-function cases are the same code on both sides.
# Their spread is the run's noise floor, and this script computes it
# and refuses to call the run readable when it exceeds -FloorPercent.
param(
    [Parameter(Mandatory)] [string] $A,
    [Parameter(Mandatory)] [string] $B,
    [string] $ALabel = 'A',
    [string] $BLabel = 'B',
    [int] $Iterations = 50000,
    [int] $Reps = 3,
    [int] $Warmup = 5000,
    # Keep this even so neither side gets the first slot more often.
    [int] $Rounds = 10,
    # The largest movement tolerated on a case whose code is identical
    # on both sides before the run is called unreadable.
    [double] $FloorPercent = 2.0,
    [string] $Pwsh = 'pwsh'
)

$ErrorActionPreference = 'Stop'
$harness = Join-Path $PSScriptRoot 'wall_clock.ps1'
if (-not (Test-Path $harness)) { throw "wall_clock.ps1 not found beside $PSCommandPath" }

# A folder missing a file its manifest names shifts every cell,
# including the ones whose code did not change, so the shapes are
# compared before anything is timed.
function Get-Shape([string] $Root) {
    if (-not (Test-Path $Root)) { throw "module folder not found: $Root" }
    @(Get-ChildItem -Path $Root -Recurse -File |
        ForEach-Object { $_.FullName.Substring($Root.Length).TrimStart('\', '/') } |
        Sort-Object)
}
$shapeA = Get-Shape $A
$shapeB = Get-Shape $B
$onlyA = @($shapeA | Where-Object { $_ -notin $shapeB })
$onlyB = @($shapeB | Where-Object { $_ -notin $shapeA })
if ($onlyA.Count -or $onlyB.Count) {
    throw ("the two folders hold different files, so a delta would not be about the code: " +
        "only in ${ALabel}: $($onlyA -join ', '); only in ${BLabel}: $($onlyB -join ', ')")
}
Write-Host ("both folders hold the same {0} files" -f $shapeA.Count)

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
        Write-Host ("round {0}/{1} {2}" -f $r, $Rounds, $pair[0])
        foreach ($row in Invoke-Side $pair[1]) {
            if (-not $cases.Contains($row.Case)) { $cases.Add($row.Case) }
            Add-Sample $pair[0] $row.Case ([int] $row.Ms)
        }
    }
}

function Get-Min([System.Collections.Generic.List[int]] $Values) {
    if ($Values.Count -eq 0) { return 0 }
    ($Values | Measure-Object -Minimum).Minimum
}

# A case is part of the floor when the same code ran on both sides.
function Test-IsFloor([string] $Case) {
    $Case -match '^(C#|ps fn)' -or $Case -match '^CONTROL'
}

$rows = foreach ($case in $cases) {
    $av = $samples["$ALabel|$case"]
    $bv = $samples["$BLabel|$case"]
    $a = Get-Min $av
    $b = Get-Min $bv
    $change = if ($a -gt 0) { 100.0 * ($b - $a) / [double] $a } else { 0.0 }
    [pscustomobject]@{
        Case     = $case
        "$ALabel" = $a
        "$BLabel" = $b
        Change   = "{0:N1}%" -f $change
        Spread   = "{0:N0}%" -f $(if ($a -gt 0) { 100.0 * (($av | Measure-Object -Maximum).Maximum - $a) / [double] $a } else { 0 })
        Floor    = if (Test-IsFloor $case) { 'yes' } else { '' }
        Delta    = $change
    }
}

"iterations={0} reps={1} rounds={2}; minimum ms per case" -f $Iterations, $Reps, $Rounds
$rows | Select-Object Case, $ALabel, $BLabel, Change, Spread, Floor | Format-Table -AutoSize | Out-String

$floor = @($rows | Where-Object { $_.Floor -eq 'yes' } | ForEach-Object { [math]::Abs($_.Delta) })
$worst = if ($floor.Count) { ($floor | Measure-Object -Maximum).Maximum } else { 0 }
"noise floor: worst movement on a case whose code is identical on both sides is {0:N1}%" -f $worst
if ($worst -gt $FloorPercent) {
    "VERDICT: NOT READABLE. The floor exceeds {0:N1}%, so no row can be attributed to the change." -f $FloorPercent
    exit 2
}
"VERDICT: readable. Read every non-floor row against the {0:N1}% floor above." -f $worst
