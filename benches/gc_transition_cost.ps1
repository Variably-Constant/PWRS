# What the managed-to-native GC transition costs per call on this host.
#
# PWRS makes one native call per phase and keeps the transition, so
# this is what that choice costs. `[SuppressGCTransition]` would remove
# it, and the runtime requires a suppressed call to run for under a
# microsecond, make no callback into the runtime, never block, never
# throw and touch no concurrency primitive. `pwrs_cmdlet_invoke`
# encloses the cmdlet's whole body, so it meets none of those.
#
# The same trivial import twice, once each way. GetCurrentProcessId is
# the fair subject: it reads a field, so the call itself is near-free
# and the difference between the two is the transition.
#
# Reduced by minimum across rounds: competing work only ever adds time.
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;

public static class GcTransition
{
    [DllImport("kernel32", EntryPoint = "GetCurrentProcessId")]
    private static extern uint Plain();

    [DllImport("kernel32", EntryPoint = "GetCurrentProcessId")]
    [SuppressGCTransition]
    private static extern uint Suppressed();

    private static uint _sink;

    public static double PlainNs(int n)
    {
        for (int w = 0; w < 200000; w++) _sink += Plain();
        long t0 = Stopwatch.GetTimestamp();
        for (int i = 0; i < n; i++) _sink += Plain();
        long t1 = Stopwatch.GetTimestamp();
        return (t1 - t0) * (1e9 / Stopwatch.Frequency) / n;
    }

    public static double SuppressedNs(int n)
    {
        for (int w = 0; w < 200000; w++) _sink += Suppressed();
        long t0 = Stopwatch.GetTimestamp();
        for (int i = 0; i < n; i++) _sink += Suppressed();
        long t1 = Stopwatch.GetTimestamp();
        return (t1 - t0) * (1e9 / Stopwatch.Frequency) / n;
    }
}
'@

$n = 3000000
$plain = @(1..3 | ForEach-Object { [GcTransition]::PlainNs($n) })
$supp = @(1..3 | ForEach-Object { [GcTransition]::SuppressedNs($n) })
$p = ($plain | Measure-Object -Minimum).Minimum
$s = ($supp | Measure-Object -Minimum).Minimum

"with the transition   min {0,6:N2} ns   rounds: {1}" -f $p, (($plain | ForEach-Object { '{0:N2}' -f $_ }) -join ', ')
"transition suppressed min {0,6:N2} ns   rounds: {1}" -f $s, (($supp | ForEach-Object { '{0:N2}' -f $_ }) -join ', ')
"the transition itself: {0:N2} ns per call" -f ($p - $s)
