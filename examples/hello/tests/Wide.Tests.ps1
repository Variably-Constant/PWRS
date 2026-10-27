# A cmdlet, a constructor and a copied class with more items than one
# 64-bit word has bits for. Item i has bit i % 64 of word i / 64, so the
# 130 items of Hello.Wide take three words. PWRS_MODULE points at the
# built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # The numbers of the fields of a Hello.Wide that are set, in order.
    function Get-SetField($wide) {
        @(0..129 | Where-Object { $null -ne $wide."N$_" })
    }
}

Describe 'a cmdlet of more than 64 parameters' {
    It 'declares 130 parameters, each taking the property of its name from a piped object' {
        $p = (Get-Command New-RustWide).Parameters
        $byName = @(0..129 | Where-Object { $p["N$_"].ParameterSets.Values.ValueFromPipelineByPropertyName -contains $true })
        $byName.Count | Should -Be 130
    }

    It 'binds the first and last parameter of each word, and nothing else' {
        $w = New-RustWide -N0 1 -N63 2 -N64 3 -N127 4 -N128 5 -N129 6
        Get-SetField $w | Should -Be @(0, 63, 64, 127, 128, 129)
        $w.N0, $w.N63, $w.N64, $w.N127, $w.N128, $w.N129 | Should -Be @(1, 2, 3, 4, 5, 6)
    }

    It 'binds each record again when its only change is to a parameter in word <Word>' -TestCases @(
        @{ Word = 0; Name = 'N1' }
        @{ Word = 1; Name = 'N100' }
        @{ Word = 2; Name = 'N129' }
    ) {
        param($Name)
        $records = 1, 2, 3 | ForEach-Object { [pscustomobject]@{ $Name = $_ } }
        @($records | New-RustWide | ForEach-Object $Name) | Should -Be @(1, 2, 3)
    }

    It 'copies a Hello.Wide piped in, with what the command line changes' {
        $full = [Hello.Wide]::new.Invoke([object[]](0..129))
        $copy = $full | New-RustWide -N100 -1
        $want = @(0..129 | ForEach-Object { if ($_ -eq 100) { -1 } else { $_ } })
        @(0..129 | ForEach-Object { $copy."N$_" }) | Should -Be $want
    }
}

Describe 'a constructor of more than 64 arguments' {
    It 'takes all 130, in order' {
        $w = [Hello.Wide]::new.Invoke([object[]](0..129))
        @(0..129 | ForEach-Object { $w."N$_" }) | Should -Be @(0..129)
    }

    It 'leaves a field $null whose argument is $null or left out' {
        $w = [Hello.Wide]::new.Invoke([object[]](@($null) * 64 + @(7)))
        Get-SetField $w | Should -Be @(64)
        $w.N64 | Should -Be 7
        @(Get-SetField ([Hello.Wide]::new())).Count | Should -Be 0
    }
}
