# What a large array costs a cmdlet to take, in each form the array can
# arrive in and through each way a PWRS parameter can receive it,
# against the hello example.
# The forms: a bare byte[]; the same array wrapped in a PSObject, which
# is how a byte[] written by any cmdlet reaches the next one; and an
# object[] holding the same values boxed, whose element type differs
# from the parameter's. The ways: Get-RustByteSum, a Vec<u8> parameter
# the shell declares byte[]; Get-RustRawByteSum, the same parameter
# marked raw, declared object; Measure-RustInput, a PsObject parameter
# declared byte[] with clr and read through a pin; Get-RustChecksum, a
# PsObject parameter declared object and read through a pin; and
# Get-RustByteRange, a Vec<u8> written out as one byte[]. Each arm is
# one call, timed on
# its own, and every arm's answer is checked against the raw arm's
# before anything is timed. The arm order rotates every round, a
# collection runs before every pass, and the busy cores of every other
# process are read across each pass from their processor time and
# printed beside the figures, since the box is not assumed quiet.
# CONTROL is the raw arm again under a second name.
param(
    [Parameter(Mandatory)] [string] $Module,
    [int[]] $Megabytes = @(4, 32),
    [int] $Rounds = 7
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $Module 'Hello.psd1') -Force -ErrorAction Stop

# C# 5, so Windows PowerShell 5.1's compiler takes it.
$source = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;

namespace PwrsBulkBench
{
    public sealed class LoadReading
    {
        public Dictionary<int, double> Cpu = new Dictionary<int, double>();
        public Dictionary<int, string> Names = new Dictionary<int, string>();
        public int Unreadable;
        public long Ticks;
    }

    public static class Load
    {
        // Processor time of every other process. A process whose time
        // cannot be read is counted in Unreadable and left out of the sum.
        // Id 0 is Windows' idle process, whose time is the opposite of busy.
        public static LoadReading Read()
        {
            LoadReading r = new LoadReading();
            int self = Process.GetCurrentProcess().Id;
            foreach (Process p in Process.GetProcesses())
            {
                using (p)
                {
                    if (p.Id == self || p.Id == 0) continue;
                    try
                    {
                        r.Cpu[p.Id] = p.TotalProcessorTime.TotalMilliseconds;
                        r.Names[p.Id] = p.ProcessName;
                    }
                    catch (System.ComponentModel.Win32Exception) { r.Unreadable++; }
                    catch (InvalidOperationException) { r.Unreadable++; }
                    catch (NotSupportedException) { r.Unreadable++; }
                }
            }
            r.Ticks = Stopwatch.GetTimestamp();
            return r;
        }

        private static double Used(LoadReading before, int id, double now)
        {
            double was;
            return before.Cpu.TryGetValue(id, out was) ? Math.Max(0, now - was) : now;
        }

        public static double WindowMs(LoadReading before, LoadReading after)
        {
            return (after.Ticks - before.Ticks) * 1000.0 / Stopwatch.Frequency;
        }

        // Processor milliseconds other processes used between two
        // readings, counting a process that started inside the window at
        // all of its time.
        public static double UsedMs(LoadReading before, LoadReading after)
        {
            double used = 0;
            foreach (KeyValuePair<int, double> e in after.Cpu) used += Used(before, e.Key, e.Value);
            return used;
        }

        // Cores busy in other processes between two readings.
        public static double BusyCores(LoadReading before, LoadReading after)
        {
            double ms = WindowMs(before, after);
            return ms > 0 ? UsedMs(before, after) / ms : 0;
        }

        // The busiest other processes between two readings, as
        // "name cores" pairs.
        public static string[] Busiest(LoadReading before, LoadReading after, int count)
        {
            double ms = WindowMs(before, after);
            List<KeyValuePair<string, double>> rows = new List<KeyValuePair<string, double>>();
            foreach (KeyValuePair<int, double> e in after.Cpu)
            {
                rows.Add(new KeyValuePair<string, double>(after.Names[e.Key], ms > 0 ? Used(before, e.Key, e.Value) / ms : 0));
            }
            rows.Sort(delegate (KeyValuePair<string, double> a, KeyValuePair<string, double> b) { return b.Value.CompareTo(a.Value); });
            List<string> top = new List<string>();
            for (int i = 0; i < rows.Count && i < count; i++) top.Add(rows[i].Key + " " + rows[i].Value.ToString("0.00"));
            return top.ToArray();
        }
    }
}
'@
$null = Add-Type -TypeDefinition $source -PassThru

function Get-Percentile([double[]] $Sorted, [double] $P) {
    $x = $P * ($Sorted.Count - 1)
    $lo = [int][math]::Floor($x)
    $hi = [int][math]::Ceiling($x)
    $Sorted[$lo] + ($Sorted[$hi] - $Sorted[$lo]) * ($x - $lo)
}

function Get-Summary([System.Collections.Generic.List[double]] $Values) {
    $sorted = [double[]] @($Values | Sort-Object)
    [pscustomobject]@{
        Median = Get-Percentile $sorted 0.5
        Q1     = Get-Percentile $sorted 0.25
        Q3     = Get-Percentile $sorted 0.75
        Min    = $sorted[0]
        Max    = $sorted[-1]
    }
}

# The comma keeps PowerShell from unrolling the list into its elements,
# or into nothing when it is empty.
function New-Series { , [System.Collections.Generic.List[double]]::new() }

