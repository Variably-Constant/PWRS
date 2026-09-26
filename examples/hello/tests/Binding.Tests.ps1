# The parameter attributes that shape binding: ShouldProcess, parameter
# sets, pipeline-by-property-name, remaining arguments, the validators
# and DontShow. PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'supports_should_process' {
    It 'gives the cmdlet -WhatIf and -Confirm' {
        $p = (Get-Command Remove-RustThing).Parameters
        $p.ContainsKey('WhatIf') | Should -BeTrue
        $p.ContainsKey('Confirm') | Should -BeTrue
    }

    It 'acts when the engine allows the change' {
        Remove-RustThing -Name cache -Confirm:$false | Should -Be 'removed cache'
    }

    It 'does not act under -WhatIf' {
        Remove-RustThing -Name cache -WhatIf | Should -BeNullOrEmpty
    }
}

Describe 'parameter sets' {
    It 'defaults to ByName' {
        (Get-Command Get-RustRecord).DefaultParameterSet | Should -Be 'ByName'
        Get-RustRecord abc | Should -Be 'name abc tag none rest  internal false'
    }

    It 'binds the other set by its own parameter' {
        Get-RustRecord -Id 7 | Should -Be 'id 7 tag none rest  internal false'
    }

    It 'refuses both sets at once' {
        { Get-RustRecord -Name abc -Id 7 -ErrorAction Stop } | Should -Throw
    }
}

Describe 'a parameter in several sets' {
    It 'belongs to the sets it names and to no other' {
        $sets = @((Get-Command Get-RustRoute).Parameters['Destination'].ParameterSets.Keys | Sort-Object)
        $sets | Should -Be @('LiteralPath', 'Path')
        @((Get-Command Get-RustRoute).ParameterSets.Name | Sort-Object) | Should -Be @('LiteralPath', 'Path', 'Text')
    }

    It 'binds in each set it names, by position where that set gives one' {
        Get-RustRoute notes.txt archive.txt | Should -Be 'path notes.txt -> archive.txt'
        Get-RustRoute -LiteralPath 'notes[1].txt' -Destination archive.txt | Should -Be 'literal notes[1].txt -> archive.txt'
        Get-RustRoute -Text hello | Should -Be 'text hello -> none'
    }

    It 'is refused beside a parameter of a set it does not name' {
        $e = $null
        try { Get-RustRoute -Text hello -Destination archive.txt -ErrorAction Stop } catch { $e = $_ }
        $e | Should -Not -BeNullOrEmpty
        $e.FullyQualifiedErrorId | Should -BeLike 'AmbiguousParameterSet*'
    }

    It 'gets a help syntax line per set, each naming only its own parameters' {
        $lines = @((Get-Help Get-RustRoute).syntax.syntaxItem | ForEach-Object { (@($_.parameter | ForEach-Object name) | Sort-Object) -join ',' })
        $lines.Count | Should -Be 3
        $lines | Should -Contain 'Destination,Path'
        $lines | Should -Contain 'Destination,LiteralPath'
        $lines | Should -Contain 'Text'
    }
}

Describe 'binding modifiers' {
    It 'binds a parameter from a property of a piped object' {
        # Only Tag carries ValueFromPipelineByPropertyName, so Name
        # comes from the command line and Tag from the object.
        $r = [pscustomobject]@{ Tag = 'blue' } | Get-RustRecord -Name abc
        $r | Should -Be 'name abc tag blue rest  internal false'
    }

    It 'collects the leftover arguments' {
        Get-RustRecord abc extra1 extra2 | Should -Be 'name abc tag none rest extra1,extra2 internal false'
    }

    It 'hides a DontShow parameter from help while still binding it' {
        $internal = (Get-Command Get-RustRecord).Parameters['Internal']
        @($internal.Attributes | Where-Object { $_.DontShow }).Count | Should -BeGreaterThan 0
        Get-RustRecord abc -Internal | Should -Be 'name abc tag none rest  internal true'
    }
}

Describe 'validators' {
    It 'rejects a name that does not match the pattern' {
        { Get-RustRecord -Name ab1 -ErrorAction Stop } | Should -Throw
    }

    It 'matches the pattern without regard to case, as the engine does' {
        # ValidatePattern carries RegexOptions.IgnoreCase by default,
        # so ^[a-z]+$ admits an uppercase name.
        $attr = (Get-Command Get-RustRecord).Parameters['Name'].Attributes |
            Where-Object { $_ -is [System.Management.Automation.ValidatePatternAttribute] }
        "$($attr.Options)" | Should -Match 'IgnoreCase'
        Get-RustRecord -Name ABC | Should -Be 'name ABC tag none rest  internal false'
    }

    It 'rejects an empty name' {
        { Get-RustRecord -Name '' -ErrorAction Stop } | Should -Throw
    }

    It 'accepts a name that matches' {
        Get-RustRecord -Name abc | Should -Be 'name abc tag none rest  internal false'
    }
}
