# The state that moves a PowerShell measurement more than most of what is
# being measured. Run this before a session and report it with any table.
#
# Module logging, script block logging and transcription each add a fixed
# cost to every cmdlet invocation or script block compilation, so a figure
# taken with them on does not transfer to a machine with them off, or back.
# Windows PowerShell reads them from its Group Policy key. PowerShell 7
# reads its own key on Windows, or Windows PowerShell's where its own says
# to, and on every platform the PowerShellPolicies of powershell.config.json
# in $PSHOME and in the user's configuration folder. Every one of those
# places is read here, and each value set prints with where it is set.
# Defender's real-time scanning adds cost to .NET calls and file probes
# rather than to computation. A loaded machine moves a whole-import figure
# by more than the constructs inside it, and a virtual machine also loses
# the time its host gives other guests, which Linux counts as steal.
$ErrorActionPreference = 'Stop'

# Windows PowerShell has no $IsWindows, and runs only on Windows.
$onWindows = $PSVersionTable.PSEdition -eq 'Desktop' -or $IsWindows
$kernel = if ($onWindows) { 'Windows' } else { "$(uname -s)" }

$os = [System.Environment]::OSVersion.VersionString
if ($kernel -eq 'Linux' -and (Test-Path -LiteralPath /etc/os-release)) {
    $pretty = Select-String -LiteralPath /etc/os-release -Pattern '^PRETTY_NAME="?([^"]*)"?$' | Select-Object -First 1
    if ($pretty) { $os = '{0}, {1}' -f $pretty.Matches[0].Groups[1].Value, $os }
} elseif ($kernel -eq 'FreeBSD') {
    $os = 'FreeBSD {0}, {1}' -f "$(freebsd-version)", $os
}

"host          PowerShell {0} ({1})" -f $PSVersionTable.PSVersion, $PSVersionTable.PSEdition
"os            {0}" -f $os
"processors    {0}" -f [System.Environment]::ProcessorCount
"server gc     {0}" -f [System.Runtime.GCSettings]::IsServerGC

$leaves = 'ModuleLogging', 'ScriptBlockLogging', 'Transcription'
$places = @()
$found = 0
if ($onWindows) {
    $places += 'Group Policy'
    foreach ($hive in [Microsoft.Win32.Registry]::LocalMachine, [Microsoft.Win32.Registry]::CurrentUser) {
        foreach ($root in 'SOFTWARE\Policies\Microsoft\PowerShellCore', 'SOFTWARE\Policies\Microsoft\Windows\PowerShell') {
            foreach ($leaf in $leaves) {
                $key = $hive.OpenSubKey("$root\$leaf")
                if (-not $key) { continue }
                foreach ($n in $key.GetValueNames()) {
                    "policy        {0}\{1}\{2} {3} = {4}" -f $hive.Name, $root, $leaf, $n, $key.GetValue($n)
                    $found++
                }
                $key.Close()
            }
        }
    }
}
if ($PSVersionTable.PSEdition -eq 'Core') {
    $places += 'powershell.config.json'
    $userFolder = if ($onWindows) {
        Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'PowerShell'
    } elseif ($env:XDG_CONFIG_HOME) {
        Join-Path $env:XDG_CONFIG_HOME 'powershell'
    } else {
        Join-Path $HOME '.config/powershell'
    }
    foreach ($file in (Join-Path $PSHOME 'powershell.config.json'), (Join-Path $userFolder 'powershell.config.json')) {
        if (-not (Test-Path -LiteralPath $file)) { continue }
        try {
            $policies = (Get-Content -Raw -LiteralPath $file | ConvertFrom-Json).PowerShellPolicies
        } catch {
            "policy        {0} is not JSON PowerShell can read: {1}" -f $file, $_.Exception.Message
            $found++
            continue
        }
        if ($null -eq $policies) { continue }
        foreach ($leaf in $leaves) {
            if ($null -eq $policies.$leaf) { continue }
            foreach ($setting in $policies.$leaf.PSObject.Properties) {
                "policy        {0} {1}.{2} = {3}" -f $file, $leaf, $setting.Name, ($setting.Value -join ', ')
                $found++
            }
        }
    }
}
if ($found -eq 0) { "policy        {0} not set in {1}" -f ($leaves -join ', '), ($places -join ' or ') }

