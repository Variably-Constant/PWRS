# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE points
# at the built module folder.
#
# The attribute key: a .NET attribute on a cmdlet, a parameter, a class,
# a constructor, a property and a method, through hello's
# Get-RustOldGreeting, Get-RustSalute, Get-RustPreview and Hello.Badge.
BeforeDiscovery {
    $core = $PSVersionTable.PSEdition -eq 'Core'
}

BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # The text of each warning in $Records, in order.
    function Get-WarningText($Records) {
        @($Records | Where-Object { $_ -is [System.Management.Automation.WarningRecord] } | ForEach-Object { "$_" })
    }
}

Describe 'An obsolete cmdlet and parameter' {
    BeforeAll {
        # PowerShell's own warning for an obsolete command, to compare the
        # cmdlet's against.
        function Get-OldThing {
            [Obsolete('Use Get-Greeting.')]
            [CmdletBinding()]
            param()
        }
    }

    It 'warns on each call of an obsolete cmdlet, as PowerShell warns of an obsolete function' {
        Get-WarningText (Get-OldThing 3>&1) | Should -Be @("The command 'Get-OldThing' is obsolete. Use Get-Greeting.")
        $records = Get-RustOldGreeting -Name World 3>&1
        Get-WarningText $records | Should -Be @("The command 'Get-RustOldGreeting' is obsolete. Use Get-Greeting.")
        $records[-1] | Should -Be 'Hello, World!'
        Get-WarningText (1..2 | ForEach-Object { Get-RustOldGreeting -Name World 3>&1 }) | Should -HaveCount 2
    }

    It 'is silenced by -WarningAction SilentlyContinue and collected by -WarningVariable as the function''s warning is' {
        @(Get-RustOldGreeting -Name World -WarningAction SilentlyContinue 3>&1) | Should -Be @('Hello, World!')
        $function = $null
        $null = Get-OldThing -WarningVariable function 3>&1
        $cmdlet = $null
        $null = Get-RustOldGreeting -Name World -WarningVariable cmdlet 3>&1
        @($cmdlet).Count | Should -Be @($function).Count
    }

    It 'warns when an obsolete parameter binds, in PowerShell''s own words, after the command''s warning' {
        $records = Get-RustOldGreeting -Name World -Times 2 3>&1
        Get-WarningText $records | Should -Be @(
            "The command 'Get-RustOldGreeting' is obsolete. Use Get-Greeting.",
            "Parameter 'Times' is obsolete. Pipe the name once for each greeting."
        )
        @($records | Where-Object { $_ -is [string] }) | Should -Be @('Hello, World!', 'Hello, World!')
    }
}

Describe 'Validation and help from a parameter''s attributes' {
    It 'refuses a path on a drive [ValidateDrive] does not name, naming those it does' {
        { Get-RustSalute -Path Function:\prompt -ErrorAction Stop } | Should -Throw -ExpectedMessage '*drive Function *approved drives: Env,Variable*'
    }

    It 'takes a path on a drive [ValidateDrive] names' {
        Get-RustSalute -Path Env:\PATH | Should -Be 'Hello, Env:\PATH'
        Get-RustSalute -Path Variable:\HOME -Greeting Hi | Should -Be 'Hi, Variable:\HOME'
    }

    It 'gives the default [PSDefaultValue] declares in Get-Help, and None where there is none' {
        (Get-Help Get-RustSalute -Parameter Greeting).defaultValue | Should -Be 'Hello'
        (Get-Help Get-RustSalute -Parameter Path).defaultValue | Should -Be 'None'
    }
}

Describe 'An attribute only one shell''s references have' {
    It 'leaves an [Experimental] cmdlet out of PowerShell 7 while its feature is off' -Skip:(-not $core) {
        Get-Command Get-RustPreview -ErrorAction SilentlyContinue | Should -BeNullOrEmpty
    }

    It 'has the cmdlet in Windows PowerShell, whose shell leaves the attribute off' -Skip:$core {
        Get-RustPreview | Should -Be 'preview'
    }
}

Describe 'Attributes on a class and its members' {
    It 'puts each where reflection finds it' {
        $type = [Hello.Badge]
        $type.GetCustomAttributes([System.ComponentModel.DescriptionAttribute], $false).Description | Should -Be 'A badge with text on it.'
        $type.GetConstructor([type[]]@([string])).GetCustomAttributes([System.ComponentModel.DescriptionAttribute], $false).Description | Should -Be 'Makes a badge.'
        $type.GetProperty('Text').GetCustomAttributes([System.ComponentModel.DisplayNameAttribute], $false).DisplayName | Should -Be 'Badge text'
        $type.GetMethod('Shout').GetCustomAttributes([System.ObsoleteAttribute], $false).Message | Should -Be 'Read Text and change its case.'
    }

    It 'leaves the members working' {
        $badge = [Hello.Badge]::new('hi')
        $badge.Text | Should -Be 'hi'
        $badge.Shout() | Should -Be 'HI'
    }
}
