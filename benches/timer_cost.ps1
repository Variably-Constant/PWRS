# What one Stopwatch.GetTimestamp() and one Interlocked.Add cost on
# this host.
#
# The managed phase counter takes a timestamp to open the native
# window, another to close it, and an interlocked add to record it, all
# inside the window `run_ns_avg` reports. That boundary is the
# difference between `run_ns_avg` minus `native_ns_avg` and the managed
# packing it is read as, so docs/PERF.md quotes these numbers and this
# is what produced them.
#
# Reduced by minimum across rounds: competing work only ever adds time,
# so the minimum is the closest estimate of the cost on an idle core.
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Threading;

public static class TimerCost
{
    private static long _sink;

    public static double ReadNs(int n)
    {
        for (int warm = 0; warm < 200000; warm++) _sink += Stopwatch.GetTimestamp();
        long t0 = Stopwatch.GetTimestamp();
        for (int i = 0; i < n; i++) _sink += Stopwatch.GetTimestamp();
        long t1 = Stopwatch.GetTimestamp();
        return (t1 - t0) * (1e9 / Stopwatch.Frequency) / n;
    }

    public static double AddNs(int n)
    {
        for (int warm = 0; warm < 200000; warm++) Interlocked.Add(ref _sink, 1);
        long t0 = Stopwatch.GetTimestamp();
        for (int i = 0; i < n; i++) Interlocked.Add(ref _sink, 1);
        long t1 = Stopwatch.GetTimestamp();
        return (t1 - t0) * (1e9 / Stopwatch.Frequency) / n;
    }

    public static long Frequency => Stopwatch.Frequency;
}
'@

$n = 3000000
$reads = @(1..3 | ForEach-Object { [TimerCost]::ReadNs($n) })
$adds = @(1..3 | ForEach-Object { [TimerCost]::AddNs($n) })
$read = ($reads | Measure-Object -Minimum).Minimum
$add = ($adds | Measure-Object -Minimum).Minimum

"Stopwatch.Frequency = {0:N0} Hz" -f [TimerCost]::Frequency
"GetTimestamp    min {0,6:N2} ns   rounds: {1}" -f $read, (($reads | ForEach-Object { '{0:N2}' -f $_ }) -join ', ')
"Interlocked.Add min {0,6:N2} ns   rounds: {1}" -f $add, (($adds | ForEach-Object { '{0:N2}' -f $_ }) -join ', ')
"counter boundary inside run_ns_avg: {0:N1} ns (two reads and one add)" -f (2 * $read + $add)
