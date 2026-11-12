# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A Rust error that names its .NET exception type reaches PowerShell as
# that type on each path it leaves by, so a catch of the type catches
# it: a cmdlet's record, written or terminating, keeps its id, category
# and target with the named exception as its own; a binder refusal is
# the binder's exception with the named one inside it; a job writes it
# and fails with it. A name that gives no exception type is
# Pwrs.PwrsException, its message saying why. The oracle for a cmdlet's
# record is Get-Item on a missing path, a compiled cmdlet whose record
# has an ItemNotFoundException, run under the same switches.
# Bases.Tests.ps1 covers a proxy member's error, and Lifecycle.Tests.ps1
# a lifecycle hook's.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
    $script:missing = Join-Path $TestDrive 'nothing-here'
    $script:notFound = 'System.Management.Automation.ItemNotFoundException'

    # The first exception of type $type in $e's chain of inner exceptions,
    # $e itself included.
    function Find-Inner([Exception] $e, [type] $type) {
        while ($e) {
            if ($e -is $type) { return $e }
            $e = $e.InnerException
        }
        $null
    }
}

Describe 'a cmdlet error that names its exception' {
    It 'is a written record whose exception is that type, as a compiled cmdlet''s is' {
        $compiled = $null
        Get-Item -LiteralPath $missing -ErrorAction SilentlyContinue -ErrorVariable compiled
        $err = $null
        Get-Greeting -Name x -Fail -Exception $notFound -ErrorAction SilentlyContinue -ErrorVariable err
        @($err).Count | Should -Be @($compiled).Count
        $err[0].Exception.GetType() | Should -Be $compiled[0].Exception.GetType()
        $err[0].Exception.Message | Should -BeExactly 'refusing to greet x'
        $err[0].Exception.InnerException.Message | Should -BeExactly 'the greeting to x was refused because -Fail was given'
        $err[0].Exception.Data['PwrsErrorId'] | Should -Be 'GreetingRefused'
        $err[0].FullyQualifiedErrorId | Should -BeLike 'GreetingRefused,*'
        $err[0].CategoryInfo.Category | Should -Be 'InvalidData'
    }

    It 'is caught by its type under -ErrorAction Stop, as a compiled cmdlet''s is' {
        # ItemNotFoundException carries its own record, which is what a catch
        # of its type reports: for Get-Item, the id PathNotFound with no
        # command after it.
        $compiled = $null
        try { Get-Item -LiteralPath $missing -ErrorAction Stop } catch [System.Management.Automation.ItemNotFoundException] { $compiled = $_ }
        $compiled | Should -Not -BeNullOrEmpty
        $compiled.FullyQualifiedErrorId | Should -Be 'PathNotFound'
        $stopped = $null
        try { Get-Greeting -Name x -Fail -Exception $notFound -ErrorAction Stop } catch [System.Management.Automation.ItemNotFoundException] { $stopped = $_ }
        $stopped | Should -Not -BeNullOrEmpty
        $stopped.Exception.GetType() | Should -Be $compiled.Exception.GetType()
        $stopped.FullyQualifiedErrorId | Should -Be 'GreetingRefused'
        $stopped.CategoryInfo.Category | Should -Be 'InvalidData'
        $plain = $null
        try { Get-Greeting -Name x -Fail -Exception System.InvalidOperationException -ErrorAction Stop } catch [System.InvalidOperationException] { $plain = $_ }
        $plain.FullyQualifiedErrorId | Should -BeLike 'GreetingRefused,*'
        $plain.CategoryInfo.Category | Should -Be 'InvalidData'
    }

    It 'is caught by its type when terminating' {
        $terminated = $null
        try { Get-Greeting -Name x -Terminate -Exception System.IO.FileNotFoundException } catch [System.IO.FileNotFoundException] { $terminated = $_ }
        $terminated.Exception | Should -BeOfType [System.IO.FileNotFoundException]
        $terminated.Exception.InnerException.Message | Should -BeExactly 'the greeting to x was refused because -Terminate was given'
        $terminated.FullyQualifiedErrorId | Should -BeLike 'GreetingRefused,*'
        $terminated.CategoryInfo.Category | Should -Be 'InvalidData'
    }

    It 'is PwrsException, saying why, for a name that gives no exception type' {
        $err = $null
        Get-Greeting -Name x -Fail -Exception No.Such.Type -ErrorAction SilentlyContinue -ErrorVariable err
        $err[0].Exception.GetType().FullName | Should -Be 'Pwrs.PwrsException'
        $err[0].Exception.Message | Should -BeExactly 'refusing to greet x (the error names the exception type No.Such.Type, which could not be made: no type is named No.Such.Type)'
        $err[0].FullyQualifiedErrorId | Should -BeLike 'GreetingRefused,*'
        $err = $null
        Get-Greeting -Name x -Fail -Exception System.String -ErrorAction SilentlyContinue -ErrorVariable err
        $err[0].Exception.Message | Should -BeExactly 'refusing to greet x (the error names the exception type System.String, which could not be made: System.String is not an exception type)'
    }
}

Describe 'a binder refusal whose error names its exception' {
    It 'has the named exception inside the binder''s own for a validate_with function' {
        $refusal = $null
        try { Split-RustEven -Number 3 } catch { $refusal = $_ }
        $refusal.Exception | Should -BeOfType [System.Management.Automation.ParameterBindingValidationException]
        $named = Find-Inner $refusal.Exception ([System.ArgumentException])
        $named | Should -Not -BeNullOrEmpty
        $named.Data['PwrsErrorId'] | Should -Be 'HelloOddNumber'
    }

    It 'has the named exception inside the binder''s own for a transform' {
        $refusal = $null
        try { Get-RustSize -Size xMB } catch { $refusal = $_ }
        $refusal.Exception | Should -BeOfType [System.Management.Automation.ParameterBindingArgumentTransformationException]
        (Find-Inner $refusal.Exception ([System.FormatException])).Data['PwrsErrorId'] | Should -Be 'HelloSize'
        $refusal = $null
        try { Get-RustSize -Size 99999999999GB } catch { $refusal = $_ }
        Find-Inner $refusal.Exception ([System.OverflowException]) | Should -Not -BeNullOrEmpty
    }
}

Describe 'a job whose error names its exception' {
    It 'writes a record with that exception and fails with it as the reason' {
        $job = Start-RustCount 5 -FailAfter 2 -Exception System.TimeoutException
        $null = Wait-Job $job -Timeout 120
        $job.State | Should -Be 'Failed'
        $job.JobStateInfo.Reason | Should -BeOfType [System.TimeoutException]
        $job.JobStateInfo.Reason.Message | Should -BeExactly '[HelloCountFailed] failed after 2 numbers'
        $job.JobStateInfo.Reason.Data['PwrsErrorId'] | Should -Be 'HelloCountFailed'
        $failed = $null
        @(Receive-Job $job -ErrorAction SilentlyContinue -ErrorVariable failed -WarningAction SilentlyContinue) | Should -Be @(1, 2)
        $written = @($failed | Where-Object { $_.FullyQualifiedErrorId -like 'HelloCountFailed*' })
        $written.Count | Should -Be 1
        $written[0].Exception | Should -BeOfType [System.TimeoutException]
        $written[0].Exception.Message | Should -BeExactly 'failed after 2 numbers'
        $written[0].CategoryInfo.Category | Should -Be 'InvalidResult'
        Remove-Job $job
    }
}
