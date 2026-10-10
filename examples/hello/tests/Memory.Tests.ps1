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

Describe 'A view of memory the module keeps alive' {
    # The module hands out views of sixteen bytes it holds, 0 to 15,
    # each keeping them alive through an Arc; Get-RustRegionHolderCount
    # counts the views not yet released.
    BeforeAll {
        function Wait-Collection {
            [GC]::Collect()
            [GC]::WaitForPendingFinalizers()
            [GC]::Collect()
        }
    }

    It 'is a Memory over the bytes on PowerShell 7 and a byte[] copy on Windows PowerShell' {
        $v = New-RustRegionView
        if ($core) {
            $v.GetType().Name | Should -Be 'Memory`1'
            $v.Length | Should -Be 16
            $v.ToArray()[5] | Should -Be 5
        } else {
            $v.GetType().Name | Should -Be 'Byte[]'
            $v.Count | Should -Be 16
            $v[5] | Should -Be 5
        }
    }

    It 'carries a write through the view to the bytes the module reads' {
        $v = New-RustRegionView
        if ($core) {
            [System.Memory[byte]]::new([byte[]]@(200)).CopyTo($v.Slice(3, 1))
            (Get-RustRegion)[3] | Should -Be 200
            [System.Memory[byte]]::new([byte[]]@(3)).CopyTo($v.Slice(3, 1))
        } else {
            $v[3] = 200
        }
        (Get-RustRegion)[3] | Should -Be 3
    }

    It 'is a ReadOnlyMemory when made read-only' {
        $v = New-RustRegionView -ReadOnly
        if ($core) {
            $v.GetType().Name | Should -Be 'ReadOnlyMemory`1'
            $v.ToArray()[15] | Should -Be 15
        } else {
            $v.GetType().Name | Should -Be 'Byte[]'
        }
    }

    It 'holds the bytes until it is collected on PowerShell 7, and not at all on Windows PowerShell' {
        Wait-Collection
        $before = Get-RustRegionHolderCount
        $v = New-RustRegionView
        Get-RustRegionHolderCount | Should -Be ($before + [int]$core)
        $v = $null
        Wait-Collection
        Get-RustRegionHolderCount | Should -Be $before
    }

    It 'refuses every read once its manager is disposed, and lets go of the bytes then' {
        if ($core) {
            Wait-Collection
            $before = Get-RustRegionHolderCount
            $v = New-RustRegionView
            # Reflection, since Windows PowerShell's parser reads this file
            # too and has no syntax for a generic method's type arguments.
            $try = [System.Runtime.InteropServices.MemoryMarshal].GetMethods() |
                Where-Object { $_.Name -eq 'TryGetMemoryManager' -and $_.GetParameters().Count -eq 2 }
            $arguments = @([System.ReadOnlyMemory[byte]]$v, $null)
            $try.MakeGenericMethod([byte], [System.Buffers.MemoryManager[byte]]).Invoke($null, $arguments) | Should -BeTrue
            [System.IDisposable].GetMethod('Dispose').Invoke($arguments[1], @())
            Get-RustRegionHolderCount | Should -Be $before
            $e = { $v.ToArray() } | Should -Throw -PassThru
            $e.Exception.InnerException | Should -BeOfType ([System.ObjectDisposedException])
            $e.Exception.InnerException.Message | Should -Match 'disposed'
        } else {
            (New-RustRegionView).GetType().Name | Should -Be 'Byte[]'
        }
    }

    It 'refuses every read once revoked, while a later view and an untied one still read' {
        $tied = New-RustRegionView -Revocable
        $untied = New-RustRegionView
        Revoke-RustRegionView
        if ($core) {
            $e = { $tied.ToArray() } | Should -Throw -PassThru
            $e.Exception.InnerException | Should -BeOfType ([System.ObjectDisposedException])
            $e.Exception.InnerException.Message | Should -Match 'revoked'
            (New-RustRegionView -Revocable).ToArray()[1] | Should -Be 1
            $untied.ToArray()[1] | Should -Be 1
        } else {
            $tied[1] | Should -Be 1
            $untied[1] | Should -Be 1
        }
    }

    It 'is refused when it is made already revoked, and lets go of the bytes' {
        Wait-Collection
        $before = Get-RustRegionHolderCount
        $err = $null
        $out = @(New-RustRegionView -Revoked -ErrorAction SilentlyContinue -ErrorVariable err)
        $out.Count | Should -Be 0
        @($err).Count | Should -Be 1
        $err[0].Exception.Message | Should -Match 'revoked'
        Get-RustRegionHolderCount | Should -Be $before
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
