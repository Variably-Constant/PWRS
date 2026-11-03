# The operators a class declares through #[psclass] keys: a copied class's
# static methods and a proxy class's instance methods, reached through
# PowerShell's own +, -, *, / and %; conversion operators, which casts and
# the parameter binder call; and a PSTypeConverter backed by two Rust
# functions. PWRS_MODULE points at the built module folder.
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

Describe 'conversion operators' {
    It 'converts an int to a fraction implicitly, and a fraction to a double explicitly' {
        $half = [Hello.Fraction]::new(1, 2)
        [double]$half | Should -Be 0.5
        ([double]$half) | Should -BeOfType [double]
        $five = [Hello.Fraction]5
        "$($five.Numerator)/$($five.Denominator)" | Should -Be '5/1'
        $names = [Hello.Fraction].GetMethods() | Where-Object Name -in 'op_Implicit', 'op_Explicit' | ForEach-Object Name | Sort-Object
        $names | Should -Be @('op_Explicit', 'op_Implicit')
    }

    It 'binds a cmdlet parameter of the class from an int through the implicit conversion' {
        Get-RustFractionValue -Fraction 3 | Should -Be 3
        Get-RustFractionValue -Fraction ([Hello.Fraction]::new(3, 4)) | Should -Be 0.75
    }

    It 'is found by the value''s exact type, so a long reaches no int conversion' {
        { [Hello.Fraction]5L } | Should -Throw '*Cannot convert*'
    }
}

Describe 'a class converting through its PSTypeConverter' {
    It 'converts text and numbers to the class' {
        [math]::Round(([Hello.Temperature]'70F').Celsius, 3) | Should -Be 21.111
        ([Hello.Temperature]'21.5C').Celsius | Should -Be 21.5
        ([Hello.Temperature]25).Celsius | Should -Be 25
    }

    It 'converts the class to a double and an int' {
        $warm = [Hello.Temperature]'21.5C'
        [double]$warm | Should -Be 21.5
        [int]$warm | Should -Be 22
    }

    It 'binds a cmdlet parameter of the class from text and from a number through the converter' {
        [math]::Round((Get-RustCelsius -Temperature '212F'), 3) | Should -Be 100
        Get-RustCelsius -Temperature 30 | Should -Be 30
    }

    It 'calls its from function once for each conversion, storing the result CanConvertFrom got' {
        $before = [Hello.Temperature]::FromAnyCalls()
        $null = [Hello.Temperature]'10C'
        [Hello.Temperature]::FromAnyCalls() - $before | Should -Be 1
    }

    It 'refuses a value its converter cannot convert' {
        { [Hello.Temperature]'warm' } | Should -Throw '*Hello.Temperature*'
        { [System.Guid][Hello.Temperature]'1C' } | Should -Throw
    }

    It 'names its converter with [TypeConverter]' {
        $attribute = [Hello.Temperature].GetCustomAttributes([System.ComponentModel.TypeConverterAttribute], $false)
        $attribute.ConverterTypeName | Should -BeLike 'Hello.Temperature+PwrsConverter*'
    }
}