try {
    $mp = Get-MpComputerStatus -ErrorAction Stop
    "defender      real-time {0}, engine {1}" -f $(if ($mp.RealTimeProtectionEnabled) { 'ON' } else { 'off' }), $mp.AMEngineVersion
} catch {
    "defender      not readable from this host"
}

# PowerShell 7.5 on FreeBSD reports process CPU times millions of times
# larger than ps does, so there they come from ps, which prints each as
# minutes:seconds.hundredths. The idle process's time is idle time.
if ($kernel -eq 'FreeBSD') {
    $busiest = /bin/ps -ax -o time= -o ucomm= | ForEach-Object {
        if ($_ -match '^\s*(\d+):(\d+)\.(\d+)\s+(.+)$' -and $Matches[4] -ne 'idle') {
            [pscustomobject] @{ Name = $Matches[4]; CPU = [int] $Matches[1] * 60 + [int] $Matches[2] + [int] $Matches[3] / 100 }
        }
    } | Sort-Object CPU -Descending | Select-Object -First 3
} else {
    $busiest = Get-Process | Sort-Object CPU -Descending | Select-Object -First 3
}
foreach ($p in $busiest) { "busiest       {0} cpu {1:N0}s" -f $p.Name, $p.CPU }

# The share of every processor's time that was busy over one second.
$load = $null
if ($onWindows) {
    $load = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
    "cpu load      {0}%" -f $load
} elseif ($kernel -eq 'Linux') {
    # The first line of /proc/stat counts the ticks of every processor:
    # user nice system idle iowait irq softirq steal. Busy leaves out idle,
    # iowait and steal, the time the host gave another guest.
    $ticks = { @(((Get-Content -LiteralPath /proc/stat -TotalCount 1) -split '\s+')[1..8] | ForEach-Object { [long] $_ }) }
    $before = & $ticks
    Start-Sleep -Seconds 1
    $after = & $ticks
    $d = @(for ($i = 0; $i -lt 8; $i++) { $after[$i] - $before[$i] })
    $total = ($d | Measure-Object -Sum).Sum
    if ($total -gt 0) {
        $load = 100 * ($d[0] + $d[1] + $d[2] + $d[5] + $d[6]) / $total
        $average = ((Get-Content -LiteralPath /proc/loadavg) -split ' ')[0..2] -join ' '
        "cpu load      {0:N0}%, steal {1:N1}%; load average {2}" -f $load, (100 * $d[7] / $total), $average
    }
} elseif ($kernel -eq 'FreeBSD') {
    # kern.cp_time counts the ticks of every processor: user nice system
    # interrupt idle.
    $ticks = { @("$(sysctl -n kern.cp_time)".Trim() -split '\s+' | ForEach-Object { [long] $_ }) }
    $before = & $ticks
    Start-Sleep -Seconds 1
    $after = & $ticks
    $d = @(for ($i = 0; $i -lt 5; $i++) { $after[$i] - $before[$i] })
    $total = ($d | Measure-Object -Sum).Sum
    if ($total -gt 0) {
        $load = 100 * ($total - $d[4]) / $total
        "cpu load      {0:N0}%; load average {1}" -f $load, "$(sysctl -n vm.loadavg)".Trim('{', '}', ' ')
    }
} else {
    "cpu load      not read on {0}; load average {1}" -f $kernel, "$(sysctl -n vm.loadavg)".Trim('{', '}', ' ')
}
if ($load -gt 15) { "`nWARNING: {0:N0}% load. A figure read here is not a quiet-machine figure." -f $load }
