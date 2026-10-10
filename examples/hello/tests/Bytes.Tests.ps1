# byte[] values bound to Vec<u8> parameters and written from Vec<u8>.
# PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'byte array parameters and outputs' {
    It 'sums a typed byte array' {
        Get-RustByteSum -Bytes ([byte[]](1..255)) | Should -Be 32640
    }

    It 'sums an untyped array of numbers' {
        Get-RustByteSum -Bytes @(1, 2, 3) | Should -Be 6
    }

    It 'writes an empty byte array as one object' {
        $r = Get-RustByteRange -Count 0
        $r.GetType().Name | Should -Be 'Byte[]'
        $r.Count | Should -Be 0
    }

    It 'writes a typed byte array' {
        $r = Get-RustByteRange -Count 300
        $r.GetType().Name | Should -Be 'Byte[]'
        $r.Count | Should -Be 300
        $r[299] | Should -Be 43
    }

    It 'round-trips four megabytes' {
        $r = Get-RustByteRange -Count 4194304
        $r.Count | Should -Be 4194304
        Get-RustByteSum -Bytes $r | Should -Be 534773760
    }
}

Describe 'a raw parameter skips the engine array binder' {
    It 'sums a typed byte array' {
        Get-RustRawByteSum -Bytes ([byte[]](1..255)) | Should -Be 32640
    }

    It 'agrees with the bound parameter on four megabytes' {
        $r = Get-RustByteRange -Count 4194304
        Get-RustRawByteSum -Bytes $r | Should -Be (Get-RustByteSum -Bytes $r)
    }

    It 'trades the engine type check for the conversion in Rust' {
        # The typed parameter is rejected by the binder before Rust runs.
        { Get-RustByteSum -Bytes 'nope' -ErrorAction Stop } | Should -Throw -ErrorId 'CannotConvertArgumentNoMessage,Pwrs.Modules.Hello.GetRustByteSumCommand'
        # The raw one reaches Rust, where a string enumerates as its characters.
        Get-RustRawByteSum -Bytes 'AB' | Should -Be 131
        # A value out of a byte's range is the conversion's own error.
        { Get-RustRawByteSum -Bytes 300 -ErrorAction Stop } | Should -Throw -ErrorId 'PwrsConversionError,Pwrs.Modules.Hello.GetRustRawByteSumCommand'
    }
}

Describe 'a PsObject parameter declared byte[] with clr' {
    It 'is declared byte[] to the binder' {
        (Get-Command Measure-RustInput).Parameters['InputObject'].ParameterType | Should -Be ([byte[]])
    }

    It 'takes a byte array bound by name or piped whole' {
        $bytes = [byte[]](1, 2, 3)
        Measure-RustInput -InputObject $bytes | Should -Be 'bytes 3 sum 6'
        , $bytes | Measure-RustInput | Should -Be 'bytes 3 sum 6'
    }

    It 'takes each byte of an enumerated array as an array of one' {
        @([byte[]](1, 2, 3) | Measure-RustInput) | Should -Be @('bytes 1 sum 1', 'bytes 1 sum 2', 'bytes 1 sum 3')
    }

    It 'leaves a piped file to -LiteralPath, which binds its PSPath' {
        $file = Join-Path ([IO.Path]::GetTempPath()) "pwrs-declared-$PID.bin"
        [IO.File]::WriteAllBytes($file, [byte[]](9, 9))
        try {
            Get-Item -LiteralPath $file | Measure-RustInput | Should -Be "file pwrs-declared-$PID.bin"
        } finally {
            Remove-Item -LiteralPath $file
        }
    }

    It 'reaches the caller''s own array, so nothing was copied' {
        $bytes = [byte[]](1, 2, 3)
        Measure-RustInput -InputObject $bytes -Invert | Should -Be 'bytes 3 sum 6'
        $bytes | Should -Be @(254, 253, 252)
    }

    It 'binds a byte[] wrapped in a PSObject as the same array, not an element-by-element copy' {
        $bytes = [byte[]](1, 2, 3)
        $wrapped = Write-Output -NoEnumerate $bytes
        $wrapped -is [psobject] | Should -BeTrue
        Measure-RustInput -InputObject $wrapped -Invert | Should -Be 'bytes 3 sum 6'
        $bytes | Should -Be @(254, 253, 252)
    }
}

Describe 'allow_empty_collection' {
    It 'lets a mandatory array parameter take an empty array' {
        $attr = @((Get-Command Measure-RustInput).Parameters['InputObject'].Attributes | Where-Object { $_ -is [System.Management.Automation.AllowEmptyCollectionAttribute] })
        $attr.Count | Should -Be 1
        Measure-RustInput -InputObject ([byte[]]::new(0)) | Should -Be 'bytes 0 sum 0'
        , [byte[]]::new(0) | Measure-RustInput | Should -Be 'bytes 0 sum 0'
    }

    It 'leaves a mandatory array parameter without it refusing an empty array, as the engine does' {
        { Get-RustByteSum -Bytes ([byte[]]::new(0)) -ErrorAction Stop } | Should -Throw -ErrorId 'ParameterArgumentValidationErrorEmptyArrayNotAllowed,Pwrs.Modules.Hello.GetRustByteSumCommand'
    }
}
