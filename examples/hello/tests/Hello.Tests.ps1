# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # Help text on one line. The formatter wraps a paragraph to the
    # host's width, so a phrase long enough to cross a wrap matches or
    # not depending on how wide the window running the suite is.
    function Get-HelpText($section) { (($section | Out-String) -replace '\s+', ' ').Trim() }
}

Describe 'Get-RustProperty' {
    It 'reads a note property off a PSCustomObject' {
        [pscustomobject]@{ Name = 'x'; Count = 3 } | Get-RustProperty -Name Name | Should -Be 'x'
    }

    It 'reads a property whose value is not a string' {
        [pscustomobject]@{ Name = 'x'; Count = 3 } | Get-RustProperty -Name Count | Should -Be 3
    }

    It 'reads a property off an object a Rust cmdlet wrote' {
        Get-Note -Text hi | Get-RustProperty -Name Text | Should -Be 'hi'
    }

    It 'errors on a name the object does not carry rather than writing null' {
        { [pscustomobject]@{ Name = 'x' } | Get-RustProperty -Name Missing -ErrorAction Stop } |
            Should -Throw
    }
}

Describe 'Measure-RustPropertyReads' {
    It 'reports one read per object per pass' {
        $items = @(1..5 | ForEach-Object { [pscustomobject]@{ Name = "item$_" } })
        Measure-RustPropertyReads -InputObject $items -Name Name -Passes 3 | Should -Be 15
    }

    It 'reads a CLR property, not only a note property' {
        $items = @([System.Version]::new(1, 2, 3, 4))
        Measure-RustPropertyReads -InputObject $items -Name Major -Passes 2 | Should -Be 2
    }

    It 'errors on a name the objects do not carry rather than counting the read' {
        $items = @([pscustomobject]@{ Name = 'x' })
        { Measure-RustPropertyReads -InputObject $items -Name Missing -Passes 1 -ErrorAction Stop } |
            Should -Throw
    }

    It 'refuses a pass count outside the declared range' {
        $items = @([pscustomobject]@{ Name = 'x' })
        { Measure-RustPropertyReads -InputObject $items -Name Name -Passes 0 -ErrorAction Stop } |
            Should -Throw
    }
}

Describe 'Measure-RustTypeReads' {
    It 'reports one ask per object per pass, by tag' {
        $items = @(1..5 | ForEach-Object { [pscustomobject]@{ Name = "item$_" } })
        Measure-RustTypeReads -InputObject $items -Passes 3 | Should -Be 15
    }

    It 'reports one ask per object per pass, by name' {
        $items = @([System.Version]::new(1, 2, 3, 4), [System.Version]::new(5, 6, 7, 8))
        Measure-RustTypeReads -InputObject $items -Passes 4 -ByName | Should -Be 8
    }

    It 'answers the same count by either route over mixed input' {
        $items = @(1, 'two', 3.0, [pscustomobject]@{ n = 4 })
        $byTag = Measure-RustTypeReads -InputObject $items -Passes 2
        $byName = Measure-RustTypeReads -InputObject $items -Passes 2 -ByName
        $byTag | Should -Be $byName
        $byTag | Should -Be 8
    }

    It 'refuses a pass count outside the declared range' {
        $items = @([pscustomobject]@{ Name = 'x' })
        { Measure-RustTypeReads -InputObject $items -Passes 0 -ErrorAction Stop } |
            Should -Throw
    }
}

