# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Get-RustInvocation reads its PipelinePosition and PipelineLength
# from Pipeline::invocation in begin, passes its input through, and
# writes its place at end, so a chain of them reports every position.
# It reads in begin and nowhere else, so every case here also shows
# the accessor answering before any input arrives.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'where a cmdlet stands in its pipeline' {
    It 'is 1 of 1 alone' {
        # Assigned, not piped to Should: Should is a command, so piping
        # to it would make this 1 of 2.
        $place = Get-RustInvocation
        $place | Should -Be '1/1'
    }

    It 'counts the command it is piped into' {
        $place = @(Get-RustInvocation | ForEach-Object { $_ })
        $place | Should -Be '1/2'
    }

    It 'is each position of a chain of itself, counting from 1' {
        $places = @(Get-RustInvocation | Get-RustInvocation | Get-RustInvocation) | Sort-Object
        $places | Should -Be @('1/3', '2/3', '3/3')
    }

    It 'counts the commands around it that are not its own' {
        $out = @(Write-Output 'a', 'b' | ForEach-Object { $_ } | Get-RustInvocation)
        $out[-1] | Should -Be '3/3'
        $out[0..1] | Should -Be @('a', 'b')
    }

    It 'does not count an expression at the head, which is input and not a command' {
        $out = @('a', 'b' | Get-RustInvocation)
        $out[-1] | Should -Be '1/1'
        $out[0..1] | Should -Be @('a', 'b')
    }
}
