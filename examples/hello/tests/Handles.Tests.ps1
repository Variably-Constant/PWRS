# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# One proxy object stands for a whole series. Each stage takes it by
# parameter type, reads it in place through PsProxy and writes a new one,
# so no number exists until a cmdlet asks for rows. These pin that the
# object passes through commands that know nothing of it, that a stage
# leaves its input as it was, that the binder refuses anything else, that
# the object's gate refuses a second reach from the thread holding it,
# that a stop reaches a worker sending nothing, and that a class reporting
# its native bytes is collected on them.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $script:psd1 = Join-Path $module 'Hello.psd1'
    Import-Module $psd1 -Force -ErrorAction Stop
}

Describe 'A series taken by type' {
    It 'is one object until a cmdlet writes its numbers' {
        $s = @(New-RustSeries 5 | Add-RustSeriesStep -Scale 2 | Add-RustSeriesStep -Shift 1)
        $s.Count | Should -Be 1
        $s[0].GetType().FullName | Should -Be 'Hello.Series'
        $s[0].Plan -join ', ' | Should -Be 'scale 2, shift 1'
        ($s[0] | Expand-RustSeries) -join ',' | Should -Be '1,3,5,7,9'
    }

    It 'applies its steps in the order they were added' {
        (New-RustSeries 6 | Add-RustSeriesStep -Scale 2 -Above 4 | Expand-RustSeries) -join ',' | Should -Be '6,8,10'
        (New-RustSeries 6 | Add-RustSeriesStep -Above 4 | Add-RustSeriesStep -Scale 2 | Expand-RustSeries) -join ',' | Should -Be '10'
    }

    It 'leaves the series a stage was given as it was' {
        $s = New-RustSeries 3
        $t = $s | Add-RustSeriesStep -Shift 10
        ($s | Expand-RustSeries) -join ',' | Should -Be '0,1,2'
        ($t | Expand-RustSeries) -join ',' | Should -Be '10,11,12'
        @($s.Plan).Count | Should -Be 0
    }

    It 'passes through commands that know nothing of it' {
        (New-RustSeries 3 | Where-Object { $true } | ForEach-Object { $_ } | Add-RustSeriesStep -Scale 3 | Expand-RustSeries) -join ',' | Should -Be '0,3,6'
    }

    It 'is taken from a variable, by name and by position' {
        $s = New-RustSeries 4 | Add-RustSeriesStep -Above 1
        (Expand-RustSeries -Series $s) -join ',' | Should -Be '2,3'
        (Measure-RustSeries $s).Sum | Should -Be 5
    }

    It 'reads several series in turn' {
        $a = New-RustSeries 2
        $b = New-RustSeries 2 | Add-RustSeriesStep -Shift 5
        (@($a, $b) | Expand-RustSeries) -join ',' | Should -Be '0,1,5,6'
    }

    It 'totals a million numbers without writing one' {
        $total = New-RustSeries 1000000 | Add-RustSeriesStep -Scale 2 | Measure-RustSeries
        $total.Count | Should -Be 1000000
        $total.Sum | Should -Be 999999000000
    }

    It 'is the only kind of object the binder accepts' {
        { 'text' | Expand-RustSeries -ErrorAction Stop } | Should -Throw -ErrorId 'InputObjectNotBound*'
        { Expand-RustSeries -Series (New-Counter 'c') -ErrorAction Stop } | Should -Throw '*Hello.Series*'
    }

    It 'is refused once disposed' {
        $s = New-RustSeries 3
        $s.Dispose()
        { $s | Expand-RustSeries -ErrorAction Stop } | Should -Throw '*disposed*'
    }
}

Describe 'The gate on a series a cmdlet holds' {
    It 'lets a property read from the thread holding it as a shared reader through' {
        $s = New-RustSeries 3 | Add-RustSeriesStep -Scale 2
        Test-RustSeriesHold $s { $s.get_Count() } | Should -Be 'ran: 3'
        Test-RustSeriesHold $s { [string]($null -eq $s.Plan) } | Should -Be 'ran: False'
    }

    It 'refuses a call from the thread holding it exclusively' {
        $s = New-RustSeries 3
        Test-RustSeriesHold $s -Exclusive { $s.get_Count() } | Should -BeLike 'refused: *in use by a call already running on this thread*'
    }

    It 'gives a property read from that thread $null under an exclusive hold, as a read after Dispose' {
        $s = New-RustSeries 3 | Add-RustSeriesStep -Scale 2
        Test-RustSeriesHold $s -Exclusive { [string]($null -eq $s.Plan) } | Should -Be 'ran: True'
    }

    It 'frees a series disposed while held once the holder is done' {
        $s = New-RustSeries 3
        Test-RustSeriesHold $s { $s.Dispose() } | Should -Be 'ran: '
        $s.IsDisposed | Should -BeTrue
        $x = New-RustSeries 3
        Test-RustSeriesHold $x -Exclusive { $x.Dispose() } | Should -Be 'ran: '
        $x.IsDisposed | Should -BeTrue
    }

    It 'reads as before once the hold is over' {
        $s = New-RustSeries 3
        Test-RustSeriesHold $s { 'x' } | Should -Be 'ran: x'
        $s.Count | Should -Be 3
    }
}

