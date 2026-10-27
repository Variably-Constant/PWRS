# The operators a class declares through #[psclass] keys: a copied class's
# static methods and a proxy class's instance methods, reached through
# PowerShell's own +, -, *, / and %. PWRS_MODULE points at the built
# module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'operators on a copied class' {
    It 'adds, subtracts, scales and divides vectors' {
        $a = [Hello.Vector]::new(3, 4)
        $b = [Hello.Vector]::new(1, 2)
        $sum = $a + $b
        $sum.GetType().FullName | Should -Be 'Hello.Vector'
        @($sum.X, $sum.Y) | Should -Be @(4, 6)
        $difference = $a - $b
        @($difference.X, $difference.Y) | Should -Be @(2, 2)
        $scaled = $a * 2
        @($scaled.X, $scaled.Y) | Should -Be @(6, 8)
        $halved = $a / 2
        @($halved.X, $halved.Y) | Should -Be @(1.5, 2)
    }

    It 'refuses unary minus, which PowerShell runs as 0 minus the vector' {
        $a = [Hello.Vector]::new(3, 4)
        $e = $null
        try { $null = -$a } catch { $e = $_ }
        $e.Exception.Message | Should -BeLike '*"0"*op_Subtraction*Hello.Vector*'
    }

    It 'leaves the operands as they were' {
        $a = [Hello.Vector]::new(3, 4)
        $null = $a + $a
        @($a.X, $a.Y) | Should -Be @(3, 4)
    }

    It 'declares each operator as the op_ method PowerShell looks for' {
        $names = @([Hello.Vector].GetMethods() | Where-Object { $_.IsSpecialName -and $_.Name -like 'op_*' } | ForEach-Object Name | Sort-Object)
        $names | Should -Be @('op_Addition', 'op_Division', 'op_Multiply', 'op_Subtraction')
    }

    It 'passes on the error its method returns' {
        $a = [Hello.Vector]::new(3, 4)
        { $a / 0 } | Should -Throw '*a vector cannot be divided by 0*'
    }
}

Describe 'operators on a proxy class' {
    It 'adds to a tally and takes a remainder, leaving the tally as it was' {
        $t = [Hello.Tally]::new(10)
        $more = $t + 5
        $more.GetType().FullName | Should -Be 'Hello.Tally'
        $more.Total | Should -Be 15
        $t.Total | Should -Be 10
        $t % 4 | Should -Be 2
        ($t % 4).GetType().FullName | Should -Be 'System.Int64'
    }

    It 'has only the operators it declares' {
        $names = @([Hello.Tally].GetMethods() | Where-Object { $_.IsSpecialName -and $_.Name -like 'op_*' } | ForEach-Object Name | Sort-Object)
        $names | Should -Be @('op_Addition', 'op_Modulus')
        $t = [Hello.Tally]::new(10)
        { $t - 1 } | Should -Throw
    }
}
