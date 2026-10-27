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

Describe 'allow_empty_string' {
    It 'declares AllowEmptyString on the parameter' {
        $attr = @((Get-Command Measure-RustLine).Parameters['InputObject'].Attributes | Where-Object { $_ -is [System.Management.Automation.AllowEmptyStringAttribute] })
        $attr.Count | Should -Be 1
    }

    It 'binds a blank line piped in, between two others' {
        'a', '', 'bc' | Measure-RustLine | Should -Be @('1: 1', '2: 0', '3: 2')
    }

    It 'refuses an empty string for a mandatory string parameter without it' {
        # The binder refuses it before the cmdlet runs, so the record is
        # on the error stream, not in the cmdlet's -ErrorVariable; under
        # the runner's Stop preference it would throw instead.
        $ErrorActionPreference = 'Continue'
        $records = @('' | Get-Greeting 2>&1)
        $records.Count | Should -Be 1
        $records[0] | Should -BeOfType System.Management.Automation.ErrorRecord
        $records[0].FullyQualifiedErrorId | Should -BeLike 'ParameterArgumentValidationErrorEmptyStringNotAllowed,*'
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

    It 'takes a piped file into -LiteralPath, which binds its PSPath' {
        $file = Join-Path ([IO.Path]::GetTempPath()) "pwrs-route-$PID.txt"
        [IO.File]::WriteAllText($file, 'x')
        try {
            $item = Get-Item -LiteralPath $file
            $item | Get-RustRoute | Should -Be "literal $($item.PSPath) -> none"
        } finally {
            Remove-Item -LiteralPath $file
        }
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
        # Only Tag has ValueFromPipelineByPropertyName, so Name
        # comes from the command line and Tag from the object.
        $r = [pscustomobject]@{ Tag = 'blue' } | Get-RustRecord -Name abc
        $r | Should -Be 'name abc tag blue rest  internal false'
    }

    It 'reads a null property bound to an optional string as absent, and an empty one as empty' {
        [pscustomobject]@{ Tag = $null } | Get-RustRecord -Name abc | Should -Be 'name abc tag none rest  internal false'
        [pscustomobject]@{ Tag = '' } | Get-RustRecord -Name abc | Should -Be 'name abc tag  rest  internal false'
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
        # ValidatePattern uses RegexOptions.IgnoreCase by default,
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

Describe 'length, count, script, null and wildcard keys' {
    BeforeAll {
        # The binding error a call to Format-RustLabel raises.
        function Get-LabelError([scriptblock] $call) {
            try { & $call; $null } catch { $_ }
        }
    }

    It 'declares each attribute on its parameter' {
        $p = (Get-Command Format-RustLabel).Parameters
        $has = { param($name, $type) @($p[$name].Attributes | Where-Object { $_ -is $type }).Count }
        & $has Label ([System.Management.Automation.ValidateLengthAttribute]) | Should -Be 1
        & $has Tag ([System.Management.Automation.ValidateCountAttribute]) | Should -Be 1
        @($p['Size'].Attributes | Where-Object { $_.GetType().FullName -eq 'Pwrs.ValidateScriptTextAttribute' }).Count | Should -Be 1
        & $has Owner ([System.Management.Automation.ValidateNotNullAttribute]) | Should -Be 1
        & $has Note ([System.Management.Automation.AllowNullAttribute]) | Should -Be 1
        & $has Pattern ([System.Management.Automation.SupportsWildcardsAttribute]) | Should -Be 1
    }

    It 'binds arguments every key accepts' {
        Format-RustLabel box -Tag a, b -Size 4 -Owner me -Note $null -Pattern 'b*' |
            Should -Be 'box tags a,b size 4 owner System.String note null matches true'
    }

    It 'refuses a label longer than 8 characters' {
        $e = Get-LabelError { Format-RustLabel ninechars -Note n -ErrorAction Stop }
        $e.FullyQualifiedErrorId | Should -BeLike 'ParameterArgumentValidationError,*'
        $e.Exception.ParameterName | Should -Be 'Label'
    }

    It 'refuses more than 3 tags' {
        $e = Get-LabelError { Format-RustLabel box -Tag a, b, c, d -Note n -ErrorAction Stop }
        $e.FullyQualifiedErrorId | Should -BeLike 'ParameterArgumentValidationError,*'
        $e.Exception.ParameterName | Should -Be 'Tag'
    }

    It 'refuses an odd size with the message PowerShell''s ValidateScript gives' {
        $e = Get-LabelError { Format-RustLabel box -Size 3 -Note n -ErrorAction Stop }
        $e.FullyQualifiedErrorId | Should -BeLike 'ParameterArgumentValidationError,*'
        $e.Exception.ParameterName | Should -Be 'Size'
        $e.Exception.Message | Should -BeLike '*The "$_ % 2 -eq 0" validation script for the argument with value "3" did not return a result of True.*'
    }

    It 'refuses $null for -Owner' {
        $e = Get-LabelError { Format-RustLabel box -Owner $null -Note n -ErrorAction Stop }
        $e.FullyQualifiedErrorId | Should -BeLike 'ParameterArgumentValidationError,*'
        $e.Exception.ParameterName | Should -Be 'Owner'
    }

    It 'takes $null for the mandatory -Note, and refuses it for the mandatory -Label' {
        Format-RustLabel box -Note $null | Should -Be 'box tags  size 0 owner none note null matches none'
        # -Label's ValidateLength refuses $null before the mandatory check
        # does, so the id is the validator's.
        $e = Get-LabelError { Format-RustLabel $null -Note n -ErrorAction Stop }
        $e.FullyQualifiedErrorId | Should -BeLike 'ParameterArgumentValidationError*'
        $e.Exception.ParameterName | Should -Be 'Label'
    }

    It 'says in its help that -Pattern takes wildcards, and matches the label against it' {
        (Get-Help Format-RustLabel -Parameter Pattern).globbing | Should -Be 'true'
        (Get-Help Format-RustLabel -Parameter Label).globbing | Should -Be 'false'
        Format-RustLabel box -Note n -Pattern 'c*' | Should -Be 'box tags  size 0 owner none note n matches false'
    }
}
