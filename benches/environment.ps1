# The state that moves a PowerShell measurement more than most of what is
# being measured. Run this before a session and report it with any table.
#
# Module logging, script block logging and transcription each add a fixed
# cost to every cmdlet invocation or script block compilation, so a figure
# taken with them on does not transfer to a machine with them off, or back.
# Defender's real-time scanning lands on .NET calls and file probes rather
# than on computation. A loaded box moves a whole-import figure by more
# than the constructs inside it.
$ErrorActionPreference = 'Stop'

"host          PowerShell {0} ({1})" -f $PSVersionTable.PSVersion, $PSVersionTable.PSEdition
"os            {0}" -f [System.Environment]::OSVersion.VersionString
"processors    {0}" -f [System.Environment]::ProcessorCount
"server gc     {0}" -f [System.Runtime.GCSettings]::IsServerGC

$policy = 'SOFTWARE\Policies\Microsoft\Windows\PowerShell'
foreach ($leaf in 'ModuleLogging', 'ScriptBlockLogging', 'Transcription') {
    $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("$policy\$leaf")
    if ($key) {
        foreach ($n in $key.GetValueNames()) { "policy        {0}\{1} = {2}" -f $leaf, $n, $key.GetValue($n) }
        $key.Close()
    } else {
        "policy        {0} not set" -f $leaf
    }
}

try {
    $mp = Get-MpComputerStatus -ErrorAction Stop
    "defender      real-time {0}, engine {1}" -f (-not $mp.RealTimeProtectionEnabled ? 'off' : 'ON'), $mp.AMEngineVersion
} catch {
    "defender      not readable from this host"
}

$busiest = Get-Process | Sort-Object CPU -Descending | Select-Object -First 3
foreach ($p in $busiest) { "busiest       {0} cpu {1:N0}s" -f $p.Name, $p.CPU }
$load = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
"cpu load      {0}%" -f $load
if ($load -gt 15) { "`nWARNING: {0}% load. A figure read here is not a quiet-machine figure." -f $load }
