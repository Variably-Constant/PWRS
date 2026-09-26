# A hand-written C# cmdlet under src/csharp, compiled into the shell
# and exported with the Rust cmdlets. PWRS_MODULE points at the built
# module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'hybrid C# cmdlet' {
    It 'runs beside the Rust cmdlets' {
        Get-RustHybrid -Name x | Should -Be 'hybrid x'
    }

    It 'exports a cmdlet alias into the session' {
        (Get-Command -Name nrtick).CommandType | Should -Be 'Alias'
        (Get-Command -Name nrtick).ResolvedCommand.Name | Should -Be 'New-RustTicker'
        (New-RustTicker -Label a).Label | Should -Be (nrtick -Label a).Label
        (Get-Module -Name Hello).ExportedAliases.Keys | Should -Contain 'grmn'
        grmn | Should -Be 'Hello'
    }

    It 'runs a cmdlet that declares no parameters' {
        Get-RustModuleName | Should -Be 'Hello'
    }

    It 'is exported by the manifest and the module' {
        (Get-Module -Name Hello).ExportedCmdlets.Keys | Should -Contain 'Get-RustHybrid'
        (Get-Command -Name Get-RustHybrid).CommandType | Should -Be 'Cmdlet'
        $manifest = Import-PowerShellDataFile (Join-Path $env:PWRS_MODULE 'Hello.psd1')
        $manifest.CmdletsToExport | Should -Contain 'Get-RustHybrid'
        $manifest.CmdletsToExport | Should -Contain 'Get-Greeting'
    }

    It 'reads the attribute forms a real compiler accepts, whichever order they are in' {
        Get-RustHybridInfo | Should -Be 'hybrid info module'
        Get-RustHybridInfo -Topic x | Should -Be 'hybrid info x'
        (Get-Module -Name Hello).ExportedCmdlets.Keys | Should -Contain 'Get-RustHybridInfo'
        (Get-Command -Name grhinfo).ResolvedCommand.Name | Should -Be 'Get-RustHybridInfo'
    }

    It 'orders the hand-written cmdlets by file name, not by directory order' {
        $manifest = Import-PowerShellDataFile (Join-Path $env:PWRS_MODULE 'Hello.psd1')
        $plain = [array]::IndexOf($manifest.CmdletsToExport, 'Get-RustHybrid')
        $info = [array]::IndexOf($manifest.CmdletsToExport, 'Get-RustHybridInfo')
        $plain | Should -BeGreaterThan -1
        $info | Should -BeGreaterThan $plain
    }

    It 'runs hand-written C# that uses System.Numerics.Vector of T over spans' {
        # 37 elements, so every lane width leaves a remainder for the
        # scalar loop to take.
        $left = [int[]](1..37)
        $right = [int[]](37..1)
        $expected = 0
        for ($i = 0; $i -lt 37; $i++) { $expected += $left[$i] * $right[$i] }
        $r = Measure-RustHybridDot -Left $left -Right $right
        $r.Sum | Should -Be $expected
        $r.Path | Should -BeIn 'Vector256', 'Vector', 'Scalar'
        if ($r.Path -eq 'Scalar') { $r.Lanes | Should -Be 0 } else { $r.Lanes | Should -BeGreaterThan 0 }
        { Measure-RustHybridDot -Left 1, 2 -Right 1 -ErrorAction Stop } | Should -Throw -ExpectedMessage '*same length*'
    }

    It 'compiles the intrinsics path into the PowerShell 7 half only' {
        # NET8_0_OR_GREATER guards the System.Runtime.Intrinsics path, as
        # a .NET SDK project guards it; the Windows PowerShell half is a
        # netstandard2.0 build, which does not define it.
        $r = Measure-RustHybridDot -Left 1, 2, 3 -Right 4, 5, 6
        $r.Sum | Should -Be 32
        $r.Intrinsics | Should -Be ($PSVersionTable.PSEdition -eq 'Core')
        if ($PSVersionTable.PSEdition -ne 'Core') { $r.Path | Should -Not -Be 'Vector256' }
    }

    It 'exports the alias the C# class declares, and leaves the parameter alias alone' {
        $manifest = Import-PowerShellDataFile (Join-Path $env:PWRS_MODULE 'Hello.psd1')
        $manifest.AliasesToExport | Should -Contain 'grhyb'
        $manifest.AliasesToExport | Should -Not -Contain 'n'
        (Get-Module -Name Hello).ExportedAliases.Keys | Should -Contain 'grhyb'
        (Get-Command -Name grhyb).ResolvedCommand.Name | Should -Be 'Get-RustHybrid'
        grhyb -Name y | Should -Be 'hybrid y'
        grhyb -n y | Should -Be 'hybrid y'
    }
}
