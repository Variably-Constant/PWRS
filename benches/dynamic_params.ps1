# What giving a cmdlet dynamic parameters costs each call, and which
# part of that is PWRS's rather than PowerShell's own.
#
# Every end-to-end arm is -Calls command-line invocations of one command
# inside one foreach, and each is read against the arm that differs from
# it in one thing:
#   C# plain      a compiled PSCmdlet with Get-RustReading's parameter
#                 and output, and no dynamic parameters
#   C# null       C# plain implementing IDynamicParameters and returning
#                 $null
#   C# empty      the same, returning an empty
#                 RuntimeDefinedParameterDictionary, which is what a PWRS
#                 hook that adds nothing hands the engine
#   C# snapshot   C# null that also builds the bound-parameter table the
#                 generated GetDynamicParameters builds and pins it as the
#                 call into the library does; it returns $null as a PWRS
#                 hook that adds nothing does
#   C# two tables C# snapshot with the table copied a second time, the
#                 values taken out of their PSObject wrappers on the copy
#                 rather than on the build, and the copy pinned: what a
#                 second copy costs
#   C# one        C# plain returning -Unit with its validate set once
#                 -Kind is temperature, as Get-RustReading's hook does
#   pwrs static   Get-RustStaticReading: Get-RustReading with no hook
#   pwrs blind    Get-RustBlindReading: Get-RustReading with a hook that
#                 adds nothing and never reads what is bound
#   pwrs dynamic  Get-RustReading -Kind electricity: the hook reads -Kind
#                 and adds nothing
#   pwrs one      Get-RustReading -Kind temperature: the hook adds -Unit
#   CONTROL       pwrs dynamic again under a second name; its distance
#                 from pwrs dynamic is the run's floor
# Both kinds are eleven characters, so every arm writes a string of one
# length.
#
# The direct arms call GetDynamicParameters on one cmdlet instance from
# a C# loop, with no binder around it: what the generated method costs
# by itself, what the snapshot costs by itself, and the C# returns they
# are read against. A direct figure and the end-to-end difference it
# corresponds to are printed together with their residual.
#
# The arm order rotates every round, a collection runs before every
# pass, and every difference is taken within a round, then summarised
# across rounds by median and quartiles. The busy cores of every other
# process are read across each pass from their processor time and
# printed beside the figures, since the box is not assumed quiet.
param(
    [Parameter(Mandatory)] [string] $Module,
    [int] $Calls = 20000,
    [int] $DirectCalls = 200000,
    [int] $Rounds = 15,
    [int] $Warmup = 2000
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $Module 'Hello.psd1') -Force -ErrorAction Stop

# C# 5, so Windows PowerShell 5.1's compiler takes it.
$source = @'
using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Management.Automation;
using System.Runtime.InteropServices;

namespace PwrsDynBench
{
    public abstract class ReadingBase : PSCmdlet
    {
        private string _kind;
        protected ulong Bound;

        [Parameter(Mandatory = true, Position = 0)]
        public string Kind { get { return _kind; } set { _kind = value; Bound |= 1UL; } }

        protected override void ProcessRecord()
        {
            object unit;
            WriteObject(Kind + ":" + (MyInvocation.BoundParameters.TryGetValue("Unit", out unit) ? unit : "none"));
        }
    }

    [Cmdlet(VerbsCommon.Get, "CsPlainReading")]
    public sealed class GetCsPlainReading : ReadingBase { }

    [Cmdlet(VerbsCommon.Get, "CsNullReading")]
    public sealed class GetCsNullReading : ReadingBase, IDynamicParameters
    {
        public object GetDynamicParameters() { return null; }
    }

    [Cmdlet(VerbsCommon.Get, "CsEmptyReading")]
    public sealed class GetCsEmptyReading : ReadingBase, IDynamicParameters
    {
        public object GetDynamicParameters() { return new RuntimeDefinedParameterDictionary(); }
    }

    [Cmdlet(VerbsCommon.Get, "CsSnapshotReading")]
    public sealed class GetCsSnapshotReading : ReadingBase, IDynamicParameters
    {
        public object GetDynamicParameters()
        {
            Snapshot.Pin(Snapshot.Take(MyInvocation.BoundParameters, Bound, Kind));
            return null;
        }
    }

