# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# #[psmethods] methods that take a PsTask return a Task, settled by a
# Rust thread with a value, an error or a cancellation; the caller's
# CancellationToken reaches the Rust side as a flag.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # Waits for a task that may fault or be canceled, and answers it.
    function Wait-Settled($Task) {
        try { $null = $Task.Wait(10000) } catch { $null = $_ }
        $Task
    }
}

Describe 'A method that takes a PsTask' {
    BeforeAll {
        $script:timer = [Hello.Timer]::new()
    }

    It 'returns a Task of its value that a Rust thread completes' {
        $task = $timer.WaitAsync(50)
        $task -is [System.Threading.Tasks.Task[long]] | Should -BeTrue
        $task.Wait(10000) | Should -BeTrue
        $task.Result | Should -Be 50
        $task.Status | Should -Be 'RanToCompletion'
    }

    It 'returns before the work is done' {
        $task = $timer.WaitAsync(2000)
        $task.IsCompleted | Should -BeFalse
        $task.Wait(10000) | Should -BeTrue
    }

    It 'answers through GetAwaiter().GetResult() and from a static' {
        [Hello.Timer]::SumAsync(2, 3).GetAwaiter().GetResult() | Should -Be 5
    }

    It 'ends canceled when the token asks while it runs' {
        $source = [System.Threading.CancellationTokenSource]::new()
        $clock = [System.Diagnostics.Stopwatch]::StartNew()
        $task = $timer.WaitAsync(10000, $source.Token)
        $source.Cancel()
        (Wait-Settled $task).Status | Should -Be 'Canceled'
        $clock.ElapsedMilliseconds | Should -BeLessThan 5000
    }

    It 'ends canceled when the token has asked before the call' {
        $source = [System.Threading.CancellationTokenSource]::new()
        $source.Cancel()
        (Wait-Settled $timer.WaitAsync(10000, $source.Token)).Status | Should -Be 'Canceled'
    }

    It 'faults with the error the module raises' {
        $task = Wait-Settled $timer.WaitAsync(-1)
        $task.Status | Should -Be 'Faulted'
        $task.Exception.InnerException.Message | Should -Match 'HelloWait.*cannot wait -1 ms'
        $task = Wait-Settled ([Hello.Timer]::SumAsync([long]::MaxValue, 1))
        $task.Exception.InnerException.Message | Should -Match 'overflows'
    }

    It 'faults rather than waiting forever when the module drops it unsettled' {
        $task = Wait-Settled $timer.ForgetAsync()
        $task.Status | Should -Be 'Faulted'
        $task.Exception.InnerException.Message | Should -Match 'PwrsTaskDropped'
    }

    It 'returns a Task of no value for PsTask<()>' {
        $task = $timer.PingAsync()
        $task -is [System.Threading.Tasks.Task] | Should -BeTrue
        $task.Wait(10000) | Should -BeTrue
        $task.Status | Should -Be 'RanToCompletion'
    }

    It 'declares the Task type and the optional token on the method' {
        $method = [Hello.Timer].GetMethod('WaitAsync')
        $method.ReturnType | Should -Be ([System.Threading.Tasks.Task[long]])
        $token = $method.GetParameters()[-1]
        $token.ParameterType | Should -Be ([System.Threading.CancellationToken])
        $token.IsOptional | Should -BeTrue
    }

    It 'keeps the method as a method of the object, which ran once per call' {
        $before = $timer.Started
        $null = $timer.WaitAsync(1).Wait(10000)
        $timer.Started | Should -Be ($before + 1)
    }
}
