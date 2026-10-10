# Class objects accepted back as parameters and nested as fields.
# PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'class-typed parameters and nested class fields' {
    It 'accepts copied objects and nests them in another copied class' {
        $ada = Get-Person -Name Ada -Age 36 -Tag math, code -Score 9.5
        $bob = Get-Person -Name Bob
        $t = New-RustTeam -Lead $ada -Member $ada, $bob
        $t.GetType().FullName | Should -Be 'Hello.Team'
        $t.Lead.GetType().FullName | Should -Be 'Hello.Person'
        $t.Lead.Name | Should -Be 'Ada'
        $t.Lead.Tags | Should -Be @('math', 'code')
        $t.Lead.Score | Should -Be 9.5
        $t.Members.GetType().Name | Should -Be 'Person[]'
        $t.Members.Count | Should -Be 2
        $t.Members[1].Name | Should -Be 'Bob'
        $t.Members[1].Score | Should -BeNullOrEmpty
        $t.Note | Should -BeNullOrEmpty
    }

    It 'reads a nested team back through the pipeline' {
        $t = New-RustTeam -Lead (Get-Person -Name Cy) -Member (Get-Person -Name Di)
        $t | Get-RustTeamSummary | Should -Be 'Cy:1:none'
        Get-RustTeamSummary -Team $t | Should -Be 'Cy:1:none'
    }

    It 'carries a psobject-mode class as an optional field and parameter' {
        $n = Get-Note -Text pinned -Priority 2
        $t = New-RustTeam -Lead (Get-Person -Name Eve) -Note $n
        $t.Note.PSObject.TypeNames[0] | Should -Be 'Hello.Note'
        $t.Note.Text | Should -Be 'pinned'
        Get-RustTeamSummary -Team $t | Should -Be 'Eve:0:pinned'
    }

    It 'reads a proxy object back into Rust' {
        $c = New-Counter -Label ticks -Value 4
        $null = $c.Advance(3)
        Get-RustCounterText -Counter $c | Should -Be 'ticks=7'
    }

    It 'rejects a value that is not the class' {
        { New-RustTeam -Lead 'not a person' -ErrorAction Stop } | Should -Throw
    }
}