    [Cmdlet(VerbsCommon.Get, "CsTwoTablesReading")]
    public sealed class GetCsTwoTablesReading : ReadingBase, IDynamicParameters
    {
        public object GetDynamicParameters()
        {
            Snapshot.Pin(Snapshot.Copy(Snapshot.TakeWrapped(MyInvocation.BoundParameters, Bound, Kind)));
            return null;
        }
    }

    [Cmdlet(VerbsCommon.Get, "CsOneReading")]
    public sealed class GetCsOneReading : ReadingBase, IDynamicParameters
    {
        public object GetDynamicParameters()
        {
            RuntimeDefinedParameterDictionary dict = new RuntimeDefinedParameterDictionary();
            if (Kind == "temperature")
            {
                Collection<Attribute> attrs = new Collection<Attribute>();
                attrs.Add(new ParameterAttribute());
                attrs.Add(new ValidateSetAttribute("C", "F"));
                dict.Add("Unit", new RuntimeDefinedParameter("Unit", typeof(string), attrs));
            }
            return dict;
        }
    }

    public static class Snapshot
    {
        public static object Bare(object value)
        {
            PSObject wrapped = value as PSObject;
            return wrapped != null ? wrapped.BaseObject : value;
        }

        // The table the generated GetDynamicParameters builds for a
        // cmdlet with one parameter: the engine's bound parameters, then
        // the parameter whose setter ran, each value out of its wrapper.
        public static Hashtable Take(IDictionary boundParameters, ulong bits, object kind)
        {
            Hashtable bound = new Hashtable(StringComparer.OrdinalIgnoreCase);
            foreach (DictionaryEntry e in boundParameters) bound[e.Key] = Bare(e.Value);
            if ((bits & 1UL) != 0 && !bound.ContainsKey("Kind")) bound["Kind"] = Bare(kind);
            return bound;
        }

        // The same table with its values left wrapped.
        public static Hashtable TakeWrapped(IDictionary boundParameters, ulong bits, object kind)
        {
            Hashtable bound = new Hashtable(StringComparer.OrdinalIgnoreCase);
            foreach (DictionaryEntry e in boundParameters) bound[e.Key] = e.Value;
            if ((bits & 1UL) != 0 && !bound.ContainsKey("Kind")) bound["Kind"] = kind;
            return bound;
        }

        // A second table holding the first's entries, string keys, values
        // out of their wrappers.
        public static Hashtable Copy(Hashtable bound)
        {
            Hashtable copy = new Hashtable();
            foreach (DictionaryEntry e in bound) copy[e.Key == null ? "" : e.Key.ToString()] = Bare(e.Value);
            return copy;
        }

        // The handle that pins a table for the call into the library.
        public static void Pin(Hashtable table)
        {
            GCHandle handle = GCHandle.Alloc(table);
            handle.Free();
        }
    }

