# Enums declared with #[psenum] and unsigned 64-bit values. PWRS_MODULE
# points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'enum Hello.Signal' {
    It 'binds an enum parameter from its name' {
        Get-RustSignal -Signal Green | Should -Be 'Green:None:0'
    }

    It 'binds Option and Vec of the enum' {
        Get-RustSignal -Signal Red -Next Amber -History Red, Green | Should -Be 'Red:Some(Amber):2'
    }

    It 'rejects a name that is not a member' {
        { Get-RustSignal -Signal Purple -ErrorAction Stop } | Should -Throw
    }

    It 'writes a CLR enum value with the declared underlying value' {
        $s = ConvertTo-RustSignal -Value 5
        $s.GetType().FullName | Should -Be 'Hello.Signal'
        "$s" | Should -Be 'Amber'
        [long] $s | Should -Be 5
    }

    It 'carries the enum on a copied class field' {
        $light = New-RustLight -Name corner -State Green
        $light.State | Should -Be ([Hello.Signal]::Green)
        $light.Previous | Should -BeNullOrEmpty
        (New-RustLight -Name corner -State Red -Previous Amber).Previous | Should -Be ([Hello.Signal]::Amber)
    }

    It 'completes enum members through the engine' {
        $line = 'Get-RustSignal -Signal '
        $r = TabExpansion2 -inputScript $line -cursorColumn $line.Length
        $r.CompletionMatches.CompletionText | Should -Contain 'Amber'
    }
}

Describe 'unsigned 64-bit values' {
    It 'round-trips UInt64.MaxValue' {
        $r = Get-RustUnsigned -Value ([uint64]::MaxValue)
        $r.GetType().Name | Should -Be 'UInt64'
        $r | Should -Be ([uint64]::MaxValue)
    }

    It 'sums a UInt64 array' {
        $r = @(Get-RustUnsigned -Value 1 -Values 2, 3, 4)
        $r.Count | Should -Be 2
        $r[1] | Should -Be 9
        $r[1].GetType().Name | Should -Be 'UInt64'
    }
}