Describe 'Write-RustStreams' {
    It 'writes a verbose record only when the stream is on' {
        @(Write-RustStreams -Message hi 4>&1 |
            Where-Object { $_ -is [System.Management.Automation.VerboseRecord] }).Count | Should -Be 0
        $records = Write-RustStreams -Message hi -Verbose 4>&1
        $verbose = @($records | Where-Object { $_ -is [System.Management.Automation.VerboseRecord] })
        $verbose.Count | Should -Be 1
        $verbose[0].Message | Should -Be 'verbose: hi'
    }

    It 'writes a warning record, which is on by default' {
        $records = Write-RustStreams -Message hi 3>&1
        $warnings = @($records | Where-Object { $_ -is [System.Management.Automation.WarningRecord] })
        $warnings.Count | Should -Be 1
        $warnings[0].Message | Should -Be 'warning: hi'
    }

    It 'writes a debug record only when the preference asks for it' {
        @(Write-RustStreams -Message hi 5>&1 |
            Where-Object { $_ -is [System.Management.Automation.DebugRecord] }).Count | Should -Be 0
        $DebugPreference = 'Continue'
        $records = Write-RustStreams -Message hi 5>&1
        $debug = @($records | Where-Object { $_ -is [System.Management.Automation.DebugRecord] })
        $debug.Count | Should -Be 1
        $debug[0].Message | Should -Be 'debug: hi'
    }

    It 'keeps a warning in -WarningVariable under -WarningAction SilentlyContinue, as Write-Warning does' {
        Write-RustStreams -Message hi -WarningAction SilentlyContinue -WarningVariable held 6>$null
        @($held).Count | Should -Be 1
        "$($held[0])" | Should -Be 'warning: hi'
        @(Write-RustStreams -Message hi -WarningAction SilentlyContinue 3>&1 6>$null |
            Where-Object { $_ -is [System.Management.Automation.WarningRecord] }).Count | Should -Be 0
    }

    It 'writes an information record under SilentlyContinue, as Write-Information does' {
        $InformationPreference = 'SilentlyContinue'
        $records = Write-RustStreams -Message hi 6>&1 3>$null
        $information = @($records | Where-Object { $_ -is [System.Management.Automation.InformationRecord] })
        $information.Count | Should -Be 1
        "$($information[0].MessageData)" | Should -Be 'information: hi'
        Write-RustStreams -Message hi -InformationVariable held 6>$null 3>$null
        @($held).Count | Should -Be 1
    }

    It 'writes no information record to a redirection under -InformationAction Ignore' {
        @(Write-RustStreams -Message hi -InformationAction Ignore 6>&1 3>$null |
            Where-Object { $_ -is [System.Management.Automation.InformationRecord] }).Count | Should -Be 0
    }
}

