# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# The runtime learns, per cmdlet type, which lifecycle phases run the
# trait's default body and stops calling native for them from the
# second instance on. These tests pin both sides of that: a cmdlet
# that implements begin and end keeps getting them on every instance,
# and a process-only cmdlet keeps working across many instances.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'lifecycle phases' {
    It 'runs an implemented begin and end on every instance' {
        foreach ($round in 1..5) {
            (1..4 | Measure-RustTotal) | Should -Be 10
        }
        $records = 5 | Measure-RustTotal -Verbose 4>&1
        $verbose = @($records | Where-Object { $_ -is [System.Management.Automation.VerboseRecord] })
        $verbose.Count | Should -Be 1
        $verbose[0].Message | Should -Be 'begin'
        @($records | Where-Object { $_ -is [long] })[0] | Should -Be 5
    }

    It 'keeps a process-only cmdlet correct across many instances' {
        $out = foreach ($i in 1..200) { Get-Greeting -Name $i }
        @($out).Count | Should -Be 200
        $out[0] | Should -Be 'Hello, 1!'
        $out[199] | Should -Be 'Hello, 200!'
    }

    It 'binds command-line parameters once and pipeline input per record' {
        Get-Greeting -Name once -Count 2 | Should -Be @('Hello, once!', 'Hello, once!')
        (@('p', 'q') | Get-Greeting -Count 1) -join ',' | Should -Be 'Hello, p!,Hello, q!'
    }
}
