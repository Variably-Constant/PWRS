# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Get-RustChildItem, Get-RustProcess, New-RustItem, Join-RustPath and
# Get-RustCommand are declared with #[cmdlet(wraps = "...")]: each
# declares the parameters its command has in the PowerShell running it
# and runs that command as a steppable pipeline. Each case is set beside
# the command run directly, so a case whose result differs between
# PowerShell versions, such as Join-Path's -ChildPath taking several
# strings from 7.6, passes in every version.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # A command's parameters without the common ones, sorted.
    function Get-OwnParameter([string] $Name) {
        $common = [System.Management.Automation.Cmdlet]::CommonParameters + [System.Management.Automation.Cmdlet]::OptionalCommonParameters
        @((Get-Command $Name).Parameters.Keys | Where-Object { $common -notcontains $_ } | Sort-Object)
    }

    # A process id no system issues: above Linux's and FreeBSD's largest,
    # and not a multiple of 4, as every Windows one is. Looking it up reads
    # one process entry, where a lookup by name reads the whole process
    # table, and .NET on FreeBSD fails that read with "sysctl 1 failed with
    # 3 error" when another process ends during it.
    $script:missingId = 2147483646

    # What a call wrote, as text, or what it threw.
    function Get-Outcome([scriptblock] $Call) {
        try { 'wrote ' + (@(& $Call) -join ',') }
        catch { 'threw ' + $_.Exception.GetType().FullName }
    }
}

Describe 'A cmdlet wrapping another command' {
    It 'declares every parameter its command has here' -TestCases @(
        @{ Wrapper = 'Get-RustProcess'; Command = 'Get-Process' }
        @{ Wrapper = 'New-RustItem'; Command = 'New-Item' }
        @{ Wrapper = 'Join-RustPath'; Command = 'Join-Path' }
        @{ Wrapper = 'Get-RustCommand'; Command = 'Get-Command' }
    ) {
        param($Wrapper, $Command)
        Get-OwnParameter $Wrapper | Should -Be (Get-OwnParameter $Command)
    }

    It 'adds its own parameters to the command''s, the provider''s dynamic ones included' {
        Push-Location $TestDrive
        try {
            Get-OwnParameter 'Get-RustChildItem' | Should -Be (@(Get-OwnParameter 'Get-ChildItem') + 'Upper' | Sort-Object)
        }
        finally { Pop-Location }
    }

    It 'binds an argument to the parameter set the command has for it' {
        (Get-RustProcess -Id $PID).Id | Should -Be $PID
    }

    It 'passes the command each record as it was piped' {
        @(Get-Process -Id $PID | Get-RustProcess).Id | Should -Be @($PID)
    }

    It 'forwards the provider''s -File and -Directory to the command' {
        New-Item -ItemType File -Path (Join-Path $TestDrive 'a.txt') | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $TestDrive 'sub') | Out-Null
        @(Get-RustChildItem -Path $TestDrive -File).Name | Should -Be @(Get-ChildItem -Path $TestDrive -File).Name
        @(Get-RustChildItem -Path $TestDrive -Directory).Name | Should -Be @(Get-ChildItem -Path $TestDrive -Directory).Name
    }

    It 'passes what the command writes through its output step' {
        New-Item -ItemType File -Path (Join-Path $TestDrive 'b.txt') -Force | Out-Null
        @(Get-RustChildItem -Path $TestDrive -File -Upper) | Should -Be @(Get-ChildItem -Path $TestDrive -File | ForEach-Object { $_.Name.ToUpperInvariant() })
    }

    It 'leaves -WhatIf and -Confirm to the command' {
        $target = Join-Path $TestDrive 'made.txt'
        New-RustItem -Path $target -ItemType File -WhatIf
        Test-Path $target | Should -BeFalse
        (New-RustItem -Path $target -ItemType File).Name | Should -Be 'made.txt'
    }

    It 'reports the command''s errors as the command does' -TestCases @(
        @{ Action = 'Continue' }
        @{ Action = 'SilentlyContinue' }
    ) {
        param($Action)
        $direct = $null
        $wrapped = $null
        Get-Process -Id $missingId -ErrorAction $Action -ErrorVariable direct 2>$null
        Get-RustProcess -Id $missingId -ErrorAction $Action -ErrorVariable wrapped 2>$null
        @($wrapped).Count | Should -Be @($direct).Count
        $wrapped[0].FullyQualifiedErrorId | Should -Be $direct[0].FullyQualifiedErrorId
    }

    It 'stops at the command''s error under -ErrorAction Stop' {
        $direct = Get-Outcome { Get-Process -Id $missingId -ErrorAction Stop }
        Get-Outcome { Get-RustProcess -Id $missingId -ErrorAction Stop } | Should -Be $direct
    }

    It 'takes what this PowerShell''s command takes, and refuses what it refuses' -TestCases @(
        @{ Wrapped = { Join-RustPath -Path a -ChildPath b, c }; Direct = { Join-Path -Path a -ChildPath b, c } }
        @{ Wrapped = { (Get-RustCommand -Name Get-Process -ExcludeModule pwrs-none).Name }; Direct = { (Get-Command -Name Get-Process -ExcludeModule pwrs-none).Name } }
    ) {
        param($Wrapped, $Direct)
        Get-Outcome $Wrapped | Should -Be (Get-Outcome $Direct)
    }

    It 'stops the command once a later command has what it needs' {
        @(Get-RustProcess | Select-Object -First 1).Count | Should -Be 1
    }
}

BeforeDiscovery {
    $hasArchive = [bool](Get-Command Microsoft.PowerShell.Archive\Compress-Archive -ErrorAction SilentlyContinue)
}

# Compress-RustArchive wraps a function of another module, whose
# declarations wrapped.lock.json records as they were read where the module
# is installed.
Describe 'A cmdlet wrapping another module''s command' {
    It 'declares the parameters its recorded copy has, which the installed command has too' -Skip:(-not $hasArchive) {
        Get-OwnParameter 'Compress-RustArchive' | Should -Be (Get-OwnParameter 'Compress-Archive')
    }

    It 'makes the archive the command makes, and none under -WhatIf' -Skip:(-not $hasArchive) {
        $source = Join-Path $TestDrive 'in.txt'
        Set-Content -Path $source -Value 'text'
        $zip = Join-Path $TestDrive 'out.zip'
        Compress-RustArchive -Path $source -DestinationPath $zip -WhatIf
        Test-Path $zip | Should -BeFalse
        Compress-RustArchive -Path $source -DestinationPath $zip
        Test-Path $zip | Should -BeTrue
    }
}
