# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# An error with details still has the one-line message the default view
# shows, with the details in its exception's InnerException, on
# each path a Rust error leaves by: a cmdlet's record, written or
# terminating, and the exception a proxy method, a static method or a
# constructor throws, which PowerShell wraps in a
# MethodInvocationException.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'an error with details' {
    It 'leaves the details out of a written record''s default view' {
        $err = $null
        Get-Greeting -Name x -Fail -ErrorAction SilentlyContinue -ErrorVariable err
        @($err).Count | Should -Be 1
        $err[0].Exception.Message | Should -BeExactly 'refusing to greet x'
        $err[0].Exception.InnerException.Message | Should -BeExactly 'the greeting to x was refused because -Fail was given'
        $shown = $err[0] | Out-String
        $shown | Should -Match 'refusing to greet x'
        $shown | Should -Not -Match 'was refused because'
    }

    It 'has the details on a terminating record' {
        $record = $null
        try { Get-Greeting -Name x -Terminate } catch { $record = $_ }
        $record.FullyQualifiedErrorId | Should -Match '^GreetingRefused'
        $record.Exception.Message | Should -BeExactly 'refusing to greet x'
        $record.Exception.InnerException.Message | Should -BeExactly 'the greeting to x was refused because -Terminate was given'
    }

    It 'has the details inside the MethodInvocationException of a proxy method' {
        $counter = [Hello.Counter]::new('c', [long]::MaxValue)
        $record = $null
        try { $counter.Advance(1) } catch { $record = $_ }
        $record.Exception | Should -BeOfType ([System.Management.Automation.MethodInvocationException])
        $record.Exception.InnerException.Message | Should -BeExactly '[CounterOverflow] the counter would overflow'
        $record.Exception.InnerException.InnerException.Message | Should -BeExactly '9223372036854775807 plus 1 is past 9223372036854775807'
    }

    It 'has the details of a static method''s error' {
        $record = $null
        try { [Hello.Counter]::Parse('c=x') } catch { $record = $_ }
        $record.Exception.InnerException.Message | Should -Match '^\[CounterParse\] x is not a number'
        $record.Exception.InnerException.InnerException.Message | Should -Match 'InvalidDigit'
    }

    It 'leaves an error without details as it was' {
        $record = $null
        try { [Hello.Counter]::new('a=b', 0) } catch { $record = $_ }
        $record.Exception.InnerException.Message | Should -Match '^\[CounterLabel\]'
        $record.Exception.InnerException.InnerException | Should -BeNullOrEmpty
    }
}
