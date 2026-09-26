# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A read-only table has to answer both access forms and refuse both
# writes. A wrapper that keeps one form and returns $null for the
# other is the failure worth guarding against, because a script that
# quietly starts reading $null does not announce itself. So every
# read is asserted for a value, never merely for not throwing, and
# every write is judged by reading the value back afterwards rather
# than by whether it raised.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'a read-only table' {
    It 'answers property access' {
        (Get-RustReadOnlyTable).alpha | Should -Be 1
    }

    It 'answers index access' {
        (Get-RustReadOnlyTable)['alpha'] | Should -Be 1
    }

    It 'answers both forms with the same value, for every key' {
        $t = Get-RustReadOnlyTable
        foreach ($pair in @{ alpha = 1; beta = 'two' }.GetEnumerator()) {
            $t.($pair.Key) | Should -Be $pair.Value
            $t[$pair.Key]  | Should -Be $pair.Value
        }
    }

    It 'refuses a write to a nested table, through either form' {
        $t = Get-RustReadOnlyTable
        { $t.inner.n = 99 } | Should -Throw
        { $t['inner']['n'] = 99 } | Should -Throw
        $t.inner.n | Should -Be 0
    }

    It 'keeps a nested table read-only when it is reached by enumerating' {
        $t = Get-RustReadOnlyTable
        $inner = $null
        foreach ($entry in $t.GetEnumerator()) {
            if ($entry.Key -eq 'inner') { $inner = $entry.Value }
        }
        $inner | Should -Not -BeNullOrEmpty
        { $inner['n'] = 99 } | Should -Throw
        $t.inner.n | Should -Be 0
    }

    It 'keeps a nested table read-only when it is reached through Values' {
        $t = Get-RustReadOnlyTable
        $nested = @($t.Values | Where-Object { $_ -is [System.Collections.IDictionary] })
        $nested.Count | Should -Be 1
        { $nested[0]['n'] = 99 } | Should -Throw
    }

    It 'refuses an index write and leaves the value alone' {
        $t = Get-RustReadOnlyTable
        { $t['alpha'] = 99 } | Should -Throw
        $t['alpha'] | Should -Be 1
    }

    It 'refuses a property write and leaves the value alone' {
        $t = Get-RustReadOnlyTable
        { $t.alpha = 99 } | Should -Throw
        $t.alpha | Should -Be 1
    }

    It 'refuses the mutating members' {
        $t = Get-RustReadOnlyTable
        { $t.Add('gamma', 3) } | Should -Throw
        { $t.Remove('alpha') } | Should -Throw
        { $t.Clear() } | Should -Throw
        $t.Count | Should -Be 3
    }

    It 'reports itself read-only and enumerates' {
        $t = Get-RustReadOnlyTable
        $t.IsReadOnly | Should -BeTrue
        ($t.Keys | Sort-Object) -join ',' | Should -Be 'alpha,beta,inner'
        $t.Count | Should -Be 3
    }

    It 'keeps the order of an ordered source, rather than a copy of it' {
        # The source is an OrderedDictionary, which carries the same
        # public non-generic interface the adapter wants. Holding the
        # source rather than copying it is what keeps z,a,m in that
        # order; a Hashtable-backed copy would not.
        #
        # The table is built by the cmdlet rather than here, because
        # the type lives in the module's own load context and the
        # engine's type-name resolver does not look there.
        $t = Get-RustReadOnlyTable -Ordered
        ($t.Keys -join ',') | Should -Be 'z,a,m'
        $t.z | Should -Be 1
        { $t['z'] = 9 } | Should -Throw
    }
}
