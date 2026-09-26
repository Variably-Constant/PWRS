# End-to-end wall-clock timing of a built pwrs module against a
# hand-written C# cmdlet and an equivalent PowerShell advanced
# function, all in one host. The C# baseline is compiled in-process
# with Add-Type (in-box Roslyn, no SDK needed), so this runs anywhere
# pwsh runs.
#
# Measurement discipline, because the effects being chased are a few
# percent and the noise without it is tens of percent:
#   - every case is warmed with a real workload, not one call, so no
#     case pays another's tiered-JIT promotion;
#   - repetitions are round robin across cases rather than one long
#     block each, so a machine that drifts costs every case equally;
#   - the reported number per case is the median of its repetitions;
#   - CONTROL repeats the first case under a second name, so residual
#     position effect is visible in the output rather than hidden.
# Interpret numbers only on a quiet box.
param(
    [Parameter(Mandatory)] [string] $Module,
    [int] $Iterations = 50000,
    [int] $Reps = 5,
    [int] $Warmup = 5000,
    # Emit one CSV row per case instead of a table, for a driver that
    # aggregates several runs.
    [switch] $Csv
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $Module 'Hello.psd1') -Force -ErrorAction Stop

# Hand-written C# cmdlet doing the same work as pwrs Get-Greeting.
Add-Type -TypeDefinition @'
using System;
using System.Management.Automation;
[Cmdlet(VerbsCommon.Get, "GreetingCs")]
public sealed class GetGreetingCsCommand : PSCmdlet
{
    [Parameter(Mandatory = true, Position = 0, ValueFromPipeline = true)]
    public string Name { get; set; }
    [Parameter] public int Count { get; set; }
    public GetGreetingCsCommand() { Count = 1; }

    private bool _asked;
    private bool _verbose;
    private bool VerboseOn
    {
        get
        {
            if (!_asked)
            {
                object bound;
                _verbose = MyInvocation.BoundParameters.TryGetValue("Verbose", out bound)
                    ? ((SwitchParameter)bound).IsPresent
                    : !"SilentlyContinue".Equals(
                        Convert.ToString(GetVariableValue("VerbosePreference")), StringComparison.OrdinalIgnoreCase);
                _asked = true;
            }
            return _verbose;
        }
    }

    protected override void ProcessRecord()
    {
        // Both sides write the same verbose record and both ask first,
        // so neither is charged for a format the other skips.
        if (VerboseOn) WriteVerbose("greeting " + Name);
        for (int i = 0; i < Count; i++) WriteObject("Hello, " + Name + "!");
    }
}
'@ -PassThru | ForEach-Object { } # force compile now
Import-Module ([GetGreetingCsCommand].Assembly) -Force

# Equivalent PowerShell advanced function.
function Get-GreetingPs {
    [CmdletBinding()]
    param([Parameter(Mandatory, ValueFromPipeline)] [string] $Name)
    begin {
        $verbose = if ($PSBoundParameters.ContainsKey('Verbose')) { [bool] $PSBoundParameters['Verbose'] }
                   else { $VerbosePreference -ne 'SilentlyContinue' }
    }
    process {
        if ($verbose) { Write-Verbose "greeting $Name" }
        "Hello, $Name!"
    }
}

$cases = [ordered] @{
    'pwrs   Get-Greeting -Name x (loop)'   = { param($n) 1..$n | ForEach-Object { Get-Greeting -Name x } }
    'C#     Get-GreetingCs -Name x (loop)' = { param($n) 1..$n | ForEach-Object { Get-GreetingCs -Name x } }
    'ps fn  Get-GreetingPs -Name x (loop)' = { param($n) 1..$n | ForEach-Object { Get-GreetingPs -Name x } }
    'pwrs   pipeline'                      = { param($n) 1..$n | Get-Greeting }
    'C#     pipeline'                      = { param($n) 1..$n | Get-GreetingCs }
    'ps fn  pipeline'                      = { param($n) 1..$n | ForEach-Object { $_ } | Get-GreetingPs }
    # Each CONTROL runs a pwrs case a second time under a different
    # name. Its distance from the case it duplicates is the run's noise
    # floor. There is one per shape, because the two shapes do not move
    # together.
    'CONTROL pwrs loop'                    = { param($n) 1..$n | ForEach-Object { Get-Greeting -Name x } }
    'CONTROL pwrs pipeline'                = { param($n) 1..$n | Get-Greeting }
}

foreach ($name in $cases.Keys) {
    & $cases[$name] $Warmup | Out-Null
}

$samples = @{}
$names = @($cases.Keys)
foreach ($name in $names) { $samples[$name] = [System.Collections.Generic.List[long]]::new() }
for ($rep = 1; $rep -le $Reps; $rep++) {
    # The order rotates every rep, so no case keeps the first slot,
    # where the machine is coldest; position is otherwise worth more
    # than the differences being read. Give -Reps at least as many
    # rounds as there are cases for every case to have held every
    # position.
    $order = @(0..($names.Count - 1) | ForEach-Object { $names[($_ + $rep - 1) % $names.Count] })
    foreach ($name in $order) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        & $cases[$name] $Iterations | Out-Null
        $sw.Stop()
        $samples[$name].Add($sw.ElapsedMilliseconds)
    }
}

function Get-Median([System.Collections.Generic.List[long]] $Values) {
    $sorted = @($Values | Sort-Object)
    $n = $sorted.Count
    if ($n -eq 0) { return 0 }
    if ($n % 2 -eq 1) { return [long] $sorted[[int](($n - 1) / 2)] }
    return [long] (($sorted[$n / 2 - 1] + $sorted[$n / 2]) / 2)
}

$results = foreach ($name in $cases.Keys) {
    $v = $samples[$name]
    [pscustomobject]@{
        Case = $name
        Ms   = Get-Median $v
        Min  = ($v | Measure-Object -Minimum).Minimum
        Max  = ($v | Measure-Object -Maximum).Maximum
    }
}

# Process-level cost, so a case that looks slower can be told apart
# from a process that is collecting more.
$proc = [System.Diagnostics.Process]::GetCurrentProcess()
# .NET Framework has no GetTotalAllocatedBytes; report -1 there.
$allocated = if ([System.GC].GetMethod('GetTotalAllocatedBytes')) { [long] ([System.GC]::GetTotalAllocatedBytes($false) / 1MB) } else { -1 }
$diag = [pscustomobject]@{
    Gen0        = [System.GC]::CollectionCount(0)
    Gen1        = [System.GC]::CollectionCount(1)
    Gen2        = [System.GC]::CollectionCount(2)
    AllocatedMb = $allocated
    HeapMb      = [long] ([System.GC]::GetTotalMemory($false) / 1MB)
    WorkingMb   = [long] ($proc.WorkingSet64 / 1MB)
    CpuMs       = [long] $proc.TotalProcessorTime.TotalMilliseconds
}

if ($Csv) {
    $results | ConvertTo-Csv -NoTypeInformation
    foreach ($p in $diag.PSObject.Properties) {
        '"diag:{0}","{1}","{1}","{1}"' -f $p.Name, $p.Value
    }
} else {
    "host={0} iterations={1} reps={2} warmup={3}" -f $PSVersionTable.PSVersion, $Iterations, $Reps, $Warmup
    $results | Format-Table -AutoSize | Out-String
    $diag | Format-List | Out-String
}
