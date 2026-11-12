# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A PWRS job is a RustJob: its work runs on a thread of its own, and the
# job cmdlets see it as they see any job. Where PowerShell's own jobs
# show the expected behavior, the case runs Start-Job beside it, the same
# way: a job whose
# work fails is Failed with the reason, and Receive-Job reports it alike.
#
# PowerShell's Receive-Job -Wait removes a job twice when the job ends
# after Receive-Job starts waiting on it and before Receive-Job has written
# what the job has already stored, and reports the second removal as
# ReceiveJobAutoRemovalError for a job no adapter knows any more. A
# RustJob's adapter answers for it until its end has been announced, so
# both removals find it. The cases pipe a job that has just started
# straight to Receive-Job -AutoRemoveJob, and one holds Receive-Job at that
# moment on purpose.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # Removes every RustJob a case left, running or not.
    function Remove-RustJobs {
        Get-Job | Where-Object PSJobTypeName -eq 'RustJob' | Remove-Job -Force
    }
}

Describe 'a job a cmdlet starts with a closure' {
    AfterEach { Remove-RustJobs }

    It 'is a RustJob the job cmdlets find, which runs to Completed' {
        $job = Start-RustCount 5 -Name Five
        $job.PSJobTypeName | Should -Be 'RustJob'
        $job.Command | Should -Be 'Start-RustCount'
        @(Get-Job -Name Five).Count | Should -Be 1
        @(Get-Job -Id $job.Id).Count | Should -Be 1
        $null = Wait-Job $job -Timeout 30
        $job.State | Should -Be 'Completed'
    }

    It 'stores what it writes for Receive-Job, every stream with it' {
        $job = Start-RustCount 4
        $null = Wait-Job $job -Timeout 30
        $job.HasMoreData | Should -BeTrue
        $job.Verbose.Count | Should -Be 4
        $job.Warning.Count | Should -Be 1
        $job.Progress.Count | Should -Be 4
        @(Receive-Job $job -WarningAction SilentlyContinue) | Should -Be @(1, 2, 3, 4)
        $job.HasMoreData | Should -BeFalse
        @(Receive-Job $job).Count | Should -Be 0
    }

    It 'takes the engine''s own name without -Name' {
        (Start-RustCount 1).Name | Should -Match '^Job\d+$'
    }

    It 'is stored by its own module''s job source adapter, beside the bundled module''s' {
        # The job manager registers an adapter under its class name alone.
        # Calc, which hello bundles, is imported first and has an adapter
        # of its own.
        $job = Start-RustCount 1 -Name Stored
        $null = Wait-Job $job -Timeout 30
        $manager = [runspace]::DefaultRunspace.JobManager
        $registered = $manager.GetType().GetField('_sourceAdapters', [System.Reflection.BindingFlags] 'Instance, NonPublic')
        $registered | Should -Not -BeNullOrEmpty -Because 'the case reads the job manager''s adapters from its _sourceAdapters field'
        $adapters = $registered.GetValue($manager)
        @($adapters.Keys) | Should -Contain 'CalcRustJobSourceAdapter'
        $holders = @($adapters.Values | Where-Object { $_.GetJobByInstanceId($job.InstanceId, $true) })
        $holders.Count | Should -Be 1
        $holders[0].GetType().FullName | Should -Be 'Pwrs.Modules.Hello.HelloRustJobSourceAdapter'
    }

    It 'ends Stopped when Stop-Job requests a stop, and still has what it wrote' {
        $job = Start-RustCount 100000 -DelayMs 20
        Start-Sleep -Milliseconds 300
        Stop-Job $job
        $job.State | Should -Be 'Stopped'
        $written = @(Receive-Job $job -WarningAction SilentlyContinue)
        $written.Count | Should -BeGreaterThan 0
        $written.Count | Should -BeLessThan 100000
    }

    It 'ends Failed with the work''s error, as a Start-Job job that throws does' {
        $rust = Start-RustCount 5 -FailAfter 2
        $compiled = Start-Job { 1; 2; throw 'failed after 2 numbers' }
        $null = Wait-Job $rust, $compiled -Timeout 120
        $compiled.State | Should -Be 'Failed'
        $rust.State | Should -Be $compiled.State
        $rust.JobStateInfo.Reason.Message | Should -BeLike '*failed after 2 numbers*'
        $rustErrors = $null
        $compiledErrors = $null
        @(Receive-Job $rust -ErrorAction SilentlyContinue -ErrorVariable rustErrors -WarningAction SilentlyContinue) | Should -Be @(1, 2)
        @(Receive-Job $compiled -ErrorAction SilentlyContinue -ErrorVariable compiledErrors) | Should -Be @(1, 2)
        @($rustErrors).Count | Should -Be @($compiledErrors).Count
        Remove-Job $compiled
    }

    It 'is removed by Receive-Job -AutoRemoveJob, and by Remove-Job -Force while it runs' {
        $ended = Start-RustCount 3 -Name Ended
        $null = Wait-Job $ended -Timeout 30
        @($ended | Receive-Job -Wait -AutoRemoveJob -WarningAction SilentlyContinue) | Should -Be @(1, 2, 3)
        @(Get-Job -Name Ended -ErrorAction SilentlyContinue).Count | Should -Be 0
        @(Start-RustCount 3 -Name Auto | Receive-Job -Wait -AutoRemoveJob -WarningAction SilentlyContinue) | Should -Be @(1, 2, 3)
        @(Get-Job -Name Auto -ErrorAction SilentlyContinue).Count | Should -Be 0
        $running = Start-RustCount 100000 -DelayMs 20 -Name Running
        Start-Sleep -Milliseconds 100
        Remove-Job $running -Force
        @(Get-Job -Name Running -ErrorAction SilentlyContinue).Count | Should -Be 0
        $running.State | Should -Be 'Stopped'
    }

    It 'is removed without an error when it ends while Receive-Job writes what it stored' {
        # The job has stored its one number and still runs when Receive-Job
        # takes it, and it writes nothing more. The command after
        # Receive-Job holds that number until the job has ended, which is
        # the moment Receive-Job removes a job twice.
        $job = Start-RustCount 1 -DelayMs 1500 -Name Held
        $deadline = [DateTime]::UtcNow.AddSeconds(30)
        while ($job.Progress.Count -lt 1 -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 2 }
        $job.State | Should -Be 'Running'
        $id = $job.InstanceId
        $held = $false
        $removalErrors = $null
        $written = @($job | Receive-Job -Wait -AutoRemoveJob -ErrorAction SilentlyContinue -ErrorVariable removalErrors | ForEach-Object {
                if (-not $held) {
                    $held = $true
                    $limit = [DateTime]::UtcNow.AddSeconds(30)
                    while ($job.JobStateInfo.State -eq 'Running' -and [DateTime]::UtcNow -lt $limit) { Start-Sleep -Milliseconds 2 }
                }
                $_
            })
        $job.JobStateInfo.State | Should -Be 'Completed'
        $written | Should -Be @(1)
        @($removalErrors).Count | Should -Be 0
        @(Get-Job -Name Held -ErrorAction SilentlyContinue).Count | Should -Be 0
        # The adapter forgets the job once its end has been announced, on
        # the job's own thread, which may still be at it.
        $limit = [DateTime]::UtcNow.AddSeconds(30)
        while (@(Get-Job -InstanceId $id -ErrorAction SilentlyContinue).Count -gt 0 -and [DateTime]::UtcNow -lt $limit) { Start-Sleep -Milliseconds 2 }
        @(Get-Job -InstanceId $id -ErrorAction SilentlyContinue).Count | Should -Be 0
    }
}