Describe 'Get-Greeting' {
    It 'greets once by default' {
        Get-Greeting -Name x | Should -Be 'Hello, x!'
    }

    It 'repeats with -Count' {
        @(Get-Greeting -Name x -Count 3).Count | Should -Be 3
    }

    It 'binds Name from the pipeline' {
        (@('a', 'b') | Get-Greeting) -join ',' | Should -Be 'Hello, a!,Hello, b!'
    }

    It 'binds Name positionally' {
        Get-Greeting y | Should -Be 'Hello, y!'
    }

    It 'writes to the verbose stream' {
        $records = Get-Greeting -Name x -Verbose 4>&1
        $verbose = @($records | Where-Object { $_ -is [System.Management.Automation.VerboseRecord] })
        $verbose.Count | Should -Be 1
        $verbose[0].Message | Should -Be 'greeting x'
    }

    It 'writes no verbose record when the stream is off' {
        $records = Get-Greeting -Name x 4>&1
        @($records | Where-Object { $_ -is [System.Management.Automation.VerboseRecord] }).Count | Should -Be 0
    }

    It 'writes to the verbose stream from the preference variable alone' {
        $VerbosePreference = 'Continue'
        $records = Get-Greeting -Name x 4>&1
        $verbose = @($records | Where-Object { $_ -is [System.Management.Automation.VerboseRecord] })
        $verbose.Count | Should -Be 1
        $verbose[0].Message | Should -Be 'greeting x'
    }

    It 'lets -Verbose:$false override the preference variable' {
        $VerbosePreference = 'Continue'
        $records = Get-Greeting -Name x -Verbose:$false 4>&1
        @($records | Where-Object { $_ -is [System.Management.Automation.VerboseRecord] }).Count | Should -Be 0
    }

    It 'writes a non-terminating error and continues' {
        $err = $null
        $out = @(Get-Greeting -Name x -Fail -ErrorAction SilentlyContinue -ErrorVariable err)
        $out.Count | Should -Be 0
        @($err).Count | Should -Be 1
        $err[0].FullyQualifiedErrorId | Should -Match 'GreetingRefused'
        $err[0].CategoryInfo.Category | Should -Be 'InvalidData'
    }

    It 'turns the error terminating under -ErrorAction Stop' {
        { Get-Greeting -Name x -Fail -ErrorAction Stop } | Should -Throw
    }

    It 'reports a panic as a terminating error and survives' {
        { Get-Greeting -Name x -Panic -ErrorAction Stop } | Should -Throw
        Get-Greeting -Name z | Should -Be 'Hello, z!'
    }

    It 'rejects Count outside the validated range' {
        { Get-Greeting -Name x -Count 0 } | Should -Throw
    }

    It 'has generated help' {
        $help = Get-Help Get-Greeting -ErrorAction Stop
        $help.Synopsis | Should -Be 'Writes a greeting for each name.'
        ($help.parameters.parameter | Where-Object Name -eq 'Name').Description.Text | Should -Be 'Who to greet.'
    }

    It 'renders the doc comment examples into help, and they run' {
        $help = Get-Help Get-Greeting -Examples -ErrorAction Stop
        $code = @($help.examples.example.code)
        $code.Count | Should -Be 3
        $code[0] | Should -Be 'Get-Greeting -Name World'
        $code[1] | Should -Be "'Ada', 'Bob' | Get-Greeting"
        $code[2] | Should -Be 'Get-Greeting -Name World -Count 3'
        # The examples are commands, so run them rather than trust the text.
        foreach ($line in $code) {
            { Invoke-Expression $line } | Should -Not -Throw
        }
        (Invoke-Expression $code[1]) | Should -Be @('Hello, Ada!', 'Hello, Bob!')
    }

    It 'keeps the synopsis and description clear of the examples section' {
        $help = Get-Help Get-Greeting -ErrorAction Stop
        $help.Synopsis | Should -Be 'Writes a greeting for each name.'
        Get-HelpText $help.description | Should -Match 'Greets once by default'
        Get-HelpText $help.description | Should -Not -Match 'Examples'
        Get-HelpText $help.description | Should -Not -Match 'Get-Greeting -Name World'
    }

    It 'takes the whole first paragraph as the synopsis' {
        $help = Get-Help Expand-RustText -ErrorAction Stop
        $help.Synopsis | Should -Be 'Repeats `Text` until it is at least `Width` characters long, then trims the result to exactly that many.'
        Get-HelpText $help.description | Should -Match 'this paragraph is the description'
        Expand-RustText -Text ab -Width 5 | Should -Be 'ababa'
    }

    It 'takes the manifest description, project URI and tags from Cargo' {
        $manifest = Import-PowerShellDataFile (Join-Path $env:PWRS_MODULE 'Hello.psd1')
        $manifest.Description | Should -Be 'The pwrs example module: every mechanism the framework offers, with a Pester test for each.'
        $manifest.PrivateData.PSData.ProjectUri | Should -Be 'https://github.com/Variably-Constant/PWRS'
        $manifest.PrivateData.PSData.Tags | Should -Contain 'pwrs'
        $manifest.PrivateData.PSData.Tags | Should -Contain 'powershell'
        $manifest.PrivateData.PSData.Tags | Should -Contain 'example'
    }

    It 'hands its shell the folder it was imported from' {
        # The shell runs from a staged copy, and the folder it takes its
        # native library from is the one this import's script handed it.
        $shell = [AppDomain]::CurrentDomain.GetAssemblies() | Where-Object { $_.GetName().Name -like 'Hello.Shell.*' } | Select-Object -Last 1
        $handed = $shell.GetType('Pwrs.Modules.Hello.PwrsModuleRoot', $true).GetField('Value').GetValue($null)
        [System.IO.Path]::GetFullPath($handed).TrimEnd('\', '/') | Should -Be ([System.IO.Path]::GetFullPath($env:PWRS_MODULE).TrimEnd('\', '/'))
    }

    It 'stops promptly when the downstream pipeline stops' {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $first = @(Get-Greeting -Name x -Count 1000000000 | Select-Object -First 2)
        $sw.Stop()
        $first.Count | Should -Be 2
        $sw.ElapsedMilliseconds | Should -BeLessThan 5000
    }
}
