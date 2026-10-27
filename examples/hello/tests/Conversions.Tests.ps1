# Parameter and output conversions beyond primitives. PWRS_MODULE
# points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'ScriptBlock parameters' {
    It 'invokes the block on the pipeline thread with arguments' {
        Invoke-RustBlock -Script { param($a, $b) $a * $b } -Arg 6, 7 | Should -Be 42
    }

    It 'returns every output of the block' {
        @(Invoke-RustBlock -Script { 1; 2; 3 }).Count | Should -Be 3
    }
}

Describe 'Hashtable parameters and outputs' {
    It 'reads keys and values from a hashtable' {
        $r = Get-RustTableInfo -Table @{ alpha = 1; beta = 'two' }
        $r.Count | Should -Be 2
        ($r.Keys | Sort-Object) -join ',' | Should -Be 'alpha,beta'
    }

    It 'reads an ordered table in the order it was given' {
        $r = Get-RustTableInfo -Table ([ordered]@{ zulu = 1; alpha = 2; mike = 3; bravo = 4; yankee = 5 })
        $r.Keys -join ',' | Should -Be 'zulu,alpha,mike,bravo,yankee'
    }

    It 'takes a generic dictionary as it is' {
        $table = [System.Collections.Generic.Dictionary[string, int]]::new()
        $table['one'] = 1
        $table['two'] = 2
        (Get-RustTableInfo -Table $table).Count | Should -Be 2
        (Get-Command Get-RustTableInfo).Parameters['Table'].ParameterType.FullName | Should -Be 'System.Collections.IDictionary'
    }

    It 'reads one key of a table as it was passed, and a missing key as null' {
        Get-RustTableEntry -Table @{ a = 'one' } -Key a | Should -Be 'present:one'
        Get-RustTableEntry -Table @{ a = 'one' } -Key b | Should -Be 'absent:null'
        Get-RustTableEntry -Table ([ordered]@{ a = 'one' }) -Key a | Should -Be 'present:one'
        $generic = [System.Collections.Generic.Dictionary[string, object]]::new()
        $generic['a'] = 'one'
        Get-RustTableEntry -Table $generic -Key a | Should -Be 'present:one'
        Get-RustTableEntry -Table $generic -Key b | Should -Be 'absent:null'
    }

    It 'returns a hashtable built in Rust' {
        $t = New-RustTable -Pairs 'a=1', 'b=2'
        $t | Should -BeOfType [hashtable]
        $t['a'] | Should -Be '1'
        $t['b'] | Should -Be '2'
    }
}

Describe 'Zero-copy byte arrays' {
    It 'checksums a byte[] through a pinned borrow' {
        $bytes = [byte[]](1..255)
        Get-RustChecksum -Bytes $bytes | Should -Be ((1..255 | Measure-Object -Sum).Sum)
    }

    It 'returns a byte[] written through one pin' {
        $out = Get-RustBytes -Count 5
        $out.GetType().Name | Should -Be 'Byte[]'
        $out.Count | Should -Be 5
        $out[4] | Should -Be 4
    }
}

Describe 'BigInteger bridge' {
    It 'round-trips a large BigInteger' {
        $big = [bigint]::Parse('123456789012345678901234567890')
        $r = Test-RustBigInt -Value $big
        $r | Should -BeOfType [bigint]
        $r | Should -Be ($big * 2)
    }

    It 'round-trips a negative BigInteger' {
        Test-RustBigInt -Value ([bigint]-99) | Should -Be ([bigint]-198)
    }
}

Describe 'Dynamic .NET access from Rust' {
    It 'calls a static method and an instance method' {
        Test-RustDynamic -Text 'shout' | Should -Be 'SHOUT:5'
    }

    It 'converts text to an enum for a static method, whose name matches without regard to case' {
        Test-RustDynamic -Text 'x' -Round 2.5 | Should -Be 3
        Test-RustDynamic -Text 'x' -Round -2.5 | Should -Be -3
    }

    It 'converts text to an enum for a constructor' {
        Test-RustDynamic -Text 'Report.TXT' -Like '*.txt' | Should -BeTrue
        Test-RustDynamic -Text 'Report.md' -Like '*.txt' | Should -BeFalse
    }

    It 'constructs an object by type name and reads a property' {
        Test-RustDynamic -Text 'x' -Uri 'https://example.com/path' | Should -Be 'example.com'
    }

    It 'reports the exception a constructor threw, not the reflection wrapper around it' {
        $failed = $null
        Test-RustDynamic -Text 'x' -Uri 'not a uri' -ErrorVariable failed -ErrorAction SilentlyContinue
        $failed.Count | Should -Be 1
        $failed[0].Exception.Message | Should -BeLike 'UriFormatException: *'
    }
}

Describe 'Path resolution' {
    It 'resolves a relative path against the provider location' {
        Push-Location $TestDrive
        try {
            Set-Content -Path 'a.txt' -Value 'x'
            $resolved = Resolve-RustPath -Path 'a.txt'
            $resolved | Should -Be (Join-Path (Get-Location).ProviderPath 'a.txt')
        } finally {
            Pop-Location
        }
    }

    It 'resolves wildcards to several paths' {
        Push-Location $TestDrive
        try {
            Set-Content -Path 'b1.txt' -Value 'x'
            Set-Content -Path 'b2.txt' -Value 'x'
            @(Resolve-RustPath -Path 'b*.txt').Count | Should -Be 2
        } finally {
            Pop-Location
        }
    }

    It 'takes a name through -LiteralPath whose wildcard characters are part of it' {
        Push-Location $TestDrive
        try {
            Set-Content -LiteralPath 'c[1].txt' -Value 'x'
            Resolve-RustPath -LiteralPath 'c[1].txt' |
                Should -Be (Join-Path (Get-Location).ProviderPath 'c[1].txt')
        } finally {
            Pop-Location
        }
    }

    It 'takes the LP alias that every literal-path parameter has' {
        Push-Location $TestDrive
        try {
            Set-Content -LiteralPath 'd.txt' -Value 'x'
            Resolve-RustPath -LP 'd.txt' | Should -Be (Join-Path (Get-Location).ProviderPath 'd.txt')
        } finally {
            Pop-Location
        }
    }

    It 'binds -LiteralPath from a piped object PSPath' {
        Push-Location $TestDrive
        try {
            Set-Content -LiteralPath 'e.txt' -Value 'x'
            Get-Item -LiteralPath 'e.txt' | Resolve-RustPath |
                Should -Be (Join-Path (Get-Location).ProviderPath 'e.txt')
        } finally {
            Pop-Location
        }
    }
}

Describe 'Untyped object parameters' {
    It 'accepts anything as PsObject and reports its type name' {
        Get-RustTypeName -Value 5 | Should -Be 'System.Int32'
        Get-RustTypeName -Value 'x' | Should -Be 'System.String'
        Get-RustTypeName -Value (Get-Date) | Should -Be 'System.DateTime'
    }
}
