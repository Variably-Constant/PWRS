# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A PWRS job is a RustJob: its work runs on a thread of its own, and the
# job cmdlets see it as they see any job. Where PowerShell's own jobs
# show the expected behavior, the case runs Start-Job beside it, the same
# way: a job whose
# work fails is Failed with the reason, and Receive-Job reports it alike.
#
# Receive-Job -Wait removes a job twice, and reports the second removal as
# ReceiveJobAutoRemovalError, when the job ends after Receive-Job starts
# waiting on it and before Receive-Job has written what the job has already
# stored. It does so to every kind of job in both hosts. So a case that
# pipes a job to Receive-Job -AutoRemoveJob waits for the job to end
# first, or gives it work lasting well past that moment.
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
        @(Start-RustCount 3 -DelayMs 200 -Name Auto | Receive-Job -Wait -AutoRemoveJob -WarningAction SilentlyContinue) | Should -Be @(1, 2, 3)
        @(Get-Job -Name Auto -ErrorAction SilentlyContinue).Count | Should -Be 0
        $running = Start-RustCount 100000 -DelayMs 20 -Name Running
        Start-Sleep -Milliseconds 100
        Remove-Job $running -Force
        @(Get-Job -Name Running -ErrorAction SilentlyContinue).Count | Should -Be 0
        $running.State | Should -Be 'Stopped'
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
        $null = Wait-Job $job -Timeout 30
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
        $null = Wait-Job $jobs[0] -Timeout 30
        $info = $null
        @($jobs[0] | Receive-Job -Wait -AutoRemoveJob -InformationVariable info) | Should -Be @(10)
        "$($info[0].MessageData)" | Should -Be '4 numbers'
    }

    It 'makes a record''s error an error of the job, as it is without -AsJob' {
        $inlineErrors = $null
        $inline = @(1, -2, 3 | Add-RustTotal -ErrorAction SilentlyContinue -ErrorVariable inlineErrors)
        $jobErrors = $null
        $job = 1, -2, 3 | Add-RustTotal -AsJob
        $null = Wait-Job $job -Timeout 30
        $inJob =@($job | Receive-Job -Wait -AutoRemoveJob -ErrorAction SilentlyContinue -ErrorVariable jobErrors)
        $inJob | Should -Be $inline
        $inJob | Should -Be @(4)
        @($jobErrors).Count | Should -Be @($inlineErrors).Count
        "$($jobErrors[0])" | Should -BeLike '*-2 is negative*'
        $job.State | Should -Be 'Completed'
    }

    It 'runs begin and end in the job when no record arrives' {
        $job = @() | Add-RustTotal -AsJob
        $null = Wait-Job $job -Timeout 30
        @($job | Receive-Job -Wait -AutoRemoveJob) | Should -Be @(0)
    }
}