Describe 'a job a cmdlet starts with a JobBody type' {
    AfterEach { Remove-RustJobs }

    It 'runs the type''s work and stores its information records' {
        $job = Start-RustTally 1, 2, 3
        $job.Name | Should -Be 'Tally'
        $null = Wait-Job $job -Timeout 30
        $info = $null
        Receive-Job $job -InformationVariable info | Should -Be 6
        "$($info[0].MessageData)" | Should -Be '3 numbers'
    }
}

Describe 'a cmdlet whose -AsJob it runs itself' {
    AfterEach { Remove-RustJobs }

    It 'writes in place without -AsJob and the same values in a job with it' {
        @(Get-RustSquares 4) | Should -Be @(1, 4, 9, 16)
        $job = Get-RustSquares 4 -AsJob
        $job | Should -BeOfType ([System.Management.Automation.Job])
        $job.Name | Should -Be 'Squares'
        @($job | Receive-Job -Wait -AutoRemoveJob) | Should -Be @(1, 4, 9, 16)
        (Get-Command Get-RustSquares).Parameters['AsJob'].ParameterType | Should -Be ([switch])
    }
}

Describe 'a cmdlet whose -AsJob the runtime runs' {
    AfterEach { Remove-RustJobs }

    It 'runs the invocation in one job, with one running total across the records' {
        1..4 | Add-RustTotal | Should -Be 10
        $jobs = @(1..4 | Add-RustTotal -AsJob)
        $jobs.Count | Should -Be 1
        $jobs[0].PSJobTypeName | Should -Be 'RustJob'
        $info = $null
        @($jobs[0] | Receive-Job -Wait -AutoRemoveJob -InformationVariable info) | Should -Be @(10)
        "$($info[0].MessageData)" | Should -Be '4 numbers'
    }

    It 'makes a record''s error an error of the job, as it is without -AsJob' {
        $inlineErrors = $null
        $inline = @(1, -2, 3 | Add-RustTotal -ErrorAction SilentlyContinue -ErrorVariable inlineErrors)
        $jobErrors = $null
        $job = 1, -2, 3 | Add-RustTotal -AsJob
        $inJob = @($job | Receive-Job -Wait -AutoRemoveJob -ErrorAction SilentlyContinue -ErrorVariable jobErrors)
        $inJob | Should -Be $inline
        $inJob | Should -Be @(4)
        @($jobErrors).Count | Should -Be @($inlineErrors).Count
        "$($jobErrors[0])" | Should -BeLike '*-2 is negative*'
        $job.State | Should -Be 'Completed'
    }

    It 'runs begin and end in the job when no record arrives' {
        @(@() | Add-RustTotal -AsJob | Receive-Job -Wait -AutoRemoveJob) | Should -Be @(0)
    }

    It 'takes a proxy parameter, each record''s own object reaching the job' {
        $first = [Hello.Ledger]::new()
        $second = [Hello.Ledger]::new()
        $first, $second, $first | Add-RustLedgerEntry -Name rent -Number 5 | Should -Be @(5, 5, 10)
        $job = $first, $second, $first | Add-RustLedgerEntry -Name rent -Number 5 -AsJob
        $null = Wait-Job $job -Timeout 30
        $job.State | Should -Be 'Completed'
        @($job | Receive-Job -Wait -AutoRemoveJob) | Should -Be @(15, 10, 20)
        $first.get_Item('rent') | Should -Be 20
        $second.get_Item('rent') | Should -Be 10
    }
}
