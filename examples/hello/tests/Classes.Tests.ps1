# Output types in the three modes. PWRS_MODULE points at the built
# module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'copied class Hello.Person' {
    It 'is a real CLR type with typed properties' {
        $p = Get-Person -Name Ada -Age 36 -Tag math, code -Score 9.5
        $p.GetType().FullName | Should -Be 'Hello.Person'
        $p.Name | Should -Be 'Ada'
        $p.Age | Should -Be 36
        $p.Age.GetType().Name | Should -Be 'Int64'
        $p.Tags.Count | Should -Be 2
        $p.Tags[1] | Should -Be 'code'
        $p.Score | Should -Be 9.5
        $p.Active | Should -BeTrue
    }

    It 'leaves an absent Option field null' {
        $p = Get-Person -Name Bob
        $p.Score | Should -BeNullOrEmpty
        $p.Age | Should -Be 0
        @($p.Tags).Count | Should -Be 0
    }

    It 'survives Get-Member and formatting' {
        $names = (Get-Person -Name Cy | Get-Member -MemberType Property).Name
        $names | Should -Contain 'Name'
        $names | Should -Contain 'Tags'
        (Get-Person -Name Cy | Format-List | Out-String) | Should -Match 'Cy'
    }
}

Describe 'proxy class Hello.Counter' {
    It 'reads fields through the proxy' {
        $c = New-Counter -Label ticks -Value 7
        $c.GetType().FullName | Should -Be 'Hello.Counter'
        $c.Label | Should -Be 'ticks'
        $c.Value | Should -Be 7
        $c.IsDisposed | Should -BeFalse
    }

    It 'frees the Rust value on Dispose; PowerShell reads null afterwards, .NET callers get ObjectDisposedException' {
        $c = New-Counter -Label once -Value 5
        $c.Value | Should -Be 5
        $c.Dispose()
        $c.IsDisposed | Should -BeTrue
        $c.Value | Should -BeNullOrEmpty
        { $c.GetType().GetProperty('Value').GetValue($c) } | Should -Throw
        $c.Dispose()
    }

    It 'creates and disposes many proxies' {
        foreach ($i in 1..2000) { (New-Counter -Label n -Value $i).Dispose() }
        (New-Counter -Label last -Value 1).Value | Should -Be 1
    }

    It 'keeps a #[psfield(skip)] field in Rust, off the property list' {
        $t = New-RustTicker -Label t
        ($t | Get-Member -MemberType Property).Name | Should -Not -Contain 'Ticks'
        ($t | Get-Member -MemberType Property).Name | Should -Contain 'Label'
        $t.Tick() | Should -Be 1
        $t.Tick() | Should -Be 2
        $t.Tick().GetType().Name | Should -Be 'UInt64'
    }

    It 'takes a byte array into that hidden field through a method' {
        $t = New-RustTicker -Label t
        $t.Feed([byte[]](1, 2, 3)) | Should -Be 6
        $t.Feed([byte[]]@()) | Should -Be 6
        $t.Tick() | Should -Be 7
        $t.Feed([byte[]](1..255)).GetType().Name | Should -Be 'UInt64'
    }

    It 'reads narrow numeric proxy fields at their declared types' {
        $t = New-RustTicker -Label t -Width 9 -Limit 300
        $t.Width | Should -Be 9
        $t.Width.GetType().Name | Should -Be 'UInt32'
        $t.Step | Should -Be 0.5
        $t.Step.GetType().Name | Should -Be 'Single'
        $t.Limit | Should -Be 300
        $t.Limit.GetType().Name | Should -Be 'UInt16'
        (New-RustTicker -Label t).Limit | Should -BeNullOrEmpty
    }

    It 'returns narrow numbers from methods at their declared types' {
        $t = New-RustTicker -Label t -Width 9 -Limit 300
        $t.Narrow() | Should -Be 9
        $t.Narrow().GetType().Name | Should -Be 'UInt32'
        $t.Ratio() | Should -Be 0.5
        $t.Ratio().GetType().Name | Should -Be 'Single'
        $t.Bound() | Should -Be 300
        $t.Bound().GetType().Name | Should -Be 'UInt16'
        (New-RustTicker -Label t).Bound() | Should -BeNullOrEmpty
    }
}

Describe 'psobject class Hello.Note' {
    It 'is a PSObject with a PSTypeName and note properties' {
        $n = Get-Note -Text hi -Priority 1
        $n.PSObject.TypeNames[0] | Should -Be 'Hello.Note'
        $n.Text | Should -Be 'hi'
        $n.Priority | Should -Be 1
        (Get-Note -Text d).Priority | Should -Be 3
    }
}

Describe 'the runtime serving the module' {
    It 'says it keeps a class-factory table per module' {
        # Reached through a cmdlet's base type, the way another module
        # reads it: on PowerShell 7 the runtime sits in this module's own
        # load context, where a type literal does not look.
        $runtime = (Get-Command Get-Person).ImplementingType.BaseType.Assembly
        $marker = $runtime.GetType('Pwrs.NativeModule', $true).GetProperty('FactoriesPerModule')
        $marker | Should -Not -BeNullOrEmpty
        $marker.GetGetMethod().IsStatic | Should -BeTrue
        $marker.GetValue($null) | Should -BeTrue
    }
}
