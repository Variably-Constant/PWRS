# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# An ErrorRecord reaches a cmdlet two ways, caught in script and
# passed as an argument or handed down the pipeline, and reads the
# same either way. The record used is the engine's own, from Get-Item
# on a path that is not there, so the category, the id and the target
# are what every PowerShell user has seen.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
    $script:absent = Join-Path $TestDrive 'absent'
}

Describe 'reading an ErrorRecord from Rust' {
    It 'reads the category, id and message of a caught error' {
        try { Get-Item -LiteralPath $script:absent -ErrorAction Stop } catch { $record = $_ }
        $info = Get-RustErrorInfo $record
        $info | Should -Match '^ObjectNotFound\|'
        $info | Should -Match 'PathNotFound'
    }

    It 'reads a record handed down the pipeline' {
        $null = Get-Item -LiteralPath $script:absent -ErrorAction SilentlyContinue -ErrorVariable e
        $e[0] | Get-RustErrorInfo | Should -Match '^ObjectNotFound\|'
    }

    It 'carries the target object' {
        try { Get-Item -LiteralPath $script:absent -ErrorAction Stop } catch { $record = $_ }
        (Get-RustErrorInfo $record).Split('|')[3] | Should -Match 'absent'
    }

    It 'reads a record a Rust cmdlet raised' {
        try { [pscustomobject]@{ Name = 'x' } | Get-RustProperty -Name Missing -ErrorAction Stop } catch { $record = $_ }
        Get-RustErrorInfo $record | Should -Match 'Missing'
    }

    It 'refuses an object that is not a record' {
        { Get-RustErrorInfo ([pscustomobject]@{ Nope = 1 }) -ErrorAction Stop } | Should -Throw
    }
}
