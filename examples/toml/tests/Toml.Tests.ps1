# Toml as it is built, run in pwsh and in Windows PowerShell.
# cargo pwrs test sets PWRS_MODULE to the module's folder.
BeforeAll {
    Import-Module (Join-Path $env:PWRS_MODULE 'Toml.psd1') -Force -ErrorAction Stop
}

Describe 'Toml' {
    Context 'ConvertFrom-Toml' {
        It 'reads a table as an object whose properties keep the order of the text' {
            $doc = ConvertFrom-Toml "title = 'demo'`n[owner]`nname = 'Tom'`nzeta = 1`nalpha = 2"
            $doc.GetType().FullName | Should -Be 'System.Management.Automation.PSCustomObject'
            $doc.title | Should -BeExactly 'demo'
            $doc.owner.name | Should -BeExactly 'Tom'
            $doc.owner.PSObject.Properties.Name | Should -BeExactly @('name', 'zeta', 'alpha')
        }

        It 'gives each kind of value its .NET type' {
            $doc = ConvertFrom-Toml "text = 'a'`nint = 42`nfloat = 1.5`nflag = true`nlist = [1, 'two']`ninline = { x = 1 }"
            $doc.text | Should -BeOfType [string]
            $doc.int | Should -BeOfType [long]
            $doc.int | Should -Be 42
            $doc.float | Should -BeOfType [double]
            $doc.flag | Should -BeOfType [bool]
            $doc.list.GetType().FullName | Should -Be 'System.Object[]'
            $doc.list[0] | Should -BeOfType [long]
            $doc.list[1] | Should -BeExactly 'two'
            $doc.inline.x | Should -Be 1
        }

        It 'reads each kind of date and time as a .NET date or time' {
            $doc = ConvertFrom-Toml @'
offset = 1979-05-27T07:32:00-07:00
utc = 1979-05-27T07:32:00Z
local = 1979-05-27T07:32:00.999999
day = 1979-05-27
time = 07:32:00.5
'@
            $doc.offset | Should -BeOfType [DateTimeOffset]
            $doc.offset.Offset | Should -Be ([TimeSpan]::FromHours(-7))
            $doc.offset.DateTime | Should -Be ([DateTime]::new(1979, 5, 27, 7, 32, 0))
            $doc.utc.Offset | Should -Be ([TimeSpan]::Zero)
            $doc.local | Should -BeOfType [DateTime]
            $doc.local.Kind | Should -Be 'Unspecified'
            $doc.local.Ticks | Should -Be ([DateTime]::new(1979, 5, 27, 7, 32, 0).Ticks + 9999990)
            $doc.day | Should -Be ([DateTime]::new(1979, 5, 27))
            $doc.time | Should -Be ([TimeSpan]::new(0, 7, 32, 0, 500))
        }

        It 'returns an array of tables as an array of objects' {
            $doc = ConvertFrom-Toml "[[server]]`nname = 'alpha'`n[[server]]`nname = 'beta'"
            $doc.server.Count | Should -Be 2
            $doc.server[0].name | Should -BeExactly 'alpha'
            $doc.server[1].name | Should -BeExactly 'beta'
        }

        It 'reads the strings piped to it as the lines of one document' {
            $doc = 'a = 1', '', '[t]', 'b = 2' | ConvertFrom-Toml
            $doc.a | Should -Be 1
            $doc.t.b | Should -Be 2
        }

        It 'reads a file from Get-Content, line by line or whole' {
            $file = Join-Path $TestDrive 'demo.toml'
            Set-Content -LiteralPath $file -Value "[package]`nname = 'demo'`nversion = '1.0.0'"
            (Get-Content -LiteralPath $file | ConvertFrom-Toml).package.name | Should -BeExactly 'demo'
            (Get-Content -LiteralPath $file -Raw | ConvertFrom-Toml).package.version | Should -BeExactly '1.0.0'
        }

        It 'writes nothing when it is given no text at all' {
            @(@() | ConvertFrom-Toml).Count | Should -Be 0
        }

        It 'returns ordered hashtables under -AsHashtable, whose keys keep their case' {
            $doc = ConvertFrom-Toml "a = 1`nA = 2`n[t]`nx = 3" -AsHashtable
            $doc | Should -BeOfType [System.Collections.Specialized.OrderedDictionary]
            @($doc.Keys) | Should -BeExactly @('a', 'A', 't')
            $doc['a'] | Should -Be 1
            $doc['A'] | Should -Be 2
            $doc['t'] | Should -BeOfType [System.Collections.Specialized.OrderedDictionary]
            $doc['t']['x'] | Should -Be 3
        }

        It 'refuses an empty key unless -AsHashtable is given' {
            { ConvertFrom-Toml '"" = 1' } | Should -Throw '*is an empty key*-AsHashtable*'
            (ConvertFrom-Toml '"" = 1' -AsHashtable)[''] | Should -Be 1
        }

        It 'refuses keys that differ only in case unless -AsHashtable is given' {
            { ConvertFrom-Toml "[t]`nkey = 1`nKEY = 2" } | Should -Throw '*t.key and t.KEY differ only in case*-AsHashtable*'
        }

        It 'ends with an error giving the line and column of text that is not TOML' {
            $failure = { ConvertFrom-Toml "[package]`nname = demo" } | Should -Throw -PassThru
            $failure.FullyQualifiedErrorId | Should -BeLike 'TomlParseError,*'
            $failure.CategoryInfo.Category | Should -Be 'ParserError'
            $failure.Exception.Message | Should -BeExactly 'TOML parse error at line 2, column 8: string values must be quoted, expected literal string'
            $failure.Exception.InnerException.Message | Should -BeLike '*name = demo*^*'
        }
    }

    Context 'ConvertTo-Toml' {
        It 'writes a table with its keys in their order' {
            [ordered]@{ name = 'demo'; version = '1.0.0' } | ConvertTo-Toml | Should -BeExactly "name = `"demo`"`nversion = `"1.0.0`""
        }

        It 'writes nested tables and arrays of tables after the plain keys' {
            $text = [ordered]@{
                owner = [ordered]@{ name = 'Tom' }
                title = 'demo'
                server = @([ordered]@{ name = 'alpha' }, [ordered]@{ name = 'beta' })
                ports = 80, 443
            } | ConvertTo-Toml
            $back = ConvertFrom-Toml $text
            $back.PSObject.Properties.Name | Should -BeExactly @('title', 'ports', 'owner', 'server')
            $back.owner.name | Should -BeExactly 'Tom'
            $back.server.name | Should -BeExactly @('alpha', 'beta')
            $back.ports | Should -Be @(80, 443)
        }

        It 'writes the objects ConvertFrom-Toml returns, so a document is read back unchanged' {
            $text = "title = `"demo`"`n`n[package]`nname = `"demo`"`nversion = `"1.0.0`""
            ConvertFrom-Toml $text | ConvertTo-Toml | Should -BeExactly $text
            ConvertFrom-Toml $text -AsHashtable | ConvertTo-Toml | Should -BeExactly $text
        }

        It 'writes a generic dictionary, and any other object as a table of its properties' {
            $dictionary = [System.Collections.Generic.Dictionary[string, object]]::new()
            $dictionary['version'] = [version]'1.2.3.4'
            $back = ConvertFrom-Toml ($dictionary | ConvertTo-Toml)
            $back.version.Major | Should -Be 1
            $back.version.Revision | Should -Be 4
        }

        It 'writes each .NET type as its TOML counterpart' {
            $text = [ordered]@{
                int = [int]1
                byte = [byte]2
                long = [long]3
                single = [float]0.1
                double = 0.25
                decimal = [decimal]1.5
                flag = $true
                letter = [char]'x'
                id = [guid]'0f8fad5b-d9cb-469f-a165-70867728950e'
                color = [ConsoleColor]::Red
                big = [bigint]5
            } | ConvertTo-Toml
            $back = ConvertFrom-Toml $text
            $back.int | Should -BeOfType [long]
            $back.byte | Should -Be 2
            $back.long | Should -Be 3
            $back.single | Should -Be 0.1
            $back.double | Should -Be 0.25
            $back.decimal | Should -Be 1.5
            $back.flag | Should -BeTrue
            $back.letter | Should -BeExactly 'x'
            $back.id | Should -BeExactly '0f8fad5b-d9cb-469f-a165-70867728950e'
            $back.color | Should -BeExactly 'Red'
            $back.big | Should -Be 5
        }

        It 'writes each date and time by its .NET type' {
            $text = [ordered]@{
                offset = [DateTimeOffset]::new(1979, 5, 27, 7, 32, 0, [TimeSpan]::FromHours(-7))
                utc = [DateTime]::new(1979, 5, 27, 7, 32, 0, [DateTimeKind]::Utc)
                local = [DateTime]::new(1979, 5, 27, 7, 32, 0, 500)
                time = [TimeSpan]::new(7, 32, 0)
            } | ConvertTo-Toml
            $text | Should -BeExactly "offset = 1979-05-27T07:32:00-07:00`nutc = 1979-05-27T07:32:00Z`nlocal = 1979-05-27T07:32:00.5`ntime = 07:32:00"
        }

        It 'refuses $null, and leaves the key out under -SkipNull' {
            { [ordered]@{ a = 1; b = $null } | ConvertTo-Toml -ErrorAction Stop } | Should -Throw '*b is $null*-SkipNull*'
            [ordered]@{ a = 1; b = $null } | ConvertTo-Toml -SkipNull | Should -BeExactly 'a = 1'
            { [ordered]@{ list = @(1, $null) } | ConvertTo-Toml -SkipNull -ErrorAction Stop } | Should -Throw '*list`[1`] is $null*'
        }

        It 'refuses nesting deeper than -Depth and names where it is' {
            $deep = [ordered]@{ a = [ordered]@{ b = [ordered]@{ c = 1 } } }
            (ConvertFrom-Toml ($deep | ConvertTo-Toml -Depth 2)).a.b.c | Should -Be 1
            { $deep | ConvertTo-Toml -Depth 1 -ErrorAction Stop } | Should -Throw '*a.b is nested 2 levels deep, and -Depth 1 allows fewer*'
        }

        It 'stops at an object whose property refers back to it when it reaches -Depth' {
            $loop = [pscustomobject]@{ name = 'loop' }
            $loop | Add-Member -NotePropertyName self -NotePropertyValue $loop
            { $loop | ConvertTo-Toml -ErrorAction Stop } | Should -Throw '*nested 33 levels deep, and -Depth 32 allows fewer*'
        }

        It 'refuses what is not a table and goes on to the next object' {
            $text = 'text', [ordered]@{ a = 1 } | ConvertTo-Toml -ErrorVariable problems -ErrorAction SilentlyContinue
            $text | Should -BeExactly 'a = 1'
            $problems.Count | Should -Be 1
            $problems[0].FullyQualifiedErrorId | Should -BeLike 'TomlNotATable,*'
            { ConvertTo-Toml -InputObject $null -ErrorAction Stop } | Should -Throw '*and $null is not one*'
        }

        It 'refuses a key that is not a string, an integer past 64 bits and a TimeSpan of a day or more' {
            { @{ 1 = 'one' } | ConvertTo-Toml -ErrorAction Stop } | Should -Throw '*has the key 1, a System.Int32, and a TOML key is a string*'
            { @{ n = [uint64]::MaxValue } | ConvertTo-Toml -ErrorAction Stop } | Should -Throw '*n is 18446744073709551615*'
            { @{ n = [bigint]::Pow(2, 70) } | ConvertTo-Toml -ErrorAction Stop } | Should -Throw '*n is 1180591620717411303424*'
            { @{ t = [TimeSpan]::FromHours(25) } | ConvertTo-Toml -ErrorAction Stop } | Should -Throw '*t is 1.01:00:00*time of day*'
        }
    }
}