    public static class Direct
    {
        public static double NsPerCall(IDynamicParameters target, int calls)
        {
            Stopwatch sw = Stopwatch.StartNew();
            for (int i = 0; i < calls; i++) target.GetDynamicParameters();
            sw.Stop();
            return sw.Elapsed.TotalMilliseconds * 1e6 / calls;
        }
    }

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
        // all of its time. A process gone by the second reading is not
        // counted, so a short window is the reading that sees a
        // neighbor's short-lived workers.
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
$assembly = (Add-Type -TypeDefinition $source -PassThru)[0].Assembly
Import-Module -Assembly $assembly -Force

$other = 'electricity'
$temperature = 'temperature'

$e2e = [ordered]@{
    'C# plain'     = { param($n) foreach ($i in 1..$n) { $null = Get-CsPlainReading -Kind $other } }
    'C# null'      = { param($n) foreach ($i in 1..$n) { $null = Get-CsNullReading -Kind $other } }
    'C# empty'     = { param($n) foreach ($i in 1..$n) { $null = Get-CsEmptyReading -Kind $other } }
    'C# snapshot'  = { param($n) foreach ($i in 1..$n) { $null = Get-CsSnapshotReading -Kind $other } }
    'C# two tables' = { param($n) foreach ($i in 1..$n) { $null = Get-CsTwoTablesReading -Kind $other } }
    'C# one'       = { param($n) foreach ($i in 1..$n) { $null = Get-CsOneReading -Kind $temperature } }
    'pwrs static'  = { param($n) foreach ($i in 1..$n) { $null = Get-RustStaticReading -Kind $other } }
    'pwrs blind'   = { param($n) foreach ($i in 1..$n) { $null = Get-RustBlindReading -Kind $other } }
    'pwrs dynamic' = { param($n) foreach ($i in 1..$n) { $null = Get-RustReading -Kind $other } }
    'pwrs one'     = { param($n) foreach ($i in 1..$n) { $null = Get-RustReading -Kind $temperature } }
    'CONTROL'      = { param($n) foreach ($i in 1..$n) { $null = Get-RustReading -Kind $other } }
}

# Each arm must do what its name says before any of it is timed.
$expect = [ordered]@{
    'Get-CsPlainReading'    = @($other, "${other}:none", $false)
    'Get-CsNullReading'     = @($other, "${other}:none", $false)
    'Get-CsEmptyReading'    = @($other, "${other}:none", $false)
    'Get-CsSnapshotReading' = @($other, "${other}:none", $false)
    'Get-CsTwoTablesReading' = @($other, "${other}:none", $false)
    'Get-CsOneReading'      = @($temperature, "${temperature}:none", $true)
    'Get-RustStaticReading' = @($other, "${other}:none", $false)
    'Get-RustBlindReading'  = @($temperature, "${temperature}:none", $false)
    'Get-RustReading'       = @($temperature, "${temperature}:none", $true)
}
foreach ($command in $expect.Keys) {
    $kind, $output, $unit = $expect[$command]
    $got = & $command -Kind $kind
    if ($got -ne $output) { throw "$command -Kind $kind wrote '$got', not '$output'" }
    $has = (Get-Command $command -ArgumentList $kind).Parameters.ContainsKey('Unit')
    if ($has -ne $unit) { throw "$command -Kind $kind has -Unit: $has, expected $unit" }
}
if ((Get-Command Get-RustReading -ArgumentList $other).Parameters.ContainsKey('Unit')) { throw "Get-RustReading -Kind $other has -Unit" }
if ((Get-Command Get-RustStaticReading).ImplementingType.GetInterface('IDynamicParameters')) { throw 'Get-RustStaticReading implements IDynamicParameters' }

# One detached instance per direct arm, holding the bound state the
# binder leaves before it asks for dynamic parameters.
function New-Direct([string] $Command, [string] $Kind) {
    $instance = [Activator]::CreateInstance((Get-Command $Command).ImplementingType)
    $instance.Kind = $Kind
    $instance.MyInvocation.BoundParameters['Kind'] = $Kind
    $instance
}
$direct = [ordered]@{
    'C# empty'    = @((New-Direct Get-CsEmptyReading $other), 0)
    'C# snapshot' = @((New-Direct Get-CsSnapshotReading $other), 0)
    'C# two tables' = @((New-Direct Get-CsTwoTablesReading $other), 0)
    'C# one'      = @((New-Direct Get-CsOneReading $temperature), 1)
    'pwrs blind'  = @((New-Direct Get-RustBlindReading $other), 0)
    'pwrs'        = @((New-Direct Get-RustReading $other), 0)
    'pwrs one'    = @((New-Direct Get-RustReading $temperature), 1)
}
foreach ($name in $direct.Keys) {
    $instance, $count = $direct[$name]
    $got = $instance.GetDynamicParameters().Count
    if ($got -ne $count) { throw "direct $name returned $got dynamic parameters, expected $count" }
}

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

# Across every pass: the processor time other processes used and the
# windows it was used in, and the pass whose window read busiest with
# its three busiest processes.
$load = @{ UsedMs = 0.0; WindowMs = 0.0; MaxBusy = 0.0; MaxPass = ''; MaxTop = @() }

function Invoke-Rounds([string[]] $Names, [scriptblock] $Pass, [hashtable] $Ns, [hashtable] $Busy) {
    foreach ($name in $Names) { $Ns[$name] = New-Series; $Busy[$name] = New-Series }
    for ($round = 1; $round -le $Rounds; $round++) {
        foreach ($i in 0..($Names.Count - 1)) {
            $name = $Names[($i + $round) % $Names.Count]
            [System.GC]::Collect()
            [System.GC]::WaitForPendingFinalizers()
            [System.GC]::Collect()
            $before = [PwrsDynBench.Load]::Read()
            $Ns[$name].Add((& $Pass $name))
            $after = [PwrsDynBench.Load]::Read()
            $cores = [PwrsDynBench.Load]::BusyCores($before, $after)
            $Busy[$name].Add($cores)
            $load.UsedMs += [PwrsDynBench.Load]::UsedMs($before, $after)
            $load.WindowMs += [PwrsDynBench.Load]::WindowMs($before, $after)
            if ($cores -gt $load.MaxBusy) {
                $load.MaxBusy = $cores
                $load.MaxPass = "$name round $round"
                $load.MaxTop = [PwrsDynBench.Load]::Busiest($before, $after, 3)
            }
        }
    }
}

$start = [PwrsDynBench.Load]::Read()
Start-Sleep -Milliseconds 1000
$idle = [PwrsDynBench.Load]::Read()

foreach ($name in $e2e.Keys) { & $e2e[$name] $Warmup }
foreach ($name in $direct.Keys) { $null = [PwrsDynBench.Direct]::NsPerCall($direct[$name][0], [math]::Max(1, $DirectCalls / 10)) }

$e2eNs = @{}; $e2eBusy = @{}
Invoke-Rounds @($e2e.Keys) {
    param($name)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $e2e[$name] $Calls
    $sw.Stop()
    $sw.Elapsed.TotalMilliseconds * 1e6 / $Calls
} $e2eNs $e2eBusy

$directNs = @{}; $directBusy = @{}
Invoke-Rounds @($direct.Keys) {
    param($name)
    [PwrsDynBench.Direct]::NsPerCall($direct[$name][0], $DirectCalls)
} $directNs $directBusy

$end = [PwrsDynBench.Load]::Read()

function Get-Difference([hashtable] $Ns, [string[]] $Plus, [string[]] $Minus) {
    $d = New-Series
    for ($r = 0; $r -lt $Rounds; $r++) {
        $v = 0.0
        foreach ($p in $Plus) { $v += $Ns[$p][$r] }
        foreach ($m in $Minus) { $v -= $Ns[$m][$r] }
        $d.Add($v)
    }
    , $d
}

function Format-Row([string] $Label, [System.Collections.Generic.List[double]] $Values) {
    $s = Get-Summary $Values
    '{0,-46} {1,9:N0} [{2,7:N0}, {3,7:N0}]' -f $Label, $s.Median, $s.Q1, $s.Q3
}

"dynamic_params.ps1 host={0} {1} calls={2} direct={3} rounds={4} warmup={5}" -f `
    $PSVersionTable.PSVersion, [Environment]::OSVersion.VersionString, $Calls, $DirectCalls, $Rounds, $Warmup
"busy cores in other processes: {0:N2} in the second before the run, {1:N2} across the passes; {2} processes unreadable" -f `
    [PwrsDynBench.Load]::BusyCores($start, $idle), $(if ($load.WindowMs -gt 0) { $load.UsedMs / $load.WindowMs } else { 0 }), $end.Unreadable
"busiest pass: {0} at {1:N2} cores: {2}" -f $load.MaxPass, $load.MaxBusy, ($load.MaxTop -join ', ')
"busiest across the run among the processes alive at its end: {0}" -f (([PwrsDynBench.Load]::Busiest($idle, $end, 3)) -join ', ')
''
'end to end, ns per call: median [q1, q3] and min over rounds; busy cores in other processes across each pass: median, max'
foreach ($name in $e2e.Keys) {
    $s = Get-Summary $e2eNs[$name]
    $b = Get-Summary $e2eBusy[$name]
    '{0,-14} {1,9:N0} [{2,7:N0}, {3,7:N0}] min {4,7:N0}   busy {5:N2}, {6:N2}' -f $name, $s.Median, $s.Q1, $s.Q3, $s.Min, $b.Median, $b.Max
}
''
'differences taken within each round, ns per call: median [q1, q3]'
'  what PowerShell charges a cmdlet with dynamic parameters'
Format-Row "PowerShell's pass, returning `$null" (Get-Difference $e2eNs @('C# null') @('C# plain'))
Format-Row "PowerShell's pass, returning an empty table" (Get-Difference $e2eNs @('C# empty') @('C# plain'))
'  what PWRS charges on top, the hook adding nothing: its hook hands the engine $null'
Format-Row 'PWRS, the whole, hook reading -Kind' (Get-Difference $e2eNs @('pwrs dynamic') @('pwrs static'))
Format-Row "PWRS's share beyond PowerShell's pass" (Get-Difference $e2eNs @('pwrs dynamic', 'C# plain') @('pwrs static', 'C# null'))
Format-Row '  the snapshot, built in C#' (Get-Difference $e2eNs @('C# snapshot') @('C# null'))
Format-Row '  the call into the library and back' (Get-Difference $e2eNs @('pwrs blind', 'C# plain') @('pwrs static', 'C# snapshot'))
Format-Row "  the hook's read of -Kind" (Get-Difference $e2eNs @('pwrs dynamic') @('pwrs blind'))
Format-Row 'a second copy of the snapshot, built in C#' (Get-Difference $e2eNs @('C# two tables') @('C# snapshot'))
'  one parameter added'
Format-Row "PowerShell's pass" (Get-Difference $e2eNs @('C# one') @('C# plain'))
Format-Row 'PWRS, the whole' (Get-Difference $e2eNs @('pwrs one') @('pwrs static'))
Format-Row "PWRS's share beyond PowerShell's pass" (Get-Difference $e2eNs @('pwrs one', 'C# plain') @('pwrs static', 'C# one'))
Format-Row 'floor: CONTROL - pwrs dynamic' (Get-Difference $e2eNs @('CONTROL') @('pwrs dynamic'))
''
'direct, GetDynamicParameters from a C# loop, ns per call: median [q1, q3] and min over rounds; busy cores: median, max'
foreach ($name in $direct.Keys) {
    $s = Get-Summary $directNs[$name]
    $b = Get-Summary $directBusy[$name]
    '{0,-14} {1,9:N0} [{2,7:N0}, {3,7:N0}] min {4,7:N0}   busy {5:N2}, {6:N2}' -f $name, $s.Median, $s.Q1, $s.Q3, $s.Min, $b.Median, $b.Max
}
''
'direct differences within each round, ns per call: median [q1, q3]'
Format-Row "PWRS's own work, hook reading -Kind" (Get-Difference $directNs @('pwrs') @('C# empty'))
Format-Row '  the snapshot' (Get-Difference $directNs @('C# snapshot') @('C# empty'))
Format-Row '  the call into the library and back' (Get-Difference $directNs @('pwrs blind') @('C# snapshot'))
Format-Row "  the hook's read of -Kind" (Get-Difference $directNs @('pwrs') @('pwrs blind'))
Format-Row 'a second copy of the snapshot' (Get-Difference $directNs @('C# two tables') @('C# snapshot'))
Format-Row "PWRS's own work, one parameter added" (Get-Difference $directNs @('pwrs one') @('C# one'))
''
'residual, end to end less direct, per round: median [q1, q3]'
$ownE2e = Get-Difference $e2eNs @('pwrs dynamic', 'C# plain') @('pwrs static', 'C# empty')
$ownDirect = Get-Difference $directNs @('pwrs') @('C# empty')
$residual = New-Series
for ($r = 0; $r -lt $Rounds; $r++) { $residual.Add($ownE2e[$r] - $ownDirect[$r]) }
Format-Row "PWRS's own share, nothing added" $residual
$oneE2e = Get-Difference $e2eNs @('pwrs one', 'C# plain') @('pwrs static', 'C# one')
$oneDirect = Get-Difference $directNs @('pwrs one') @('C# one')
$residual = New-Series
for ($r = 0; $r -lt $Rounds; $r++) { $residual.Add($oneE2e[$r] - $oneDirect[$r]) }
Format-Row "PWRS's own share, one parameter added" $residual
