# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Proxy classes whose #[psclass] names the methods behind .NET
# interfaces: a list through count and item (and set_item), a stream
# through next, and an order and an equality through compare, equals
# and hash. PowerShell's own operators and commands reach each through
# the interface.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'A class that is a list through count and item' {
    BeforeAll {
        $script:steps = [Hello.Steps]::new(10, 5, 4)
    }

    It 'answers Count and its indexer' {
        $steps.Count | Should -Be 4
        $steps[0] | Should -Be 10
        $steps[3] | Should -Be 25
    }

    It 'enumerates in a foreach and down the pipeline' {
        $doubled = foreach ($x in $steps) { $x * 2 }
        $doubled -join ',' | Should -Be '20,30,40,50'
        ($steps | Measure-Object -Sum).Sum | Should -Be 70
        @($steps).Count | Should -Be 4
    }

    It 'is an IReadOnlyList of its element type and no IList' {
        $steps -is [System.Collections.Generic.IReadOnlyList[long]] | Should -BeTrue
        $steps -is [System.Collections.Generic.IList[long]] | Should -BeFalse
    }

    It 'answers -contains through its elements' {
        $steps -contains 20 | Should -BeTrue
        $steps -contains 21 | Should -BeFalse
    }

    It 'refuses an index outside it with the error the Rust method raises' {
        { $steps.At(9) } | Should -Throw '*index 9 is past the 4 elements*'
    }
}

Describe 'A list that names set_item' {
    It 'writes an element through its indexer' {
        $cells = [Hello.Cells]::new(3)
        $cells[1] = 7
        $cells[1] | Should -Be 7
        @($cells) -join ',' | Should -Be '0,7,0'
    }

    It 'sorts its elements' {
        $cells = [Hello.Cells]::new(3)
        $cells[2] = 9
        $cells[0] = 4
        @($cells | Sort-Object -Descending) -join ',' | Should -Be '9,4,0'
    }

    It 'is an IList of a fixed size, read-only as an array is through ICollection' {
        $cells = [Hello.Cells]::new(2)
        $cells[1] = 5
        $cells -is [System.Collections.Generic.IList[long]] | Should -BeTrue
        [System.Collections.Generic.ICollection[long]].GetProperty('IsReadOnly').GetValue($cells) | Should -BeTrue
        [System.Collections.Generic.IList[long]].GetMethod('IndexOf').Invoke($cells, @([long]5)) | Should -Be 1
        $e = { [System.Collections.Generic.ICollection[long]].GetMethod('Add').Invoke($cells, @([long]1)) } | Should -Throw -PassThru
        $inner = $e.Exception
        while ($null -ne $inner.InnerException) { $inner = $inner.InnerException }
        $inner | Should -BeOfType ([System.NotSupportedException])
        $inner.Message | Should -Match 'fixed size'
    }
}

Describe 'A class that is a stream through next' {
    It 'enumerates its elements once' {
        $countdown = [Hello.Countdown]::new(3)
        @($countdown) -join ',' | Should -Be '3,2,1'
        @($countdown).Count | Should -Be 0
        $countdown.Left | Should -Be 0
    }

    It 'is an IEnumerable of its element type and no list' {
        $countdown = [Hello.Countdown]::new(1)
        $countdown -is [System.Collections.Generic.IEnumerable[long]] | Should -BeTrue
        $countdown -is [System.Collections.Generic.IReadOnlyList[long]] | Should -BeFalse
    }
}

Describe 'A class ordered and compared through compare, equals and hash' {
    BeforeAll {
        $script:a = [Hello.Release]::new(1, 2, 3)
        $script:same = [Hello.Release]::new(1, 2, 3)
        $script:b = [Hello.Release]::new(1, 10, 0)
    }

    It 'orders by value under -lt, -gt, -le and -ge' {
        $a -lt $b | Should -BeTrue
        $b -gt $a | Should -BeTrue
        $a -le $same | Should -BeTrue
        $a -ge $b | Should -BeFalse
    }

    It 'is equal by value under -eq and -ne, as two objects' {
        [object]::ReferenceEquals($a, $same) | Should -BeFalse
        $a -eq $same | Should -BeTrue
        $a -ne $b | Should -BeTrue
        $a.GetHashCode() | Should -Be $same.GetHashCode()
    }

    It 'sorts by value' {
        @($b, $a) | Sort-Object | ForEach-Object Minor | Should -Be @(2, 10)
        @($a, $b) | Sort-Object -Descending | ForEach-Object Minor | Should -Be @(10, 2)
    }

    It 'keys a hashtable by value' {
        $table = @{}
        $table[$a] = 'first'
        $table[$same] | Should -Be 'first'
        $table.ContainsKey($b) | Should -BeFalse
    }

    It 'collapses equal values in Select-Object -Unique, Sort-Object -Unique and Group-Object' {
        @($a, $same, $b | Select-Object -Unique).Count | Should -Be 2
        @($a, $same, $b | Sort-Object -Unique).Count | Should -Be 2
        @($a, $same, $b | Group-Object | ForEach-Object Count) | Should -Be @(2, 1)
    }

    It 'answers -contains by value' {
        @($b, $same) -contains $a | Should -BeTrue
    }

    It 'passes another object of the class to a method that takes &Self' {
        $a.Order($b) | Should -Be -1
        $b.Order($a) | Should -Be 1
        $a.Order($a) | Should -Be 0
        $e = { $a.Order($null) } | Should -Throw -PassThru
        $e.Exception.InnerException | Should -BeOfType ([System.ArgumentNullException])
    }

    It 'refuses to order against another type' {
        $e = { ([System.IComparable]$a).CompareTo('1.2.3') } | Should -Throw -PassThru
        $e.Exception.InnerException | Should -BeOfType ([System.ArgumentException])
    }

    It 'lets two threads order the same pair the opposite way round at once' {
        $first = [Hello.Release]::new(1, 0, 0)
        $second = [Hello.Release]::new(2, 0, 0)
        $loop = { param($x, $y) $sum = 0; for ($i = 0; $i -lt 2000; $i++) { $sum += $x.Order($y) }; $sum }
        $one = [powershell]::Create().AddScript($loop).AddArgument($first).AddArgument($second)
        $two = [powershell]::Create().AddScript($loop).AddArgument($second).AddArgument($first)
        try {
            $r1 = $one.BeginInvoke()
            $r2 = $two.BeginInvoke()
            $r1.AsyncWaitHandle.WaitOne(60000) | Should -BeTrue
            $r2.AsyncWaitHandle.WaitOne(60000) | Should -BeTrue
            $one.EndInvoke($r1) | Should -Be -2000
            $two.EndInvoke($r2) | Should -Be 2000
        } finally {
            $one.Dispose()
            $two.Dispose()
        }
    }
}
