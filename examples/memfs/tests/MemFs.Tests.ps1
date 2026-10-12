# The in-memory provider. PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'MemFs.psd1') -Force -ErrorAction Stop
}

Describe 'MemFs provider' {
    It 'registers the provider' {
        (Get-PSProvider -PSProvider MemFs).Name | Should -Be 'MemFs'
    }

    It 'has a default mem: drive' {
        (Get-PSDrive -Name mem).Provider.Name | Should -Be 'MemFs'
    }

    It 'creates directories and files and lists children' {
        New-Item -Path 'mem:\docs' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\docs\a.txt' -Value 'hello'
        New-Item -Path 'mem:\docs\sub' -ItemType Directory | Out-Null
        $names = (Get-ChildItem -Path 'mem:\docs' | ForEach-Object Name | Sort-Object)
        $names | Should -Be @('a.txt', 'sub')
    }

    It 'reads content back' {
        Set-Content -Path 'mem:\note.txt' -Value 'one', 'two'
        (Get-Content -Path 'mem:\note.txt') -join ',' | Should -Be 'one,two'
    }

    It 'reads a directory through the variable syntax as one object whose note properties survive' {
        # docs holds a.txt and sub from the listing test above.
        $dir = $mem:docs
        $dir.Name | Should -Be 'docs'
        $dir.Entries | Should -Be 2
        $dir.PSObject.TypeNames[0] | Should -Be 'Pwrs.MemDir'
    }

    It 'tests item existence' {
        Test-Path 'mem:\note.txt' | Should -BeTrue
        Test-Path 'mem:\nope.txt' | Should -BeFalse
    }

    It 'recurses' {
        New-Item -Path 'mem:\tree' -ItemType Directory | Out-Null
        New-Item -Path 'mem:\tree\deep' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\tree\deep\leaf.txt' -Value 'x'
        $all = (Get-ChildItem -Path 'mem:\tree' -Recurse | ForEach-Object Name | Sort-Object)
        $all | Should -Contain 'deep'
        $all | Should -Contain 'leaf.txt'
    }

    It 'renames an item' {
        Set-Content -Path 'mem:\old.txt' -Value 'v'
        Rename-Item -Path 'mem:\old.txt' -NewName 'new.txt'
        Test-Path 'mem:\old.txt' | Should -BeFalse
        Test-Path 'mem:\new.txt' | Should -BeTrue
    }

    It 'removes a tree with -Recurse' {
        New-Item -Path 'mem:\gone' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\gone\x.txt' -Value 'x'
        Remove-Item -Path 'mem:\gone' -Recurse
        Test-Path 'mem:\gone' | Should -BeFalse
    }

    It 'exposes the item object shape' {
        Set-Content -Path 'mem:\shape.txt' -Value 'abcde'
        $i = Get-Item -Path 'mem:\shape.txt'
        $i.Name | Should -Be 'shape.txt'
        $i.IsContainer | Should -BeFalse
        $i.Length | Should -Be 5
    }

    It 'gives a second drive its own tree' {
        New-PSDrive -Name mem2 -PSProvider MemFs -Root 'ignored' -Scope Global | Out-Null
        (Get-PSDrive -Name mem2).Root | Should -Be ''
        Set-Content -Path 'mem2:\only.txt' -Value 'here'
        Test-Path 'mem2:\only.txt' | Should -BeTrue
        Test-Path 'mem:\only.txt' | Should -BeFalse
        Test-Path 'mem2:\note.txt' | Should -BeFalse
    }

    It 'drops the tree with the drive on Remove-PSDrive' {
        Remove-PSDrive -Name mem2 -Scope Global
        { Get-PSDrive -Name mem2 -ErrorAction Stop } | Should -Throw
        New-PSDrive -Name mem2 -PSProvider MemFs -Root 'ignored' -Scope Global | Out-Null
        Test-Path 'mem2:\only.txt' | Should -BeFalse
        Remove-PSDrive -Name mem2 -Scope Global
    }
}
