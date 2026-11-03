# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Select-RustFirst, Invoke-RustEach and New-RustFile wrap Select-Object,
# ForEach-Object and New-Item through a steppable pipeline: each record
# piped to them is passed to the wrapped command, and what it writes is
# theirs. Each case is set beside the wrapped command run directly.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'A cmdlet stepping another command' {
    It 'writes what the wrapped command writes, record by record' {
        @(1..3 | Invoke-RustEach { $_ * 10 }) | Should -Be @(1..3 | ForEach-Object { $_ * 10 })
    }

    It 'steps the wrapped command once without a record when nothing is piped' {
        @(Invoke-RustEach { 'once' }) | Should -Be @(ForEach-Object { 'once' })
    }

    It 'ends quietly when the wrapped command stops the records passed to it' {
        @(1..5 | Select-RustFirst 2) | Should -Be @(1..5 | Select-Object -First 2)
    }

    It 'stops passing records on once a later command has what it needs' {
        $seen = [System.Collections.Generic.List[int]]::new()
        @(1..100000 | Invoke-RustEach { $seen.Add($_); $_ } | Select-Object -First 2) | Should -Be @(1, 2)
        $seen.Count | Should -Be 2
    }

    It 'ends with the exception the wrapped command throws, as running it directly does' {
        $direct = { 1..2 | ForEach-Object { throw "boom $_" } } | Should -Throw -PassThru
        $wrapped = { 1..2 | Invoke-RustEach { throw "boom $_" } } | Should -Throw -PassThru
        $wrapped.Exception.GetType().FullName | Should -Be $direct.Exception.GetType().FullName
        $wrapped.Exception.Message | Should -Be $direct.Exception.Message
        $wrapped.FullyQualifiedErrorId | Should -Be $direct.FullyQualifiedErrorId
    }

    It 'passes the -ErrorAction it was given on to the wrapped command' {
        # New-Item writes its own error for a file that is there already,
        # so -ErrorAction applies to it as it does to New-Item run directly.
        $path = Join-Path $TestDrive 'twice.txt'
        $null = New-Item -Path $path -ItemType File
        $direct = { New-Item -Path $path -ItemType File -ErrorAction Stop } | Should -Throw -PassThru
        $wrapped = { New-RustFile $path -ErrorAction Stop } | Should -Throw -PassThru
        $wrapped.Exception.GetType().FullName | Should -Be $direct.Exception.GetType().FullName
        $wrapped.FullyQualifiedErrorId | Should -Be $direct.FullyQualifiedErrorId
        { New-RustFile $path -ErrorAction SilentlyContinue } | Should -Not -Throw
    }

    It 'passes the -WhatIf it was given on to the wrapped command' {
        $path = Join-Path $TestDrive 'wrapped.txt'
        $out = @(New-RustFile $path -WhatIf)
        $out.Count | Should -Be 0
        Test-Path -LiteralPath $path | Should -BeFalse
        (New-RustFile $path).Name | Should -Be 'wrapped.txt'
        Test-Path -LiteralPath $path | Should -BeTrue
    }
}
