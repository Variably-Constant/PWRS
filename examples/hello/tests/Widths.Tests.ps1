# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# These assert against the real CLR, which is the only place the
# claims can be checked: the fake host in convert_tests has no type
# system, so a width, a Decimal's field order and a DateTimeOffset's
# offset are all things only a loaded module can prove.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'a scalar keeps its CLR width' {
    It 'writes each width as its own type' {
        # The engine types an operator's answer by its operands'
        # widths, so a value that left script an Int32 and returns an
        # Int64 changes what the caller's next operator does.
        $names = @(Get-RustWidths | ForEach-Object { $_.GetType().Name })
        $names | Should -Be @(
            'SByte', 'Int16', 'Int32', 'Byte', 'UInt16', 'UInt32',
            'Single', 'Int64', 'Double'
        )
    }

    It 'keeps a Single from being read as a widened Double' {
        # 7.5 is exact in both, so this is the width and not rounding.
        $single = @(Get-RustWidths)[6]
        $single | Should -BeOfType [single]
        $single | Should -Be ([single]7.5)
    }

    It 'gives an arithmetic result the width PowerShell gives it' {
        $int32 = @(Get-RustWidths)[2]
        ($int32 + 1).GetType().Name | Should -Be 'Int32'
    }
}

Describe 'the type tag' {
    It 'names the tag of each type it knows' {
        Get-RustTypeTag -InputObject ([int]1) | Should -Be 4
        Get-RustTypeTag -InputObject ([long]1) | Should -Be 5
        Get-RustTypeTag -InputObject ([byte]1) | Should -Be 6
        Get-RustTypeTag -InputObject ([double]1) | Should -Be 11
        Get-RustTypeTag -InputObject 'text' | Should -Be 12
        Get-RustTypeTag -InputObject ([decimal]1) | Should -Be 17
    }

    It 'answers object for a type outside the vocabulary rather than guessing' {
        Get-RustTypeTag -InputObject ([pscustomobject]@{ a = 1 }) | Should -Be 0
    }

    It 'sees through the PSObject the engine wraps a value in' {
        $wrapped = [psobject]::AsPSObject([int]7)
        Get-RustTypeTag -InputObject $wrapped | Should -Be 4
    }
}

Describe 'a decimal' {
    It 'round-trips its value and its scale' {
        Get-RustDecimalRoundTrip -Value ([decimal]'123.456') | Should -Be ([decimal]'123.456')
        Get-RustDecimalRoundTrip -Value ([decimal]'123.456') -Scale | Should -Be 3
    }

    It 'keeps trailing zeros, which are scale and not value' {
        # 1.10 and 1.1 are equal but not identical: the scale differs,
        # and a round trip that lost it would still compare equal.
        # The d literal is used because a string cast normalizes.
        Get-RustDecimalRoundTrip -Value 1.10d -Scale | Should -Be 2
        Get-RustDecimalRoundTrip -Value 1.1d -Scale | Should -Be 1
    }

    It 'reports whatever scale the value arrived with, and does not pick one' {
        # A string cast does not agree across hosts: pwsh 7 makes
        # [decimal]'1.10' scale 1, Windows PowerShell 5.1 makes it 2.
        # So the host is asked what it produced and the round trip is
        # held to returning that, rather than either answer being
        # written in here. Parse and the d literal keep 2 on both.
        $cast = [decimal]'1.10'
        $arrived = [decimal]::GetBits($cast)[3] -shr 16 -band 0xFF
        Get-RustDecimalRoundTrip -Value $cast -Scale | Should -Be $arrived
        Get-RustDecimalRoundTrip -Value ([decimal]::Parse('1.10')) -Scale | Should -Be 2
    }

    It 'round-trips the extremes and a negative' {
        Get-RustDecimalRoundTrip -Value ([decimal]::MaxValue) | Should -Be ([decimal]::MaxValue)
        Get-RustDecimalRoundTrip -Value ([decimal]::MinValue) | Should -Be ([decimal]::MinValue)
        Get-RustDecimalRoundTrip -Value ([decimal]'-12345.6789') | Should -Be ([decimal]'-12345.6789')
    }

    It 'comes back as a Decimal and not something widened' {
        (Get-RustDecimalRoundTrip -Value ([decimal]'1.5')).GetType().Name | Should -Be 'Decimal'
    }
}

Describe 'a pinned Decimal array' {
    It 'sums a block whose elements share one scale' {
        # This is the claim the whole tag exists for: the array is
        # read as one block through a pin rather than per object.
        # d literals, because a string cast normalizes the scale and
        # [decimal[]]@('1.50','2.25') would arrive as scales 1 and 2.
        $values = [decimal[]]@(1.50d, 2.25d, 3.25d)
        Measure-RustDecimalBlock -Value $values | Should -Be 7.00d
    }

    It 'sums a block carrying negatives' {
        $values = [decimal[]]@(5.00d, -2.00d)
        Measure-RustDecimalBlock -Value $values | Should -Be 3.00d
    }

    It 'refuses a block whose scales differ rather than adding words that do not line up' {
        { Measure-RustDecimalBlock -Value ([decimal[]]@(1.5d, 2.25d)) -ErrorAction Stop } |
            Should -Throw
    }

    It 'refuses an array of the wrong element type' {
        { Measure-RustDecimalBlock -Value ([double[]]@(1.5, 2.5)) -ErrorAction Stop } |
            Should -Throw
    }
}

Describe 'a DateTimeOffset' {
    It 'round-trips an instant with its offset' {
        $v = [datetimeoffset]::new(2026, 9, 22, 13, 45, 0, [timespan]::FromMinutes(-330))
        $back = Get-RustOffset -Value $v
        $back | Should -BeOfType [datetimeoffset]
        $back.UtcTicks | Should -Be $v.UtcTicks
        $back.Offset | Should -Be $v.Offset
    }

    It 'reads the offset in whole minutes, including a half-hour zone' {
        $v = [datetimeoffset]::new(2026, 9, 22, 13, 45, 0, [timespan]::FromMinutes(330))
        Get-RustOffset -Value $v -Minutes | Should -Be 330
    }

    It 'converts to the UTC clock by taking the offset off' {
        $v = [datetimeoffset]::new(2026, 9, 22, 13, 45, 0, [timespan]::FromMinutes(-330))
        Get-RustOffset -Value $v -UtcTicks | Should -Be $v.UtcDateTime.Ticks
    }

    It 'keeps UTC distinct from the same wall clock at an offset' {
        # A DateTime carries only which clock it belongs to; an offset
        # names the displacement, so these two are different instants.
        $utc = [datetimeoffset]::new(2026, 9, 22, 12, 0, 0, [timespan]::Zero)
        $plus = [datetimeoffset]::new(2026, 9, 22, 12, 0, 0, [timespan]::FromHours(2))
        (Get-RustOffset -Value $utc).UtcTicks |
            Should -Not -Be (Get-RustOffset -Value $plus).UtcTicks
    }
}
