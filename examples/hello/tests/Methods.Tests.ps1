# Rust methods called on a proxy object through #[psmethods].
# PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'methods on proxy Hello.Counter' {
    It 'runs a mutating method and reads the new state through the properties' {
        $c = New-Counter -Label ticks -Value 1
        $c.Advance(5) | Should -Be 6
        $c.Advance(5).GetType().Name | Should -Be 'Int64'
        $c.Value | Should -Be 11
        $c.History | Should -Be @(6, 11)
    }

    It 'takes an optional argument, or omits it' {
        $c = New-Counter -Label t -Value 2
        $c.Describe('n') | Should -Be 'n=2'
        $c.Describe('') | Should -Be '=2'
        $c.Describe() | Should -Be 't=2'
        $c.Describe($null) | Should -Be 't=2'
    }

    It 'returns nothing from a method returning PsResult<()>' {
        $c = New-Counter -Label t -Value 9
        $null = $c.Advance(1)
        $c.Reset()
        $c.Value | Should -Be 0
        @($c.History).Count | Should -Be 0
    }

    It 'surfaces a Rust error as an exception' {
        $c = New-Counter -Label t -Value ([long]::MaxValue)
        { $c.Advance(1) } | Should -Throw
        $c.Value | Should -Be ([long]::MaxValue)
    }

    It 'refuses a call after Dispose' {
        $c = New-Counter -Label t -Value 1
        $c.Dispose()
        { $c.Advance(1) } | Should -Throw
    }

    It 'lists the methods through Get-Member' {
        $names = (New-Counter -Label t | Get-Member -MemberType Method).Name
        $names | Should -Contain 'Advance'
        $names | Should -Contain 'Describe'
        $names | Should -Contain 'Reset'
    }

    It 'returns a new proxy object from a method' {
        $c = New-Counter -Label whole -Value 10
        $half = $c.Split('part')
        $half.GetType().FullName | Should -Be 'Hello.Counter'
        $half.Label | Should -Be 'part'
        $half.Value | Should -Be 5
        $c.Value | Should -Be 5
        $half.Advance(1) | Should -Be 6
        $half.Dispose()
    }

    It 'takes a proxy object as a method argument' {
        $a = New-Counter -Label a -Value 3
        $b = New-Counter -Label b -Value 4
        $a.Absorb($b) | Should -Be 7
        $b.Value | Should -Be 4
    }

    It 'takes its own receiver as the by-value argument of a &self method' {
        $c = New-Counter -Label c -Value 3
        $d = New-Counter -Label d -Value 4
        $c.SameAs($c) | Should -BeTrue
        $c.SameAs($d) | Should -BeFalse
        $c.Value | Should -Be 3
    }

    It 'refuses its own receiver as the by-value argument of a &mut self method' {
        $c = New-Counter -Label c -Value 3
        { $c.Absorb($c) } | Should -Throw '*in use by a call already running on this thread*'
        $c.Value | Should -Be 3
        $c.Advance(1) | Should -Be 4
    }
}

Describe 'methods whose names collide with the proxy base' {
    It 'reads properties correctly on a class declaring Get and Call' {
        $s = New-RustSlots -Origin disk -Capacity 3
        $s.Origin | Should -Be 'disk'
        $s.Origin.GetType().Name | Should -Be 'String'
        $s.Capacity | Should -Be 3
        $s.Capacity.GetType().Name | Should -Be 'UInt64'
    }

    It 'runs Get, Set, Contains and Call as the module declared them' {
        $s = New-RustSlots -Origin mem -Capacity 3
        $s.Set(1, 42) | Should -Be 0
        $s.Get(1) | Should -Be 42
        $s.Contains(42) | Should -BeTrue
        $s.Contains(7) | Should -BeFalse
        $s.Call('sum') | Should -Be 42
        $s.Call('count') | Should -Be 3
    }

    It 'still disposes and reports disposal' {
        $s = New-RustSlots -Origin mem -Capacity 1
        $s.IsDisposed | Should -BeFalse
        $s.Dispose()
        $s.IsDisposed | Should -BeTrue
        { $s.Get(0) } | Should -Throw
    }

    It 'surfaces a Rust error from a colliding method name' {
        $s = New-RustSlots -Origin mem -Capacity 1
        { $s.Get(9) } | Should -Throw
        { $s.Call('nope') } | Should -Throw
    }
}
