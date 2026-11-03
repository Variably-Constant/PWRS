# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE points
# at the built module folder.
#
# A COM object made late-bound, as New-Object -ComObject makes one,
# reached from Rust through PsObject: hello's Test-RustComDictionary,
# Get-RustComItem, New-RustComWeakReference, Invoke-RustWmiQuery,
# Register-RustComEvent and Get-RustComEventCount. COM is Windows's
# alone, so elsewhere every case is skipped, as its name says.
BeforeDiscovery {
    $onWindows = [IO.Path]::DirectorySeparatorChar -eq '\'
}

BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # Waits on a WaitHandle, a wait that delivers the events of a COM
    # object made on this thread, until $Until is true or $Seconds pass.
    function Wait-Pumping([scriptblock] $Until, [int] $Seconds = 15) {
        $wait = New-Object System.Threading.ManualResetEvent $false
        try {
            $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
            while ([DateTime]::UtcNow -lt $deadline -and -not (& $Until)) { [void]$wait.WaitOne(50) }
        }
        finally { $wait.Dispose() }
    }
}

Describe 'A COM object through PsObject, on Windows' -Skip:(-not $onWindows) {
    It 'sets a property, calls methods and enumerates a dictionary Rust made' {
        Test-RustComDictionary -Key a, b -Value 1, 2 -Find A | Should -Be @('Count=2', 'Exists=true', 'Item=1', 'Keys=a,b')
    }

    It 'does the same to a dictionary passed in, which the caller then sees' {
        $d = New-Object -ComObject Scripting.Dictionary
        Test-RustComDictionary -Dictionary $d -Key x, y -Value 10, 20 -Find X | Should -Be @('Count=2', 'Exists=true', 'Item=10', 'Keys=x,y')
        $d.Count | Should -Be 2
        $d.CompareMode | Should -Be 1
        $d.Item('Y') | Should -Be '20'
    }

    It 'enumerates a COM collection passed in, reading each item''s property' {
        $fso = New-Object -ComObject Scripting.FileSystemObject
        @(Get-RustComItem -Collection $fso.Drives -Property DriveLetter) | Should -Be @($fso.Drives | ForEach-Object DriveLetter)
    }

    It 'leaves nothing referencing a COM object once Rust drops it' {
        $weak = New-RustComWeakReference Scripting.Dictionary
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
        [GC]::Collect()
        $weak.IsAlive | Should -BeFalse
    }
}

Describe 'A COM object''s events, on Windows' -Skip:(-not $onWindows) {
    BeforeAll {
        $script:caption = (Get-CimInstance Win32_OperatingSystem).Caption
    }

    It 'reach a Rust callback while the pipeline thread waits on a WaitHandle' {
        Invoke-RustWmiQuery -Query 'select Caption from Win32_OperatingSystem' -Property Caption | Should -Be $caption
    }

    It 'reach the runspace''s event queue, which PsEvents::wait reads back' {
        Invoke-RustWmiQuery -Query 'select Caption from Win32_OperatingSystem' -Property Caption -Queue | Should -Be $caption
    }

    It 'each reach the callback, one for every object the query finds' {
        $direct = @(Get-CimInstance Win32_Service | ForEach-Object Name | Sort-Object)
        @(Invoke-RustWmiQuery -Query 'select Name from Win32_Service' -Property Name | Sort-Object) | Should -Be $direct
    }

    It 'refuses an event the object does not have, naming those it has' {
        $sink = New-Object -ComObject WbemScripting.SWbemSink
        { Register-RustComEvent -InputObject $sink -EventName NoSuchEvent } | Should -Throw '*the COM object SWbemSink has no event NoSuchEvent; its events are OnObjectReady, OnCompleted, OnProgress, OnObjectPut*'
    }

    It 'leaves a handler attached by Rust in place until the module unloads, which detaches it' {
        $sink = New-Object -ComObject WbemScripting.SWbemSink
        $services = (New-Object -ComObject WbemScripting.SWbemLocator).ConnectServer('.', 'root\cimv2')
        $before = Get-RustComEventCount OnCompleted
        Register-RustComEvent -InputObject $sink -EventName OnCompleted
        $services.ExecQueryAsync($sink, 'select Caption from Win32_OperatingSystem')
        Wait-Pumping { (Get-RustComEventCount OnCompleted) -gt $before }
        $heard = Get-RustComEventCount OnCompleted
        $heard | Should -Be ($before + 1)

        Remove-Module Hello
        Import-Module (Join-Path $env:PWRS_MODULE 'Hello.psd1') -Force -ErrorAction Stop
        $services.ExecQueryAsync($sink, 'select Caption from Win32_OperatingSystem')
        Wait-Pumping { $false } -Seconds 3
        Get-RustComEventCount OnCompleted | Should -Be $heard
    }

    It 'leaves no handler on a COM object when the host exits, so the host exits cleanly' {
        $script = Join-Path $TestDrive 'kept-at-exit.ps1'
        $manifest = Join-Path $env:PWRS_MODULE 'Hello.psd1'
        Set-Content -LiteralPath $script -Value @"
Import-Module '$manifest' -ErrorAction Stop
Test-RustComDictionary -Key a, b -Value 1, 2 -Find A | Out-Null
`$fso = New-Object -ComObject Scripting.FileSystemObject
Get-RustComItem -Collection `$fso.Drives -Property DriveLetter | Out-Null
Invoke-RustWmiQuery -Query 'select Caption from Win32_OperatingSystem' -Property Caption | Out-Null
`$sink = New-Object -ComObject WbemScripting.SWbemSink
Register-RustComEvent -InputObject `$sink -EventName OnCompleted
`$services = (New-Object -ComObject WbemScripting.SWbemLocator).ConnectServer('.', 'root\cimv2')
`$services.ExecQueryAsync(`$sink, 'select Caption from Win32_OperatingSystem')
`$wait = New-Object System.Threading.ManualResetEvent `$false
`$deadline = [DateTime]::UtcNow.AddSeconds(15)
while ([DateTime]::UtcNow -lt `$deadline -and (Get-RustComEventCount OnCompleted) -lt 1) { [void]`$wait.WaitOne(50) }
'heard ' + (Get-RustComEventCount OnCompleted)
"@
        $exe = (Get-Process -Id $PID).Path
        $runs = foreach ($i in 1..4) {
            $out = & $exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script 2>&1
            "exit $LASTEXITCODE, $($out -join ' ')"
        }
        $runs | Should -Be @('exit 0, heard 1', 'exit 0, heard 1', 'exit 0, heard 1', 'exit 0, heard 1')
    }
}
