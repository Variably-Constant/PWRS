# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Proxy classes that derive from a .NET class or implement .NET
# interfaces, each member a Rust method backs. .NET's own code takes
# them as it takes any object of the type: a StreamReader reads a
# Rust-backed stream, -f formats through IFormattable, [Array]::Sort
# orders through IComparer, and a script throws and catches an
# Exception made in Rust. A managed array a method or cmdlet takes as
# PsArrayMut is the caller's own, read and written in place.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'A Rust value as a System.IO.Stream' {
    It 'is a Stream that a StreamReader reads to the end and disposes' {
        $s = [Hello.ByteStream]::new("one`ntwo")
        $s -is [System.IO.Stream] | Should -BeTrue
        $reader = [System.IO.StreamReader]::new($s)
        try {
            $reader.ReadToEnd() | Should -Be "one`ntwo"
        } finally {
            $reader.Dispose()
        }
        $s.IsDisposed | Should -BeTrue
    }

    It 'reads into the caller''s buffer in place, seeks, writes and is copied into another stream' {
        $s = [Hello.ByteStream]::new('abcdef')
        $s.CanRead -and $s.CanSeek -and $s.CanWrite | Should -BeTrue
        $buffer = [byte[]]::new(4)
        $s.Read($buffer, 1, 3) | Should -Be 3
        [System.Text.Encoding]::ASCII.GetString($buffer, 1, 3) | Should -Be 'abc'
        $s.Position | Should -Be 3
        $s.Seek(-1, 'End') | Should -Be 5
        $s.Write([byte[]](0x58, 0x59), 0, 2)
        $s.Length | Should -Be 7
        $s.Position = 0
        $copy = [System.IO.MemoryStream]::new()
        $s.CopyTo($copy)
        [System.Text.Encoding]::ASCII.GetString($copy.ToArray()) | Should -Be 'abcdeXY'
        $s.SetLength(2)
        $s.Text() | Should -Be 'ab'
        $s.Flush()
        $s.Dispose()
        $s.IsDisposed | Should -BeTrue
    }

    It 'refuses a window outside the buffer and a seek before the start' {
        $s = [Hello.ByteStream]::new('ab')
        { $s.Read([byte[]]::new(2), 1, 5) } | Should -Throw '*5 bytes from 1 are outside an array of 2*'
        { $s.Seek(-3, 'Current') } | Should -Throw '*before the start of the stream*'
        $s.Dispose()
    }

    It 'is made by a cmdlet, which calls the base constructor that takes nothing' {
        $s = New-RustByteStream 'made by a cmdlet'
        $s.GetType().FullName | Should -Be 'Hello.ByteStream'
        $reader = [System.IO.StreamReader]::new($s)
        try {
            $reader.ReadToEnd() | Should -Be 'made by a cmdlet'
        } finally {
            $reader.Dispose()
        }
    }
}

Describe 'A managed array taken in place' {
    It 'is the caller''s own array, which a cmdlet writes' {
        $bytes = [byte[]](1, 2, 3)
        Set-RustBytes -Buffer $bytes -Value 9
        $bytes -join ',' | Should -Be '9,9,9'
    }

    It 'is copied between two arrays, and refused when one array is both' {
        $from = [byte[]](1, 2, 3)
        $to = [byte[]]::new(2)
        [Hello.ByteStream]::CopyBytes($from, $to) | Should -Be 2
        $to -join ',' | Should -Be '1,2'
        { [Hello.ByteStream]::CopyBytes($from, $from) } | Should -Throw '*already pinned on this thread*'
        $from -join ',' | Should -Be '1,2,3'
    }
}

Describe 'A proxy implementing IFormattable' {
    It 'is formatted by -f and by ToString with a format' {
        $f = [Hello.Fraction]::new(1, 4)
        $f -is [System.IFormattable] | Should -BeTrue
        '{0:F2}' -f $f | Should -Be '0.25'
        '{0:P0}' -f $f | Should -Be '25%'
        $f.ToString('R', $null) | Should -Be '1/4'
        { $f.ToString('X', $null) } | Should -Throw '*has no format X*'
    }

    It 'overrides ToString() with a method of its own' {
        $f = [Hello.Fraction]::new(3, 8)
        $f.ToString() | Should -Be '3/8'
        "$f" | Should -Be '3/8'
    }
}

Describe 'A proxy implementing IComparer' {
    It 'orders an array for [Array]::Sort and counts what it compared' {
        $numbers = [object[]](3, 1, 2, 5)
        $by = [Hello.Descending]::new()
        [Array]::Sort($numbers, $by)
        $numbers -join ',' | Should -Be '5,3,2,1'
        $by.Compared | Should -BeGreaterThan 0
    }
}

Describe 'A proxy deriving from System.Exception' {
    It 'is thrown and caught with the message new gave its base and the code Rust has' {
        $caught = $null
        try {
            throw [Hello.Failure]::new('disk full', 28)
        } catch {
            $caught = $_.Exception
        }
        $caught -is [System.Exception] | Should -BeTrue
        $caught.GetType().FullName | Should -Be 'Hello.Failure'
        $caught.Message | Should -Be 'disk full'
        $caught.Code | Should -Be 28
    }

    It 'takes its message from the #[base] method when a cmdlet makes it' {
        $e = New-RustFailure 'no route' 113
        $e.Message | Should -Be 'no route'
        $e.Code | Should -Be 113
        $e.Dispose()
        $e.IsDisposed | Should -BeTrue
    }

    It 'leaves nothing behind when its Rust constructor refuses, through a collection' {
        { [Hello.Failure]::new('below zero', -1) } | Should -Throw '*0 or more, not -1*'
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
        [GC]::Collect()
        ([Hello.Failure]::new('after', 1)).Code | Should -Be 1
    }
}
