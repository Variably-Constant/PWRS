# Argument completers and dynamic parameters. PWRS_MODULE points at
# the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'Argument completer for Get-RustColor -Name' {
    It 'runs the cmdlet the completer completes' {
        # Completing a parameter does not run the command it belongs
        # to, so without this case the cmdlet itself is never called.
        Get-RustColor crimson | Should -Be 'color:crimson'
    }

    It 'completes a prefix over the known set' {
        $line = 'Get-RustColor -Name cr'
        $r = TabExpansion2 -inputScript $line -cursorColumn $line.Length
        $texts = $r.CompletionMatches.CompletionText
        $texts | Should -Contain 'crimson'
        $texts | Should -Not -Contain 'blue'
    }

    It 'offers every value for an empty word' {
        $line = 'Get-RustColor -Name '
        $r = TabExpansion2 -inputScript $line -cursorColumn $line.Length
        $r.CompletionMatches.CompletionText.Count | Should -Be 5
    }
}

Describe 'Dynamic parameters for Get-RustReading' {
    It 'adds -Unit only when -Kind is temperature' {
        (Get-Command Get-RustReading).Parameters.ContainsKey('Unit') | Should -BeFalse
        $r = Get-RustReading -Kind temperature -Unit C
        $r | Should -Be 'temperature:C'
    }

    It 'rejects -Unit for another kind' {
        { Get-RustReading -Kind pressure -Unit C -ErrorAction Stop } | Should -Throw
    }

    It 'validates the dynamic parameter set' {
        { Get-RustReading -Kind temperature -Unit K -ErrorAction Stop } | Should -Throw
    }

    It 'works with no dynamic parameter bound' {
        Get-RustReading -Kind pressure | Should -Be 'pressure:none'
    }

    It 'offers -Unit and its values to completion once -Kind is temperature' {
        $line = 'Get-RustReading -Kind temperature -U'
        (TabExpansion2 -inputScript $line -cursorColumn $line.Length).CompletionMatches.CompletionText | Should -Contain '-Unit'
        $line = 'Get-RustReading -Kind pressure -U'
        (TabExpansion2 -inputScript $line -cursorColumn $line.Length).CompletionMatches.CompletionText | Should -Not -Contain '-Unit'
        $line = 'Get-RustReading -Kind temperature -Unit '
        $values = (TabExpansion2 -inputScript $line -cursorColumn $line.Length).CompletionMatches.CompletionText
        $values | Should -Contain 'C'
        $values | Should -Contain 'F'
    }
}

Describe 'The controls the dynamic-parameter bench times Get-RustReading against' {
    It 'Get-RustStaticReading writes what Get-RustReading writes and implements no IDynamicParameters' {
        Get-RustStaticReading -Kind temperature | Should -Be (Get-RustReading -Kind temperature)
        (Get-Command Get-RustStaticReading).ImplementingType.GetInterfaces().Name | Should -Not -Contain 'IDynamicParameters'
        (Get-Command Get-RustReading).ImplementingType.GetInterfaces().Name | Should -Contain 'IDynamicParameters'
        { Get-RustStaticReading -Kind temperature -Unit C -ErrorAction Stop } | Should -Throw -ErrorId 'NamedParameterNotFound,Pwrs.Modules.Hello.GetRustStaticReadingCommand'
    }

    It 'Get-RustBlindReading has a hook that offers nothing, even for temperature' {
        Get-RustBlindReading -Kind temperature | Should -Be (Get-RustReading -Kind temperature)
        (Get-Command Get-RustBlindReading).ImplementingType.GetInterfaces().Name | Should -Contain 'IDynamicParameters'
        (Get-Command Get-RustBlindReading -ArgumentList temperature).Parameters.ContainsKey('Unit') | Should -BeFalse
        { Get-RustBlindReading -Kind temperature -Unit C -ErrorAction Stop } | Should -Throw -ErrorId 'NamedParameterNotFound,Pwrs.Modules.Hello.GetRustBlindReadingCommand'
    }
}
