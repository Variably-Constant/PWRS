# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A progress record carries every field it is given. A worker thread
# writes every stream through one channel, in the order it sent them,
# and of the progress for one activity waiting together only the latest
# is written. The parallel helpers run at most the count a command sets,
# and a stopped pipeline starts no further item and does not wait out
# the ones running.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $script:psd1 = Join-Path $module 'Hello.psd1'
    Import-Module $psd1 -Force -ErrorAction Stop

    # Runs $Script in a runspace of its own that imports the module, and
    # answers what it wrote to output, progress and error. The import ran
    # the module's import hook, so the module is removed there again, as
    # Lifecycle.Tests.ps1 counts.
    function Invoke-InOwnRunspace([string] $Script) {
        $ps = [powershell]::Create()
        try {
            $null = $ps.AddScript("Import-Module '$psd1' -ErrorAction Stop; $Script")
            $output = @($ps.Invoke())
            [pscustomobject]@{
                Output = $output
                Progress = @($ps.Streams.Progress)
                Error = @($ps.Streams.Error)
            }
        } finally {
            $ps.Commands.Clear()
            $null = $ps.AddScript('Remove-Module Hello -Force').Invoke()
            $ps.Dispose()
        }
    }

    # Starts $Script in a runspace of its own, waits until an item has
    # started, stops the pipeline, and answers how long the stop took
    # and the state it left. Get-RustParallelStats then reads what ran.
    function Stop-WhenStarted([string] $Script) {
        Get-RustParallelStats -Reset
        $ps = [powershell]::Create()
        try {
            $null = $ps.AddScript("Import-Module '$psd1' -ErrorAction Stop; $Script")
            $out = New-Object 'System.Management.Automation.PSDataCollection[psobject]'
            $null = $ps.BeginInvoke([System.Management.Automation.PSDataCollection[psobject]]$null, $out)
            $deadline = [DateTime]::UtcNow.AddSeconds(30)
            while ((Get-RustParallelStats).Started -lt 1 -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 20 }
            $clock = [Diagnostics.Stopwatch]::StartNew()
            $ps.Stop()
            $clock.Stop()
            [pscustomobject]@{ Seconds = $clock.Elapsed.TotalSeconds; State = "$($ps.InvocationStateInfo.State)" }
        } finally {
            $ps.Commands.Clear()
            $null = $ps.AddScript('Remove-Module Hello -Force').Invoke()
            $ps.Dispose()
        }
    }
}

Describe 'Write-RustProgress' {
    It 'writes every field of the record' {
        $r = Invoke-InOwnRunspace "Write-RustProgress -Id 2 -ParentId 1 -Activity Copying -Status '3 of 40 files' -CurrentOperation notes.txt -Percent 7 -SecondsRemaining 95"
        $p = @($r.Progress | Where-Object ActivityId -eq 2)
        $p.Count | Should -Be 1
        $p[0].ParentActivityId | Should -Be 1
        $p[0].Activity | Should -Be 'Copying'
        $p[0].StatusDescription | Should -Be '3 of 40 files'
        $p[0].CurrentOperation | Should -Be 'notes.txt'
        $p[0].PercentComplete | Should -Be 7
        $p[0].SecondsRemaining | Should -Be 95
        "$($p[0].RecordType)" | Should -Be 'Processing'
    }

    It 'leaves a record without a percentage processing, with nothing else set' {
        $r = Invoke-InOwnRunspace "Write-RustProgress -Id 3 -Activity Scanning -Status 'counting files'"
        $p = @($r.Progress | Where-Object ActivityId -eq 3)
        $p.Count | Should -Be 1
        $p[0].PercentComplete | Should -Be -1
        "$($p[0].RecordType)" | Should -Be 'Processing'
        $p[0].ParentActivityId | Should -Be -1
        $p[0].SecondsRemaining | Should -Be -1
        $p[0].CurrentOperation | Should -BeNullOrEmpty
    }

    It 'ends the activity only when told to' {
        $r = Invoke-InOwnRunspace 'Write-RustProgress -Id 2 -Activity Copying -Status done -Completed'
        "$(@($r.Progress | Where-Object ActivityId -eq 2)[0].RecordType)" | Should -Be 'Completed'
    }

    It 'hands back the engine''s refusal of an activity nested under itself' {
        $r = Invoke-InOwnRunspace "try { Write-RustProgress -Id 4 -ParentId 4 -Activity Copying -Status nested -ErrorAction Stop; 'written' } catch { `$_.FullyQualifiedErrorId }"
        $r.Output.Count | Should -Be 1
        $r.Output[0] | Should -BeLike 'PwrsRuntimeError,*'
    }
}