# The sum an arm's answer carries: Get-RustByteSum and Get-RustRawByteSum
# write it, Measure-RustInput writes 'bytes <n> sum <s>', and an array
# written out is summed here.
function Get-Sum($Answer) {
    if ($Answer -is [string]) { return [int64]($Answer -replace '^bytes \d+ sum ', '') }
    if ($Answer -is [byte[]]) { $s = [int64] 0; foreach ($b in $Answer) { $s += $b }; return $s }
    [int64] $Answer
}

"bulk_array.ps1 host={0} {1} megabytes={2} rounds={3}" -f `
    $PSVersionTable.PSVersion, [Environment]::OSVersion.VersionString, ($Megabytes -join ','), $Rounds

$start = [PwrsBulkBench.Load]::Read()
Start-Sleep -Milliseconds 1000
$idle = [PwrsBulkBench.Load]::Read()
"busy cores in other processes in the second before the run: {0:N2}" -f [PwrsBulkBench.Load]::BusyCores($start, $idle)

foreach ($mb in $Megabytes) {
    $n = $mb * 1MB
    $bare = [byte[]]::new($n)
    (New-Object System.Random 1).NextBytes($bare)
    $wrapped = Write-Output -NoEnumerate $bare
    if (-not ($wrapped -is [psobject])) { throw 'Write-Output -NoEnumerate did not wrap the array' }
    if (-not [object]::ReferenceEquals($wrapped.psobject.BaseObject, $bare)) { throw 'the wrapper does not hold the same array' }

    $arms = [ordered]@{
        'typed byte[] parameter, bare byte[]'         = { Get-RustByteSum -Bytes $bare }
        'typed byte[] parameter, wrapped byte[]'      = { Get-RustByteSum -Bytes $wrapped }
        'raw parameter, bare byte[]'                  = { Get-RustRawByteSum -Bytes $bare }
        'raw parameter, wrapped byte[]'               = { Get-RustRawByteSum -Bytes $wrapped }
        'declared byte[], pinned, bare byte[]'        = { Measure-RustInput -InputObject $bare }
        'declared byte[], pinned, wrapped byte[]'     = { Measure-RustInput -InputObject $wrapped }
        'PsObject parameter, pinned, bare byte[]'     = { Get-RustChecksum -Bytes $bare }
        'PsObject parameter, pinned, wrapped byte[]'  = { Get-RustChecksum -Bytes $wrapped }
        'written out from a Vec<u8>'                  = { Get-RustByteRange -Count $n }
        'CONTROL, raw parameter, bare byte[] again'   = { Get-RustRawByteSum -Bytes $bare }
    }
    # An object[] of every value boxed is 24 bytes and more per element on
    # the managed heap, so the arms taking one run at the smallest size only.
    $boxed = $null
    if ($mb -eq $Megabytes[0]) {
        $boxed = [object[]] $bare
        if ($boxed.GetType() -ne [object[]]) { throw 'the boxed array is not an object[]' }
        $arms['typed byte[] parameter, object[]'] = { Get-RustByteSum -Bytes $boxed }
        $arms['declared byte[], pinned, object[]'] = { Measure-RustInput -InputObject $boxed }
    }

    # Each arm must answer what the raw arm answers before any of it is timed.
    $expected = Get-Sum (Get-RustRawByteSum -Bytes $bare)
    if ($expected -le 0) { throw "the raw arm summed $expected" }
    foreach ($name in $arms.Keys) {
        $answer = & $arms[$name]
        if ($name -like 'written out*') {
            if (-not ($answer -is [byte[]]) -or $answer.Length -ne $n) { throw "$name wrote $($answer.GetType().Name) of $($answer.Length)" }
        } elseif ((Get-Sum $answer) -ne $expected) {
            throw "$name answered $(Get-Sum $answer), not $expected"
        }
    }

    $ms = @{}; $busy = @{}
    $names = @($arms.Keys)
    foreach ($name in $names) { $ms[$name] = New-Series; $busy[$name] = New-Series }
    $maxBusy = 0.0; $maxPass = ''; $maxTop = @()
    for ($round = 1; $round -le $Rounds; $round++) {
        foreach ($i in 0..($names.Count - 1)) {
            $name = $names[($i + $round) % $names.Count]
            [System.GC]::Collect()
            [System.GC]::WaitForPendingFinalizers()
            [System.GC]::Collect()
            $before = [PwrsBulkBench.Load]::Read()
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            $null = & $arms[$name]
            $sw.Stop()
            $after = [PwrsBulkBench.Load]::Read()
            $ms[$name].Add($sw.Elapsed.TotalMilliseconds)
            $cores = [PwrsBulkBench.Load]::BusyCores($before, $after)
            $busy[$name].Add($cores)
            if ($cores -gt $maxBusy) {
                $maxBusy = $cores
                $maxPass = "$name round $round"
                $maxTop = [PwrsBulkBench.Load]::Busiest($before, $after, 3)
            }
        }
    }

    ''
    "{0} MB, ms per call: median [q1, q3] and min over rounds; busy cores in other processes across each pass: median, max" -f $mb
    foreach ($name in $names) {
        $s = Get-Summary $ms[$name]
        $b = Get-Summary $busy[$name]
        '{0,-44} {1,10:N2} [{2,9:N2}, {3,9:N2}] min {4,9:N2}   busy {5:N2}, {6:N2}' -f $name, $s.Median, $s.Q1, $s.Q3, $s.Min, $b.Median, $b.Max
    }
    "busiest pass: {0} at {1:N2} cores: {2}" -f $maxPass, $maxBusy, ($maxTop -join ', ')
    $boxed = $null
    $wrapped = $null
    $bare = $null
    [System.GC]::Collect()
}
