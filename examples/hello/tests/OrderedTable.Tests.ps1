# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# New-RustOrderedTable writes the table PsHashtable::ordered makes:
# PowerShell 7's own System.Management.Automation.OrderedHashtable, and in
# Windows PowerShell 5.1, which has none, the runtime's
# Pwrs.OrderedHashtable. Each case is a use a script makes of a
# Hashtable, on the names alpha to zeta filled in that order with the
# values one to six. The answer is written out, and in PowerShell 7 the
# engine's own type, filled the same way, must give it as well.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    $names = 'alpha', 'beta', 'gamma', 'delta', 'epsilon', 'zeta'
    $values = 'one', 'two', 'three', 'four', 'five', 'six'
    $inOrder = 'alpha beta gamma delta epsilon zeta'

    # The module's table, and in PowerShell 7 the engine's own, filled the
    # same way.
    function Get-Tables {
        New-RustOrderedTable $names $values
        if ($PSVersionTable.PSEdition -eq 'Core') {
            $own = [System.Management.Automation.OrderedHashtable]::new()
            for ($i = 0; $i -lt $names.Count; $i++) { $own[$names[$i]] = $values[$i] }
            $own
        }
    }
    function Get-Six { param($alpha, $beta, $gamma, $delta, $epsilon, $zeta) "$alpha $beta $gamma $delta $epsilon $zeta" }
    function Get-HashtableCount { param([hashtable] $Table) $Table.Count }
    function Get-DictionaryCount { param([System.Collections.IDictionary] $Table) $Table.Count }
}

Describe 'an ordered table' {
    It 'is the engine''s own type in PowerShell 7 and the runtime''s in Windows PowerShell 5.1, one object down the pipeline' {
        $want = if ($PSVersionTable.PSEdition -eq 'Core') { 'System.Management.Automation.OrderedHashtable' } else { 'Pwrs.OrderedHashtable' }
        $written = @(New-RustOrderedTable $names $values)
        $written.Count | Should -Be 1
        $written[0].GetType().FullName | Should -Be $want
    }

    It 'reads a name by property and by index, and answers the questions a Hashtable does' {
        foreach ($t in @(Get-Tables)) {
            $kind = $t.GetType().FullName
            $t.gamma | Should -Be 'three' -Because $kind
            $t['gamma'] | Should -Be 'three' -Because $kind
            $t[2] | Should -BeNullOrEmpty -Because "$kind takes a number for a name, not a place"
            $t.ContainsKey('gamma') | Should -BeTrue -Because $kind
            $t.Contains('gamma') | Should -BeTrue -Because $kind
            $t.ContainsValue('three') | Should -BeTrue -Because $kind
            $t.Count | Should -Be 6 -Because $kind
        }
    }

    It 'lists its names in the order filled, wherever a script reads them' {
        foreach ($t in @(Get-Tables)) {
            $kind = $t.GetType().FullName
            @($t.Keys) -join ' ' | Should -Be $inOrder -Because $kind
            @($t.Values) -join ' ' | Should -Be 'one two three four five six' -Because $kind
            @($t.GetEnumerator() | ForEach-Object Key) -join ' ' | Should -Be $inOrder -Because $kind
            ($t | ConvertTo-Json -Compress) | Should -Be '{"alpha":"one","beta":"two","gamma":"three","delta":"four","epsilon":"five","zeta":"six"}' -Because $kind
            @(([pscustomobject]$t).psobject.Properties.Name) -join ' ' | Should -Be $inOrder -Because $kind
            @((New-Object psobject -Property $t).psobject.Properties.Name) -join ' ' | Should -Be $inOrder -Because $kind
        }
    }

    It 'is a Hashtable to a script: the type tests, parameters and splatting take it' {
        foreach ($t in @(Get-Tables)) {
            $kind = $t.GetType().FullName
            ($t -is [hashtable]) | Should -BeTrue -Because $kind
            ($t -is [System.Collections.IDictionary]) | Should -BeTrue -Because $kind
            Get-HashtableCount $t | Should -Be 6 -Because $kind
            Get-DictionaryCount $t | Should -Be 6 -Because $kind
            Get-Six @t | Should -Be 'one two three four five six' -Because $kind
        }
    }

    It 'clones in order, adds to a Hashtable of seven, and puts a new name last' {
        foreach ($t in @(Get-Tables)) {
            $kind = $t.GetType().FullName
            $clone = $t.Clone()
            $clone.GetType().FullName | Should -Be $kind
            @($clone.Keys) -join ' ' | Should -Be $inOrder -Because $kind
            $sum = $t + @{ eta = 'seven' }
            ($sum -is [hashtable]) | Should -BeTrue -Because $kind
            $sum.Count | Should -Be 7 -Because $kind
            $clone.eta = 'seven'
            @($clone.Keys) -join ' ' | Should -Be "$inOrder eta" -Because $kind
            $t.Count | Should -Be 6 -Because "$kind's clone is a table of its own"
        }
    }

    It 'compares names by case' {
        foreach ($t in @(Get-Tables)) {
            $kind = $t.GetType().FullName
            $t['GAMMA'] | Should -BeNullOrEmpty -Because $kind
            $t.ContainsKey('GAMMA') | Should -BeFalse -Because $kind
        }
    }

    It 'puts a name removed and set again last' {
        foreach ($t in @(Get-Tables)) {
            $kind = $t.GetType().FullName
            $t.Remove('alpha')
            $t['alpha'] = 'one again'
            @($t.Keys) -join ' ' | Should -Be 'beta gamma delta epsilon zeta alpha' -Because $kind
            $t.alpha | Should -Be 'one again' -Because $kind
        }
    }

    It 'refuses names and values of different counts' {
        { New-RustOrderedTable a, b one -ErrorAction Stop } | Should -Throw -ErrorId 'HelloTableShape*'
    }
}
