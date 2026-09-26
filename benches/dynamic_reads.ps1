# What one property read costs when Rust takes it against when
# PowerShell takes it, over the same objects in the same process.
#
# A module author deciding where per-item work belongs needs the cost
# of a single read on each side, not the cost of a call. The Rust arm
# therefore does every read inside one cmdlet invocation: the
# invocation is paid once and divides away, leaving the read.
#
# Three timed arms per object kind, plus the loop they all run in:
#   - Rust: one dynamic read per object, the name resolved at run time
#   - literal: $o.Name, the fastest read PowerShell has, with the name
#     compiled into the member access
#   - by name: $o.psobject.Properties[$n].Value, the name resolved at
#     run time, which is what the Rust arm also does and so is the
#     like-for-like comparison
#   - loop only: the empty foreach, which every PowerShell arm pays and
#     the Rust arm does not. Subtract it before reading a difference.
#
# Two object kinds, because what answers a read differs: a
# PSCustomObject answers from its own property bag, a CLR object
# answers through the adapter wrapping it.
#
# A second set of arms asks each object its TYPE rather than reading a
# property, since that is the other thing code dispatching on unknown
# input does per item:
#   - tag from Rust: type_tag, one crossing answering a u32
#   - name from Rust: type_name, GetType then FullName then a string
#     read, so three crossings and a marshalled string
#   - $o.GetType().FullName: the same question asked in script
# The loop-only floor is shared with the property arms.
#
# Reduced by minimum across rounds, since competing work only ever adds
# time, and the arm order rotates every round so none keeps the coldest
# slot.
param(
    [Parameter(Mandatory)] [string] $Module,
    [int] $Count = 500,
    [int] $Passes = 200,
    [int] $Rounds = 7
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $Module 'Hello.psd1') -Force -ErrorAction Stop

$reads = [double]$Count * $Passes
$custom = @(1..$Count | ForEach-Object { [pscustomobject]@{ Name = "item$_" } })
$clr = @(1..$Count | ForEach-Object { [System.Version]::new(1, 2, 3, $_) })

# $sink is assigned and never read on purpose: each arm reads a
# property and discards it, which is the cost being measured.
$cases = [ordered]@{
    'pscustomobject, Rust'      = { Measure-RustPropertyReads -InputObject $custom -Name Name -Passes $Passes }
    'pscustomobject, $o.Name'   = { foreach ($p in 1..$Passes) { foreach ($o in $custom) { $sink = $o.Name } } }
    'pscustomobject, by name'   = { foreach ($p in 1..$Passes) { foreach ($o in $custom) { $sink = $o.psobject.Properties['Name'].Value } } }
    'pscustomobject, loop only' = { foreach ($p in 1..$Passes) { foreach ($o in $custom) { } } }
    'Version, Rust'             = { Measure-RustPropertyReads -InputObject $clr -Name Major -Passes $Passes }
    'Version, $o.Major'         = { foreach ($p in 1..$Passes) { foreach ($o in $clr) { $sink = $o.Major } } }
    'Version, by name'          = { foreach ($p in 1..$Passes) { foreach ($o in $clr) { $sink = $o.psobject.Properties['Major'].Value } } }
    'Version, loop only'        = { foreach ($p in 1..$Passes) { foreach ($o in $clr) { } } }
    'pscustomobject, tag from Rust'  = { Measure-RustTypeReads -InputObject $custom -Passes $Passes }
    'pscustomobject, name from Rust' = { Measure-RustTypeReads -InputObject $custom -Passes $Passes -ByName }
    'pscustomobject, GetType name'   = { foreach ($p in 1..$Passes) { foreach ($o in $custom) { $sink = $o.GetType().FullName } } }
    'Version, tag from Rust'         = { Measure-RustTypeReads -InputObject $clr -Passes $Passes }
    'Version, name from Rust'        = { Measure-RustTypeReads -InputObject $clr -Passes $Passes -ByName }
    'Version, GetType name'          = { foreach ($p in 1..$Passes) { foreach ($o in $clr) { $sink = $o.GetType().FullName } } }
}

$names = @($cases.Keys)
$best = @{}
foreach ($n in $names) {
    & $cases[$n] | Out-Null
    $best[$n] = [double]::MaxValue
}

foreach ($round in 1..$Rounds) {
    foreach ($i in 0..($names.Count - 1)) {
        $n = $names[($i + $round) % $names.Count]
        [System.GC]::Collect()
        [System.GC]::WaitForPendingFinalizers()
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        & $cases[$n] | Out-Null
        $sw.Stop()
        $ms = $sw.Elapsed.TotalMilliseconds
        if ($ms -lt $best[$n]) { $best[$n] = $ms }
    }
}

"{0}, {1} objects x {2} passes = {3:N0} reads per arm, {4} rounds" -f `
    $PSVersionTable.PSVersion, $Count, $Passes, $reads, $Rounds
''
"{0,-28} {1,10} {2,12} {3,14}" -f 'arm', 'ms', 'ns/read', 'less loop'
"{0,-28} {1,10} {2,12} {3,14}" -f ('-' * 28), ('-' * 10), ('-' * 12), ('-' * 14)
foreach ($n in $names) {
    $floor = if ($n -like 'pscustomobject*') { $best['pscustomobject, loop only'] } else { $best['Version, loop only'] }
    $net = if ($n -like '*Rust' -or $n -like '*loop only') { $best[$n] } else { $best[$n] - $floor }
    "{0,-28} {1,10:N2} {2,12:N1} {3,14:N1}" -f $n, $best[$n], ($best[$n] * 1e6 / $reads), ($net * 1e6 / $reads)
}
''
foreach ($kind in 'pscustomobject', 'Version') {
    $rust = $best["$kind, Rust"]
    $literal = if ($kind -eq 'Version') { '$o.Major' } else { '$o.Name' }
    foreach ($arm in 'by name', $literal) {
        $net = $best["$kind, $arm"] - $best["$kind, loop only"]
        "{0}: Rust is {1:N2}x the cost of '{2}' with the loop taken off" -f $kind, ($rust / $net), $arm
    }
}
''
foreach ($kind in 'pscustomobject', 'Version') {
    $tag = $best["$kind, tag from Rust"]
    $name = $best["$kind, name from Rust"]
    $script = $best["$kind, GetType name"] - $best["$kind, loop only"]
    "{0}: the tag is {1:N2}x the cost of the name from Rust, {2:N2}x the cost of GetType().FullName in script" -f `
        $kind, ($tag / $name), ($tag / $script)
}