Describe 'A worker that sends nothing' {
    It 'is let go when the command downstream has what it wanted' {
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $out = @(Wait-RustSilence 60 | Select-Object -First 1)
        $clock.Stop()
        $out | Should -Be @('started')
        $clock.Elapsed.TotalSeconds | Should -BeLessThan 10
    }

    It 'is let go when the pipeline is stopped' {
        # The output collection is read by count and index: enumerating
        # one that is still open waits for it to close.
        $ps = [powershell]::Create()
        try {
            $null = $ps.AddScript("Import-Module '$psd1' -ErrorAction Stop; Wait-RustSilence 60")
            $out = New-Object 'System.Management.Automation.PSDataCollection[psobject]'
            $null = $ps.BeginInvoke([System.Management.Automation.PSDataCollection[psobject]]$null, $out)
            $deadline = [DateTime]::UtcNow.AddSeconds(30)
            while ($out.Count -eq 0 -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 20 }
            $out.Count | Should -Be 1
            $out[0] | Should -Be 'started'
            $clock = [Diagnostics.Stopwatch]::StartNew()
            $ps.Stop()
            $clock.Stop()
            $clock.Elapsed.TotalSeconds | Should -BeLessThan 10
            $ps.InvocationStateInfo.State | Should -Be 'Stopped'
            $out.Count | Should -Be 1
        } finally {
            # The import in that runspace ran the module's import hook, so
            # it is removed there too, as Lifecycle.Tests.ps1 counts.
            $ps.Commands.Clear()
            $null = $ps.AddScript('Remove-Module Hello -Force').Invoke()
            $ps.Dispose()
        }
    }

    It 'finishes when nothing stops it' {
        @(Wait-RustSilence 1) | Should -Be @('started', 'finished')
    }
}

Describe 'A proxy that reports its native bytes' {
    BeforeAll {
        # The figure the runtime holds with the collector for one object.
        function script:Get-Reported($o) {
            $o.GetType().BaseType.GetField('_pressure', [Reflection.BindingFlags]'NonPublic,Instance').GetValue($o)
        }
    }

    It 'reports its bytes when made, again after a change in place, and none once freed' {
        $b = New-RustBallast 1
        Get-Reported $b | Should -Be 1048576
        $same = $b | Set-RustBallast -Megabytes 3
        [object]::ReferenceEquals($same.PSObject.BaseObject, $b.PSObject.BaseObject) | Should -BeTrue
        $b.Claimed | Should -Be 3145728
        Get-Reported $b | Should -Be 3145728
        $b.Dispose()
        Get-Reported $b | Should -Be 0
    }

    It 'reports nothing for a class that names no native_bytes' {
        $q = New-RustBallast 5 -Quiet
        Get-Reported $q | Should -Be 0
        $q.Dispose()
    }

    It 'is collected on what it reports, where one that reports nothing waits' {
        # The objects come 20 ms apart: in pwsh, made back to back, they
        # drew one full collection, which freed none of them. Which
        # generation answers a report differs by host, so the count taken
        # is of collections of any generation.
        $runs = foreach ($quiet in $false, $true) {
            [GC]::Collect()
            [GC]::WaitForPendingFinalizers()
            [GC]::Collect()
            Start-Sleep -Milliseconds 200
            if ($quiet) { Measure-RustPressure 64 -Megabytes 256 -Interval 20 -Quiet } else { Measure-RustPressure 64 -Megabytes 256 -Interval 20 }
        }
        $reported, $quiet = $runs
        $reported.Made | Should -Be 64
        $reported.Collections | Should -BeGreaterThan 0
        $reported.Alive | Should -BeLessThan 64
        $quiet.Alive | Should -BeGreaterThan $reported.Alive
    }
}
