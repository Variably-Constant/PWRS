# The calc example bundled inside the module: laid in by the build with
# the same runtime assemblies, imported into the session by the module's
# own script, left as it is when the session already holds it, and, since
# its entry sets on-import-failure = "warn", passed over with a warning
# when it cannot be imported. PWRS_MODULE points at the built module
# folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $bundled = Join-Path $module 'Calc'
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'a bundled module' {
    It 'is laid inside the module folder with its manifest, script and native library' {
        Test-Path (Join-Path $bundled 'Calc.psd1') | Should -BeTrue
        Test-Path (Join-Path $bundled 'Calc.psm1') | Should -BeTrue
        @(Get-ChildItem (Join-Path $bundled 'runtimes') -Recurse -File | Where-Object { $_.Name -like '*pwrs_example_calc*' }).Count | Should -Be 1
    }

    It 'carries the same runtime assemblies as the module bundling it, byte for byte' {
        foreach ($tfm in 'net10.0', 'netstandard2.0') {
            foreach ($file in 'Pwrs.Bootstrap.dll', 'Pwrs.Runtime.dll') {
                $outer = [System.IO.File]::ReadAllBytes((Join-Path $module (Join-Path $tfm $file)))
                $inner = [System.IO.File]::ReadAllBytes((Join-Path $bundled (Join-Path $tfm $file)))
                [System.Linq.Enumerable]::SequenceEqual($outer, $inner) | Should -BeTrue
            }
        }
    }

    It 'is imported into the session by the module, from the bundled folder, and its cmdlet runs' {
        $calc = Get-Module -Name Calc
        $calc | Should -Not -BeNullOrEmpty
        $calc.ModuleBase | Should -Be (Get-Item $bundled).FullName
        Add-CalcNumber 2 3 | Should -Be 5
    }

    It 'leaves a module the session already holds as it is' {
        # A child host of this edition imports the bundled module first,
        # then the module bundling it: one Calc, the same instance.
        $hostPath = (Get-Process -Id $PID).Path
        $script = "Import-Module '$(Join-Path $bundled 'Calc.psd1')' -ErrorAction Stop; " +
            "`$before = Get-Module -Name Calc; " +
            "Import-Module '$(Join-Path $module 'Hello.psd1')' -ErrorAction Stop; " +
            "`$after = @(Get-Module -Name Calc); " +
            "'count=' + `$after.Count; " +
            "'same=' + [object]::ReferenceEquals(`$before, `$after[0]); " +
            "'sum=' + (Add-CalcNumber 20 22); " +
            "'greeting=' + (Get-Greeting -Name bundled)"
        $out = @(& $hostPath -NoProfile -NonInteractive -Command $script 2>&1 | ForEach-Object { "$_" })
        $LASTEXITCODE | Should -Be 0
        $out | Should -Contain 'count=1'
        $out | Should -Contain 'same=True'
        $out | Should -Contain 'sum=42'
        $out | Should -Contain 'greeting=Hello, bundled!'
    }

    It 'is passed over with one warning naming it and its error when it cannot be imported, and the module imports without it' {
        # A copy of the module folder whose Calc has lost its native
        # library, imported in a child host of this edition.
        $copy = Join-Path $TestDrive 'Hello'
        Copy-Item -LiteralPath $module -Destination $copy -Recurse
        $library = @(Get-ChildItem -LiteralPath (Join-Path (Join-Path $copy 'Calc') 'runtimes') -Recurse -File | Where-Object { $_.Name -like '*pwrs_example_calc*' })
        $library.Count | Should -Be 1
        Remove-Item -LiteralPath $library[0].FullName
        $hostPath = (Get-Process -Id $PID).Path
        $script = "`$out = @(Import-Module '$(Join-Path $copy 'Hello.psd1')' -ErrorAction Stop 3>&1); " +
            "`$warnings = @(`$out | Where-Object { `$_ -is [System.Management.Automation.WarningRecord] }); " +
            "'warnings=' + `$warnings.Count; " +
            "foreach (`$w in `$warnings) { 'warning=' + `$w.Message }; " +
            "'calc=' + @(Get-Module -Name Calc).Count; " +
            "'greeting=' + (Get-Greeting -Name alone)"
        $out = @(& $hostPath -NoProfile -NonInteractive -Command $script 2>&1 | ForEach-Object { "$_" })
        $LASTEXITCODE | Should -Be 0 -Because ($out -join "`n")
        $out | Should -Contain 'warnings=1'
        @($out | Where-Object { $_ -like 'warning=Hello imports without its bundled module Calc, whose import failed: *' }).Count | Should -Be 1 -Because ($out -join "`n")
        $out | Should -Contain 'calc=0'
        $out | Should -Contain 'greeting=Hello, alone!'
    }
}
