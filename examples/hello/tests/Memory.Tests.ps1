# Rust-owned buffers written as Memory<byte> on .NET and as byte[] on
# .NET Framework. PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
    $script:core = $PSVersionTable.PSEdition -eq 'Core'
}

Describe 'Memory<byte> over Rust memory' {
    It 'writes a filled buffer' {
        $m = Get-RustMemory -Count 5
        if ($core) {
            $m.GetType().Name | Should -Be 'Memory`1'
            $m.Length | Should -Be 5
            $bytes = $m.ToArray()
        } else {
            $m.GetType().Name | Should -Be 'Byte[]'
            $bytes = $m
        }
        $bytes.Count | Should -Be 5
        $bytes[0] | Should -Be 0
        $bytes[3] | Should -Be 3
    }

    It 'writes an empty buffer' {
        $m = Get-RustMemory -Count 0
        if ($core) { $m.Length | Should -Be 0 } else { @($m).Count | Should -Be 0 }
    }

    It 'writes a large buffer' {
        $m = Get-RustMemory -Count 4194304
        if ($core) {
            $m.Length | Should -Be 4194304
            $m.ToArray()[4194303] | Should -Be 255
        } else {
            $m.Count | Should -Be 4194304
            $m[4194303] | Should -Be 255
        }
    }
}

Describe 'A reservation the allocator cannot meet' {
    # [uint64]::MaxValue -shr 1 bytes is a size the allocator is asked
    # for and refuses; [uint64]::MaxValue is past the largest a vector
    # can hold and is refused before the allocator is asked.
    It 'comes back as an error record for <Name>' -TestCases @(
        @{ Name = 'a size the allocator refuses'; Bytes = [uint64]::MaxValue -shr 1 }
        @{ Name = 'a size past the largest vector'; Bytes = [uint64]::MaxValue }
    ) {
        param($Bytes)
        $err = $null
        $out = @(New-RustReservation -Bytes $Bytes -ErrorAction SilentlyContinue -ErrorVariable err)
        $out.Count | Should -Be 0
        @($err).Count | Should -Be 1
        $err[0].FullyQualifiedErrorId | Should -Match 'PwrsOutOfMemory'
        $err[0].CategoryInfo.Category | Should -Be 'ResourceUnavailable'
    }

    It 'leaves the session running the next command' {
        New-RustReservation -Bytes ([uint64]::MaxValue -shr 1) -ErrorAction SilentlyContinue
        New-RustReservation -Bytes 4096 | Should -Be 4096
    }
}