Describe 'A worker thread' {
    It 'writes every stream, in the order it sent them' {
        $VerbosePreference = 'Continue'
        $DebugPreference = 'Continue'
        $all = @(Invoke-RustWorker -ErrorAction Continue *>&1)
        $kinds = foreach ($record in $all) {
            if ($record -is [System.Management.Automation.WarningRecord]) { 'warning' }
            elseif ($record -is [System.Management.Automation.VerboseRecord]) { 'verbose' }
            elseif ($record -is [System.Management.Automation.DebugRecord]) { 'debug' }
            elseif ($record -is [System.Management.Automation.InformationRecord]) { 'information' }
            elseif ($record -is [System.Management.Automation.ErrorRecord]) { 'error' }
            else { "output $record" }
        }
        $kinds -join ',' | Should -Be 'output 1,warning,verbose,debug,information,error,output 2'
    }

    It 'writes its progress and its error' {
        $r = Invoke-InOwnRunspace 'Invoke-RustWorker'
        $r.Output | Should -Be @(1, 2)
        $p = @($r.Progress | Where-Object ActivityId -eq 2)
        $p.Count | Should -Be 1
        $p[0].Activity | Should -Be 'Working'
        $p[0].PercentComplete | Should -Be 50
        $r.Error.Count | Should -Be 1
        $r.Error[0].FullyQualifiedErrorId | Should -BeLike 'WorkerError,*'
        "$($r.Error[0].Exception.Message)" | Should -Be 'from the worker: error'
    }

    It 'reaches -WarningVariable and -InformationVariable' {
        $null = Invoke-RustWorker -WarningVariable warned -InformationVariable informed -WarningAction SilentlyContinue -ErrorAction SilentlyContinue
        @($warned).Count | Should -Be 1
        "$($warned[0])" | Should -Be 'from the worker: warning'
        @($informed).Count | Should -Be 1
        "$($informed[0].MessageData)" | Should -Be 'from the worker: information'
    }

    It 'ends the command with a terminating error it sends' {
        $out = @(try { Invoke-RustWorker -Terminating -ErrorAction Stop 3>$null 6>$null } catch { "caught $($_.FullyQualifiedErrorId)" })
        $out.Count | Should -Be 2
        $out[0] | Should -Be 1
        $out[1] | Should -BeLike 'caught WorkerError,*'
    }

    It 'writes fewer progress records for one activity than it sent, ending on the last' {
        $r = Invoke-InOwnRunspace 'Invoke-RustWorker -ProgressRecords 10000'
        $p = @($r.Progress | Where-Object ActivityId -eq 1)
        $p.Count | Should -BeGreaterThan 0
        $p.Count | Should -BeLessThan 10000
        $p[-1].StatusDescription | Should -Be '10000 of 10000'
        $p[-1].PercentComplete | Should -Be 100
        $r.Output | Should -Be @(1, 2)
    }
}

Describe 'The parallel helpers' {
    It 'run at most the count a command sets' {
        Get-RustParallelStats -Reset
        $null = Invoke-RustParallelWork 12 -Workers 2 -SleepMs 50
        $stats = Get-RustParallelStats
        $stats.Started | Should -Be 12
        $stats.Peak | Should -Be 2
    }

    It 'write mapped items back in input order' {
        @(Invoke-RustParallelWork 8 -Map -Workers 3 -SleepMs 5) | Should -Be @(1, 2, 3, 4, 5, 6, 7, 8)
    }

    It 'refuse a count of zero' {
        { Invoke-RustParallelWork 4 -Workers 0 -ErrorAction Stop } | Should -Throw -ErrorId 'PwrsWorkerCount,*'
    }

    It 'stop par_for_each with no item started after the stop' {
        $r = Stop-WhenStarted 'Invoke-RustParallelWork 32 -Workers 2 -SleepMs 10000'
        $r.Seconds | Should -BeLessThan 5
        $r.State | Should -Be 'Stopped'
        (Get-RustParallelStats).Started | Should -BeLessOrEqual 2
    }

    It 'stop par_map with no item started after the stop' {
        $r = Stop-WhenStarted 'Invoke-RustParallelWork 32 -Map -Workers 2 -SleepMs 10000'
        $r.Seconds | Should -BeLessThan 5
        $r.State | Should -Be 'Stopped'
        (Get-RustParallelStats).Started | Should -BeLessOrEqual 2
    }
}
