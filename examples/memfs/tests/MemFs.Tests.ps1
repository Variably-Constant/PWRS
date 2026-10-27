# The in-memory provider. PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'MemFs.psd1') -Force -ErrorAction Stop

    # A path given in its Windows form, as PowerShell writes it on this
    # system: each '\' stays on Windows and is '/' elsewhere.
    function ThisSystem([string] $path) { $path.Replace('\', [string][IO.Path]::DirectorySeparatorChar) }
    # A path given in its Windows form, written with the separator this
    # system does not write: '/' on Windows and '\' elsewhere.
    function OtherSeparator([string] $path) { if ([IO.Path]::DirectorySeparatorChar -eq '\') { $path.Replace('\', '/') } else { $path } }
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
        # docs has a.txt and sub from the listing test above.
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
        $i.FullName | Should -Be (ThisSystem 'mem:\shape.txt')
        $i.IsContainer | Should -BeFalse
        $i.Length | Should -Be 5
        $i.PSObject.Properties.Name | Should -Not -Contain 'Path'
    }

    It 'gives a second drive its own tree' {
        New-PSDrive -Name mem2 -PSProvider MemFs -Root 'ignored' -Scope Global | Out-Null
        (Get-PSDrive -Name mem2).Root | Should -Be (ThisSystem 'mem2:\')
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

    It 'renames a directory with everything under it' {
        New-Item -Path 'mem:\before' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\before\inside.txt' -Value 'kept'
        Rename-Item -Path 'mem:\before' -NewName 'after'
        Get-Content -Path 'mem:\after\inside.txt' | Should -Be 'kept'
        Test-Path 'mem:\before\inside.txt' | Should -BeFalse
    }

    It 'refuses to rename an item onto one that exists' {
        Set-Content -Path 'mem:\first.txt' -Value '1'
        Set-Content -Path 'mem:\second.txt' -Value '2'
        { Rename-Item -Path 'mem:\first.txt' -NewName 'second.txt' -ErrorAction Stop } | Should -Throw '*second.txt already exists*'
        Get-Content -Path 'mem:\second.txt' | Should -Be '2'
    }
}

# Every path names its drive, so an item piped from one command reaches the
# next, and the path cmdlets return what they return on a FileSystem drive:
# paths written with '\' on Windows and with '/' on Linux, macOS and
# FreeBSD, whichever separator a path is given with.
Describe 'MemFs paths' {
    BeforeAll {
        New-Item -Path 'mem:\paths' -ItemType Directory | Out-Null
        New-Item -Path 'mem:\paths\sub' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\paths\sub\deep.txt' -Value 'deep'
        Set-Content -Path 'mem:\paths\a.txt' -Value 'a'
        Set-ItemProperty -Path 'mem:\paths\a.txt' -Name Color -Value 'red'
        New-PSDrive -Name mem3 -PSProvider MemFs -Root 'ignored' -Scope Global | Out-Null
    }

    AfterAll {
        Remove-PSDrive -Name mem3 -Scope Global
    }

    It 'starts each drive''s root with the drive''s name' {
        (Get-PSDrive -Name mem).Root | Should -Be (ThisSystem 'mem:\')
        (Get-PSDrive -Name mem3).Root | Should -Be (ThisSystem 'mem3:\')
    }

    It 'puts the drive in every path of an item' {
        $i = Get-Item -Path 'mem:\paths\a.txt'
        $i.PSPath | Should -BeLike (ThisSystem '*MemFs::mem:\paths\a.txt')
        $i.PSParentPath | Should -BeLike (ThisSystem '*MemFs::mem:\paths')
        $i.PSChildName | Should -Be 'a.txt'
        $i.FullName | Should -Be (ThisSystem 'mem:\paths\a.txt')
        (Get-Item -Path 'mem3:\').FullName | Should -Be (ThisSystem 'mem3:\')
        (Get-Item -LiteralPath $i.PSPath).Name | Should -Be 'a.txt'
    }

    It 'passes piped items on to the next command' {
        @(Get-ChildItem -Path 'mem:\paths' | Get-Item | ForEach-Object Name | Sort-Object) | Should -Be @('a.txt', 'sub')
        (Get-Item -Path 'mem:\paths\a.txt' | Get-ItemProperty).Color | Should -Be 'red'
        Get-Item -Path 'mem:\paths\a.txt' | Get-Content | Should -Be 'a'
        Set-Content -Path 'mem:\paths\b.txt' -Value 'b'
        Get-Item -Path 'mem:\paths\b.txt' | Rename-Item -NewName 'c.txt'
        Test-Path 'mem:\paths\c.txt' | Should -BeTrue
        Get-Item -Path 'mem:\paths\c.txt' | Remove-Item
        Test-Path 'mem:\paths\c.txt' | Should -BeFalse
    }

    It 'passes on a piped item whose name has wildcard characters' {
        Set-Content -LiteralPath 'mem:\paths\w[1].txt' -Value 'one'
        Get-ChildItem -LiteralPath 'mem:\paths' | Where-Object Name -EQ 'w[1].txt' | Get-Content | Should -Be 'one'
        Remove-Item -LiteralPath 'mem:\paths\w[1].txt'
    }

    It 'takes a provider-qualified path to the drive it names' {
        Set-Content -Path 'mem3:\only.txt' -Value 'three'
        Get-Content -LiteralPath 'MemFs::mem3:\only.txt' | Should -Be 'three'
        Test-Path -LiteralPath 'MemFs::mem:\only.txt' | Should -BeFalse
    }

    It 'splits and joins paths' {
        Split-Path -Path 'mem:\paths\a.txt' -Parent | Should -Be (ThisSystem 'mem:\paths')
        Split-Path -Path 'mem:\paths' -Parent | Should -Be (ThisSystem 'mem:\')
        Split-Path -Path 'mem:\' -Parent | Should -Be ''
        Split-Path -Path 'mem:\paths\a.txt' -Leaf | Should -Be 'a.txt'
        Join-Path -Path 'mem:\paths' -ChildPath 'a.txt' | Should -Be (ThisSystem 'mem:\paths\a.txt')
    }

    It 'writes a path given with the other separator as this system writes paths' {
        Split-Path -Path (OtherSeparator 'mem:\paths\sub\deep.txt') -Parent | Should -Be (ThisSystem 'mem:\paths\sub')
        Split-Path -Path (OtherSeparator 'mem:\paths') -Parent | Should -Be (ThisSystem 'mem:\')
        Join-Path -Path (OtherSeparator 'mem:\paths\sub') -ChildPath 'deep.txt' | Should -Be (ThisSystem 'mem:\paths\sub\deep.txt')
        (Get-Item -Path (OtherSeparator 'mem:\paths\sub\deep.txt')).FullName | Should -Be (ThisSystem 'mem:\paths\sub\deep.txt')
        Push-Location (OtherSeparator 'mem:\paths\sub')
        try {
            (Get-Location).Path | Should -Be (ThisSystem 'mem:\paths\sub')
        } finally {
            Pop-Location
        }
    }

    It 'resolves paths against the location and moves up to the root' {
        Push-Location 'mem:\paths'
        try {
            (Get-Location).Path | Should -Be (ThisSystem 'mem:\paths')
            (Resolve-Path -Path '.\a.txt').Path | Should -Be (ThisSystem 'mem:\paths\a.txt')
            Resolve-Path -Path 'mem:\paths\sub\deep.txt' -Relative | Should -Be (ThisSystem '.\sub\deep.txt')
            Set-Location 'sub'
            Set-Location ..
            (Get-Location).Path | Should -Be (ThisSystem 'mem:\paths')
            Set-Location ..
            Set-Location ..
            (Get-Location).Path | Should -Be (ThisSystem 'mem:\')
        } finally {
            Pop-Location
        }
    }

    It 'filters with -Include and -Exclude' {
        @(Get-ChildItem -Path 'mem:\paths' -Include 'd*' -Recurse | ForEach-Object Name) | Should -Be @('deep.txt')
        @(Get-ChildItem -Path 'mem:\paths' -Exclude 'a*' | ForEach-Object Name) | Should -Be @('sub')
        # As on a FileSystem drive, a folder the wildcard matches is listed by the items in it.
        @(Get-ChildItem -Path 'mem:\paths\*' -Exclude 'a*' | ForEach-Object Name) | Should -Be @('deep.txt')
    }

    It 'copies an item with its properties, and a folder with -Recurse' {
        Copy-Item -Path 'mem:\paths\a.txt' -Destination 'mem:\paths\copy.txt'
        Get-Content -Path 'mem:\paths\copy.txt' | Should -Be 'a'
        (Get-ItemProperty -Path 'mem:\paths\copy.txt').Color | Should -Be 'red'
        Copy-Item -Path 'mem:\paths\sub' -Destination 'mem:\paths\sub2' -Recurse
        Get-Content -Path 'mem:\paths\sub2\deep.txt' | Should -Be 'deep'
    }

    It 'replaces a file with a copy of the same name, as on a FileSystem drive' {
        Set-Content -Path 'mem:\paths\other.txt' -Value 'other'
        Copy-Item -Path 'mem:\paths\other.txt' -Destination 'mem:\paths\copy.txt'
        Get-Content -Path 'mem:\paths\copy.txt' | Should -Be 'other'
    }

    It 'copies to another drive into that drive''s own tree' {
        Copy-Item -Path 'mem:\paths\a.txt' -Destination 'mem3:\a.txt'
        Get-Content -Path 'mem3:\a.txt' | Should -Be 'a'
        (Get-Item -Path 'mem3:\a.txt').FullName | Should -Be (ThisSystem 'mem3:\a.txt')
        Test-Path 'mem:\a.txt' | Should -BeFalse
        Copy-Item -Path 'mem:\paths\sub' -Destination 'mem3:\sub' -Recurse
        Get-Content -Path 'mem3:\sub\deep.txt' | Should -Be 'deep'
    }

    It 'copies and moves a property to an item on another drive' {
        Copy-ItemProperty -Path 'mem:\paths\a.txt' -Destination 'mem3:\only.txt' -Name Color
        (Get-ItemProperty -Path 'mem3:\only.txt').Color | Should -Be 'red'
        (Get-ItemProperty -Path 'mem:\paths\a.txt').Color | Should -Be 'red'
        Set-ItemProperty -Path 'mem:\paths\a.txt' -Name Size -Value 2
        Move-ItemProperty -Path 'mem:\paths\a.txt' -Destination 'mem3:\only.txt' -Name Size
        (Get-ItemProperty -Path 'mem3:\only.txt').Size | Should -Be 2
        (Get-ItemProperty -Path 'mem:\paths\a.txt').PSObject.Properties.Name | Should -Not -Contain 'Size'
    }
}

# Copy-Item and Move-Item follow a FileSystem drive's rules onto items that
# are already there, no change happens under -WhatIf, Invoke-Item counts
# its calls in a property, and Get-ChildItem -Depth walks as many levels as
# on a FileSystem drive.
Describe 'MemFs copies, moves, Invoke-Item and -Depth' {
    BeforeAll {
        New-Item -Path 'mem:\rules' -ItemType Directory | Out-Null
        New-PSDrive -Name mem4 -PSProvider MemFs -Root 'ignored' -Scope Global | Out-Null

        # The path of each item below the folder $root, sorted.
        function script:Below([string] $root, [object[]] $items) {
            @($items | ForEach-Object { $_.FullName.Substring($root.Length).TrimStart([char]'\', [char]'/') } | Sort-Object)
        }

        # Folders three deep under $root, on either drive, with a file in
        # $root and in each folder.
        function script:New-DepthTree([string] $root) {
            $d1 = Join-Path $root 'd1'
            $d2 = Join-Path $d1 'd2'
            $d3 = Join-Path $d2 'd3'
            foreach ($folder in $root, $d1, $d2, $d3) { New-Item -Path $folder -ItemType Directory | Out-Null }
            Set-Content -Path (Join-Path $root 'a.txt') -Value 'a'
            Set-Content -Path (Join-Path $d1 'b.txt') -Value 'b'
            Set-Content -Path (Join-Path $d2 'c.txt') -Value 'c'
            Set-Content -Path (Join-Path $d3 'e.txt') -Value 'e'
        }
    }

    AfterAll {
        Remove-PSDrive -Name mem4 -Scope Global
    }

    It 'copies onto a folder into it' {
        Set-Content -Path 'mem:\rules\a.txt' -Value 'a'
        New-Item -Path 'mem:\rules\into' -ItemType Directory | Out-Null
        Copy-Item -Path 'mem:\rules\a.txt' -Destination 'mem:\rules\into'
        Get-Content -Path 'mem:\rules\into\a.txt' | Should -Be 'a'
    }

    It 'copies a folder onto a folder into it, empty without -Recurse and whole with it' {
        New-Item -Path 'mem:\rules\src' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\rules\src\x.txt' -Value 'x'
        New-Item -Path 'mem:\rules\empty' -ItemType Directory | Out-Null
        New-Item -Path 'mem:\rules\whole' -ItemType Directory | Out-Null
        Copy-Item -Path 'mem:\rules\src' -Destination 'mem:\rules\empty'
        Test-Path 'mem:\rules\empty\src' | Should -BeTrue
        @(Get-ChildItem -Path 'mem:\rules\empty\src').Count | Should -Be 0
        Copy-Item -Path 'mem:\rules\src' -Destination 'mem:\rules\whole' -Recurse
        Get-Content -Path 'mem:\rules\whole\src\x.txt' | Should -Be 'x'
    }

    It 'refuses a folder copied onto a name already there, and a file copied onto a folder of its name' {
        { Copy-Item -Path 'mem:\rules\src' -Destination 'mem:\rules\whole' -Recurse -ErrorAction Stop } | Should -Throw '*src already exists*'
        New-Item -Path 'mem:\rules\named' -ItemType Directory | Out-Null
        New-Item -Path 'mem:\rules\named\a.txt' -ItemType Directory | Out-Null
        { Copy-Item -Path 'mem:\rules\a.txt' -Destination 'mem:\rules\named' -ErrorAction Stop } | Should -Throw '*a.txt is a directory*'
    }

    It 'moves a file with its properties, and writes it at its new path under -PassThru' {
        Set-Content -Path 'mem:\rules\m.txt' -Value 'm'
        Set-ItemProperty -Path 'mem:\rules\m.txt' -Name Color -Value 'red'
        $moved = Move-Item -Path 'mem:\rules\m.txt' -Destination 'mem:\rules\n.txt' -PassThru
        $moved.FullName | Should -Be (ThisSystem 'mem:\rules\n.txt')
        Test-Path 'mem:\rules\m.txt' | Should -BeFalse
        Get-Content -Path 'mem:\rules\n.txt' | Should -Be 'm'
        (Get-ItemProperty -Path 'mem:\rules\n.txt').Color | Should -Be 'red'
    }

    It 'moves a folder with everything under it into a folder that is there' {
        New-Item -Path 'mem:\rules\box' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\rules\box\in.txt' -Value 'in'
        New-Item -Path 'mem:\rules\shelf' -ItemType Directory | Out-Null
        Move-Item -Path 'mem:\rules\box' -Destination 'mem:\rules\shelf'
        Get-Content -Path 'mem:\rules\shelf\box\in.txt' | Should -Be 'in'
        Test-Path 'mem:\rules\box' | Should -BeFalse
    }

    It 'refuses a move onto an item that is there, and of a folder into itself' {
        Set-Content -Path 'mem:\rules\keep.txt' -Value 'keep'
        Set-Content -Path 'mem:\rules\other.txt' -Value 'other'
        { Move-Item -Path 'mem:\rules\other.txt' -Destination 'mem:\rules\keep.txt' -ErrorAction Stop } | Should -Throw '*keep.txt already exists*'
        Get-Content -Path 'mem:\rules\keep.txt' | Should -Be 'keep'
        New-Item -Path 'mem:\rules\shelf\box\inner' -ItemType Directory | Out-Null
        { Move-Item -Path 'mem:\rules\shelf' -Destination 'mem:\rules\shelf\box\inner' -ErrorAction Stop } | Should -Throw '*cannot be moved there*'
        Get-Content -Path 'mem:\rules\shelf\box\in.txt' | Should -Be 'in'
    }

    It 'leaves an item moved onto itself as it is' {
        Move-Item -Path 'mem:\rules\keep.txt' -Destination 'mem:\rules\keep.txt'
        Get-Content -Path 'mem:\rules\keep.txt' | Should -Be 'keep'
    }

    It 'moves to another drive into that drive''s own tree' {
        Set-Content -Path 'mem:\rules\travel.txt' -Value 'travel'
        Move-Item -Path 'mem:\rules\travel.txt' -Destination 'mem4:\travel.txt'
        Get-Content -Path 'mem4:\travel.txt' | Should -Be 'travel'
        Test-Path 'mem:\rules\travel.txt' | Should -BeFalse
        New-Item -Path 'mem:\rules\trip' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\rules\trip\bag.txt' -Value 'bag'
        Move-Item -Path 'mem:\rules\trip' -Destination 'mem4:\'
        Get-Content -Path 'mem4:\trip\bag.txt' | Should -Be 'bag'
        Test-Path 'mem:\rules\trip' | Should -BeFalse
    }

    It 'passes a piped item on to Move-Item' {
        Set-Content -Path 'mem:\rules\piped.txt' -Value 'p'
        Get-Item -Path 'mem:\rules\piped.txt' | Move-Item -Destination 'mem:\rules\shelf'
        Get-Content -Path 'mem:\rules\shelf\piped.txt' | Should -Be 'p'
    }

    It 'changes nothing under -WhatIf' {
        Set-Content -Path 'mem:\rules\w.txt' -Value 'w'
        New-Item -Path 'mem:\rules\w-new.txt' -ItemType File -WhatIf
        Copy-Item -Path 'mem:\rules\w.txt' -Destination 'mem:\rules\w-copy.txt' -WhatIf
        Copy-Item -Path 'mem:\rules\w.txt' -Destination 'mem4:\w-copy.txt' -WhatIf
        Move-Item -Path 'mem:\rules\w.txt' -Destination 'mem:\rules\w-moved.txt' -WhatIf
        Move-Item -Path 'mem:\rules\w.txt' -Destination 'mem4:\w-moved.txt' -WhatIf
        Rename-Item -Path 'mem:\rules\w.txt' -NewName 'w-renamed.txt' -WhatIf
        Clear-Content -Path 'mem:\rules\w.txt' -WhatIf
        Invoke-Item -Path 'mem:\rules\w.txt' -WhatIf
        Remove-Item -Path 'mem:\rules\w.txt' -WhatIf
        foreach ($path in 'mem:\rules\w-new.txt', 'mem:\rules\w-copy.txt', 'mem4:\w-copy.txt', 'mem:\rules\w-moved.txt', 'mem4:\w-moved.txt', 'mem:\rules\w-renamed.txt') {
            Test-Path $path | Should -BeFalse
        }
        Get-Content -Path 'mem:\rules\w.txt' | Should -Be 'w'
        @(Get-ItemProperty -Path 'mem:\rules\w.txt').Count | Should -Be 0
    }

    It 'counts each Invoke-Item in the Invocations property, and refuses a folder' {
        Set-Content -Path 'mem:\rules\run.txt' -Value 'run'
        Invoke-Item -Path 'mem:\rules\run.txt'
        Invoke-Item -Path 'mem:\rules\run.txt'
        (Get-ItemProperty -Path 'mem:\rules\run.txt').Invocations | Should -Be 2
        { Invoke-Item -Path 'mem:\rules\shelf' -ErrorAction Stop } | Should -Throw '*shelf is a directory, which has no default action*'
    }

    It 'walks as many levels under -Depth as a FileSystem drive does' {
        New-DepthTree 'mem:\depth'
        $fs = Join-Path $TestDrive 'depth'
        New-DepthTree $fs
        $fsRoot = (Get-Item -LiteralPath $fs).FullName
        $memRoot = (Get-Item -Path 'mem:\depth').FullName
        foreach ($depth in 0, 1, 2) {
            Below $memRoot @(Get-ChildItem -Path 'mem:\depth' -Depth $depth) | Should -Be (Below $fsRoot @(Get-ChildItem -LiteralPath $fs -Depth $depth))
        }
        @(Get-ChildItem -Path 'mem:\depth' -Depth 0 | ForEach-Object Name | Sort-Object) | Should -Be @('a.txt', 'd1')
        Below $memRoot @(Get-ChildItem -Path 'mem:\depth' -Recurse) | Should -Be (Below $fsRoot @(Get-ChildItem -LiteralPath $fs -Recurse))
    }

    It 'gives the names a FileSystem drive gives under -Name -Depth' {
        $fs = Join-Path $TestDrive 'depth'
        @(Get-ChildItem -Path 'mem:\depth' -Name -Depth 1 | Sort-Object) | Should -Be @(Get-ChildItem -LiteralPath $fs -Name -Depth 1 | Sort-Object)
    }
}

# -Filter selects by a name pattern or by a property's value, and a folder
# the filter leaves out is still searched, as on a FileSystem drive.
# -Credential, or the drive's New-PSDrive -Credential, names who made each
# new item. New-Item and Move-Item follow a FileSystem drive's rules onto
# items that are already there.
Describe 'MemFs filters, credentials and items already there' {
    BeforeAll {
        # The same tree on either drive.
        function script:New-FilterTree([string] $root) {
            $sub = Join-Path $root 'sub'
            foreach ($folder in $root, $sub) { New-Item -Path $folder -ItemType Directory | Out-Null }
            Set-Content -Path (Join-Path $root 'a.txt') -Value 'a'
            Set-Content -Path (Join-Path $root 'b.log') -Value 'b'
            Set-Content -Path (Join-Path $sub 'c.txt') -Value 'c'
            Set-Content -Path (Join-Path $sub 'd.log') -Value 'd'
        }

        # The path below $root of each item, or each name as given, sorted.
        function script:Rel([string] $root, [object[]] $items) {
            @($items | ForEach-Object { if ($_ -is [string]) { $_ } else { $_.FullName.Substring($root.Length).TrimStart([char]'\', [char]'/') } } | Sort-Object)
        }

        New-FilterTree 'mem:\sel'
        $script:fs = Join-Path $TestDrive 'sel'
        New-FilterTree $fs
        $script:fsRoot = (Get-Item -LiteralPath $fs).FullName
        $script:memRoot = (Get-Item -Path 'mem:\sel').FullName
        $password = New-Object System.Security.SecureString
        $password.AppendChar([char]'x')
        $script:cred = New-Object System.Management.Automation.PSCredential('ada', $password)
    }

    It 'filters by a name pattern as a FileSystem drive does, searching every folder' {
        Rel $memRoot @(Get-ChildItem -Path 'mem:\sel' -Filter '*.txt') | Should -Be (Rel $fsRoot @(Get-ChildItem -Path $fs -Filter '*.txt'))
        Rel $memRoot @(Get-ChildItem -Path 'mem:\sel' -Recurse -Filter '*.txt') | Should -Be (Rel $fsRoot @(Get-ChildItem -Path $fs -Recurse -Filter '*.txt'))
        Rel $memRoot @(Get-ChildItem -Path 'mem:\sel' -Depth 1 -Filter '*.txt') | Should -Be (Rel $fsRoot @(Get-ChildItem -Path $fs -Depth 1 -Filter '*.txt'))
        Rel $memRoot @(Get-ChildItem -Path 'mem:\sel' -Name -Recurse -Filter '*.txt') | Should -Be (Rel $fsRoot @(Get-ChildItem -Path $fs -Name -Recurse -Filter '*.txt'))
        Rel $memRoot @(Get-ChildItem -Path 'mem:\sel' -Recurse -Include 'c*' -Filter '*.txt') | Should -Be (Rel $fsRoot @(Get-ChildItem -Path $fs -Recurse -Include 'c*' -Filter '*.txt'))
    }

    It 'filters each level of a wildcard path as a FileSystem drive does' {
        Rel $memRoot @(Get-Item -Path 'mem:\sel\*' -Filter '*.txt') | Should -Be (Rel $fsRoot @(Get-Item -Path (Join-Path $fs '*') -Filter '*.txt'))
        Rel $memRoot @(Get-Item -Path 'mem:\sel\*\*.txt' -Filter 'c*') | Should -Be (Rel $fsRoot @(Get-Item -Path (Join-Path (Join-Path $fs '*') '*.txt') -Filter 'c*'))
        @(Get-Content -Path 'mem:\sel\*' -Filter '*.txt') | Should -Be @(Get-Content -Path (Join-Path $fs '*') -Filter '*.txt')
    }

    It 'matches a name pattern without regard to case' {
        @(Get-ChildItem -Path 'mem:\sel' -Filter '*.TXT' | ForEach-Object Name) | Should -Be @('a.txt')
    }

    It 'selects the items whose property has a value, without regard to case' {
        Set-ItemProperty -Path 'mem:\sel\a.txt' -Name Color -Value 'Red'
        Set-ItemProperty -Path 'mem:\sel\sub\d.log' -Name Color -Value 'red'
        Set-ItemProperty -Path 'mem:\sel\b.log' -Name Color -Value 'blue'
        @(Get-ChildItem -Path 'mem:\sel' -Recurse -Filter 'Color=red' | ForEach-Object Name | Sort-Object) | Should -Be @('a.txt', 'd.log')
    }

    It 'refuses a property filter with no property name' {
        { Get-ChildItem -Path 'mem:\sel' -Filter '=red' -ErrorAction Stop } | Should -Throw '*names no property before its =*'
    }

    It 'records the user of -Credential as the new item''s CreatedBy' {
        New-Item -Path 'mem:\sel\made.txt' -ItemType File -Credential $cred | Out-Null
        (Get-ItemProperty -Path 'mem:\sel\made.txt').CreatedBy | Should -Be 'ada'
        New-Item -Path 'mem:\sel\plain.txt' -ItemType File | Out-Null
        @(Get-ItemProperty -Path 'mem:\sel\plain.txt').Count | Should -Be 0
    }

    It 'records the user of the drive''s credential when a call gives none' {
        New-PSDrive -Name memc -PSProvider MemFs -Root 'ignored' -Credential $cred -Scope Global | Out-Null
        try {
            New-Item -Path 'memc:\x.txt' -ItemType File | Out-Null
            Set-Content -Path 'memc:\y.txt' -Value 'y'
            (Get-ItemProperty -Path 'memc:\x.txt').CreatedBy | Should -Be 'ada'
            (Get-ItemProperty -Path 'memc:\y.txt').CreatedBy | Should -Be 'ada'
        } finally {
            Remove-PSDrive -Name memc -Scope Global
        }
    }

    It 'refuses New-Item onto an item that is there without -Force' {
        Set-Content -Path 'mem:\sel\exists.txt' -Value 'old'
        { New-Item -Path 'mem:\sel\exists.txt' -ItemType File -Value 'new' -ErrorAction Stop } | Should -Throw '*exists.txt already exists*'
        { New-Item -Path 'mem:\sel\sub' -ItemType Directory -ErrorAction Stop } | Should -Throw '*sub already exists*'
        { New-Item -Path 'mem:\sel\exists.txt' -ItemType Directory -ErrorAction Stop } | Should -Throw '*exists.txt already exists*'
        Get-Content -Path 'mem:\sel\exists.txt' | Should -Be 'old'
    }

    It 'replaces a file under New-Item -Force, and returns a folder as it is' {
        New-Item -Path 'mem:\sel\exists.txt' -ItemType File -Value 'new' -Force | Out-Null
        Get-Content -Path 'mem:\sel\exists.txt' | Should -Be 'new'
        (New-Item -Path 'mem:\sel\sub' -ItemType Directory -Force).Name | Should -Be 'sub'
        @(Get-ChildItem -Path 'mem:\sel\sub' | ForEach-Object Name | Sort-Object) | Should -Be @('c.txt', 'd.log')
    }

    It 'never makes a file where a folder is, and changes nothing for a folder asked for where a file is' {
        { New-Item -Path 'mem:\sel\sub' -ItemType File -ErrorAction Stop } | Should -Throw '*sub is a directory*'
        { New-Item -Path 'mem:\sel\sub' -ItemType File -Force -ErrorAction Stop } | Should -Throw '*sub is a directory*'
        @(New-Item -Path 'mem:\sel\exists.txt' -ItemType Directory -Force).Count | Should -Be 0
        Get-Content -Path 'mem:\sel\exists.txt' | Should -Be 'new'
    }

    It 'replaces a file moved onto a file under -Force, on one drive and between two' {
        Set-Content -Path 'mem:\sel\m1.txt' -Value 'one'
        Set-Content -Path 'mem:\sel\m2.txt' -Value 'two'
        { Move-Item -Path 'mem:\sel\m1.txt' -Destination 'mem:\sel\m2.txt' -ErrorAction Stop } | Should -Throw '*m2.txt already exists*'
        Move-Item -Path 'mem:\sel\m1.txt' -Destination 'mem:\sel\m2.txt' -Force
        Get-Content -Path 'mem:\sel\m2.txt' | Should -Be 'one'
        Test-Path -Path 'mem:\sel\m1.txt' | Should -BeFalse
        New-PSDrive -Name memm -PSProvider MemFs -Root 'ignored' -Scope Global | Out-Null
        try {
            Set-Content -Path 'memm:\m.txt' -Value 'there'
            Move-Item -Path 'mem:\sel\m2.txt' -Destination 'memm:\m.txt' -Force
            Get-Content -Path 'memm:\m.txt' | Should -Be 'one'
            Test-Path -Path 'mem:\sel\m2.txt' | Should -BeFalse
        } finally {
            Remove-PSDrive -Name memm -Scope Global
        }
    }

    It 'refuses a folder moved onto a file, even with -Force' {
        New-Item -Path 'mem:\sel\box' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\sel\file.txt' -Value 'file'
        { Move-Item -Path 'mem:\sel\box' -Destination 'mem:\sel\file.txt' -Force -ErrorAction Stop } | Should -Throw '*file.txt already exists*'
        Test-Path -Path 'mem:\sel\box' | Should -BeTrue
    }
}

# MemNotes declares ExpandWildcards, so PowerShell passes a path with
# wildcards to its expand_path and every other path on as typed, and the
# provider itself matches names, applies -Include and -Exclude, and
# reports a note that is not there.
Describe 'MemNotes expanding wildcards' {
    BeforeAll {
        New-Item -Path 'notes:\todo' -Value 'buy milk' | Out-Null
        New-Item -Path 'notes:\trip' -Value 'pack' | Out-Null
        New-Item -Path 'notes:\shopping' -Value 'eggs' | Out-Null

        function script:Names([object[]] $items) { @($items | ForEach-Object Name | Sort-Object) }
    }

    It 'lists the notes on its notes: drive' {
        (Get-PSDrive -Name notes).Provider.Name | Should -Be 'MemNotes'
        Names @(Get-ChildItem -Path 'notes:') | Should -Be @('shopping', 'todo', 'trip')
    }

    It 'matches names by PowerShell''s wildcard rules, without regard to case' {
        Names @(Get-Item -Path 'notes:\t*') | Should -Be @('todo', 'trip')
        Names @(Get-Item -Path 'notes:\T*') | Should -Be @('todo', 'trip')
        Names @(Get-Item -Path 'notes:\tr?p') | Should -Be @('trip')
        Names @(Get-Item -Path 'notes:\[st]*') | Should -Be @('shopping', 'todo', 'trip')
    }

    It 'applies -Include and -Exclude to the notes a wildcard path names' {
        Names @(Get-Item -Path 'notes:\*' -Include 's*') | Should -Be @('shopping')
        Names @(Get-Item -Path 'notes:\t*' -Exclude 'trip') | Should -Be @('todo')
    }

    It 'applies -Include and -Exclude to a path without wildcards, as a FileSystem drive does' {
        @(Get-Item -Path 'notes:\todo' -Include 's*').Count | Should -Be 0
        Test-Path -Path 'notes:\todo' -Exclude 't*' | Should -BeFalse
        Test-Path -Path 'notes:\todo' | Should -BeTrue
    }

    It 'reports a note that is not there, and returns nothing for a pattern that matches none' {
        { Get-Item -Path 'notes:\nope' -ErrorAction Stop } | Should -Throw '*nope does not exist*'
        @(Get-Item -Path 'notes:\z*').Count | Should -Be 0
        Test-Path -Path 'notes:\z*' | Should -BeFalse
    }

    It 'resolves and reads the notes a wildcard path names' {
        @(Resolve-Path -Path 'notes:\t*' | ForEach-Object Path) | Should -Be @((ThisSystem 'notes:\todo'), (ThisSystem 'notes:\trip'))
        @(Get-Content -Path 'notes:\t*') | Should -Be @('buy milk', 'pack')
    }

    It 'replaces a note only under -Force, and changes one with Set-Item' {
        { New-Item -Path 'notes:\todo' -Value 'again' -ErrorAction Stop } | Should -Throw '*todo already exists*'
        New-Item -Path 'notes:\todo' -Value 'again' -Force | Out-Null
        Get-Content -Path 'notes:\todo' | Should -Be 'again'
        Set-Item -Path 'notes:\todo' -Value 'buy milk'
        Get-Content -Path 'notes:\todo' | Should -Be 'buy milk'
    }

    It 'refuses a name with wildcard characters or a backtick, and a folder' {
        { New-Item -Path 'notes:\a`[1`]' -Value 'x' -ErrorAction Stop } | Should -Throw '*which a note''s name cannot'
        { New-Item -Path 'notes:\box' -ItemType Directory -ErrorAction Stop } | Should -Throw '*no folders'
    }

    It 'removes the notes a wildcard path names, less those -Exclude leaves out' {
        Remove-Item -Path 'notes:\t*' -Exclude 'todo'
        Names @(Get-ChildItem -Path 'notes:') | Should -Be @('shopping', 'todo')
    }
}

# A provider method's error reaches PowerShell as a record with the Rust
# error's id and category, its message, and the path the command was on as
# its target. Whether the command goes on is PowerShell's, as for any
# provider.
Describe 'Provider errors' {
    BeforeAll {
        New-Item -Path 'mem:\err' -ItemType Directory | Out-Null
        New-Item -Path 'mem:\err\dir' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\err\a.txt' -Value 'a'

        # The record a command raises, or under -ErrorAction Stop the one it
        # writes.
        function script:Record([scriptblock] $command) {
            try {
                & $command
                $null
            } catch {
                $_
            }
        }
    }

    It 'gives the Rust id, category and message, with the path as target' {
        $r = Record { New-Item -Path 'mem:\err\a.txt' -ItemType File -ErrorAction Stop }
        $r.FullyQualifiedErrorId | Should -Be 'ItemExists,Microsoft.PowerShell.Commands.NewItemCommand'
        $r.CategoryInfo.Category | Should -Be 'ResourceExists'
        $r.TargetObject | Should -Be (ThisSystem 'mem:\err\a.txt')
        $r.Exception.Message | Should -Be 'err/a.txt already exists'
        $r.Exception.GetType().FullName | Should -Be 'Pwrs.PwrsException'
    }

    It 'gives them for an error of a property cmdlet' {
        $r = Record { Get-ItemProperty -Path 'mem:\err\a.txt' -Name Missing -ErrorAction Stop }
        $r.FullyQualifiedErrorId | Should -Be 'NoProperty,Microsoft.PowerShell.Commands.GetItemPropertyCommand'
        $r.CategoryInfo.Category | Should -Be 'ObjectNotFound'
        $r.TargetObject | Should -Be (ThisSystem 'mem:\err\a.txt')
    }

    It 'gives them through the content writer' {
        $r = Record { Set-Content -Path 'mem:\err\nope\x.txt' -Value 'x' -ErrorAction Stop }
        $r.FullyQualifiedErrorId | Should -Be 'NoParent,Microsoft.PowerShell.Commands.SetContentCommand'
        $r.CategoryInfo.Category | Should -Be 'ObjectNotFound'
        $r.TargetObject | Should -Be (ThisSystem 'mem:\err\nope\x.txt')
    }

    It 'names a method the provider lacks, and a path on no drive' {
        $lacks = Record { Clear-Item -Path 'mem:\err\a.txt' -ErrorAction Stop }
        $lacks.FullyQualifiedErrorId | Should -Be 'PwrsProviderUnsupported,Microsoft.PowerShell.Commands.ClearItemCommand'
        $lacks.CategoryInfo.Category | Should -Be 'NotImplemented'
        $lacks.Exception.Message | Should -Be 'this provider does not implement clear_item'
        $nowhere = Record { New-Item -Path 'MemFs::x' -ItemType File -ErrorAction Stop }
        $nowhere.FullyQualifiedErrorId | Should -Be 'PwrsProviderNoDrive,Microsoft.PowerShell.Commands.NewItemCommand'
        $nowhere.CategoryInfo.Category | Should -Be 'InvalidOperation'
    }

    It 'gives the ids of memfs and MemNotes' {
        (Record { Invoke-Item -Path 'mem:\err\dir' -ErrorAction Stop }).FullyQualifiedErrorId | Should -Be 'IsDirectory,Microsoft.PowerShell.Commands.InvokeItemCommand'
        (Record { Get-ChildItem -Path 'mem:\err' -Filter '=x' -ErrorAction Stop }).FullyQualifiedErrorId | Should -Be 'FilterWithoutName,Microsoft.PowerShell.Commands.GetChildItemCommand'
        $note = Record { Get-Item -Path 'notes:\nope' -ErrorAction Stop }
        $note.FullyQualifiedErrorId | Should -Be 'NoNote,Microsoft.PowerShell.Commands.GetItemCommand'
        $note.CategoryInfo.Category | Should -Be 'ObjectNotFound'
        $note.TargetObject | Should -Be (ThisSystem 'notes:\nope')
    }
}

# Each batch a command writes reaches Rust as it is written, so an error
# from it reaches the command.
Describe 'MemFs content' {
    BeforeAll {
        New-Item -Path 'mem:\text' -ItemType Directory | Out-Null
    }

    It 'replaces a file that exists with Set-Content' {
        Set-Content -Path 'mem:\text\a.txt' -Value 'one'
        Set-Content -Path 'mem:\text\a.txt' -Value 'two'
        Get-Content -Path 'mem:\text\a.txt' | Should -Be 'two'
    }

    It 'appends with Add-Content, and makes the file when there is none' {
        Add-Content -Path 'mem:\text\a.txt' -Value 'three'
        @(Get-Content -Path 'mem:\text\a.txt') | Should -Be @('two', 'three')
        Add-Content -Path 'mem:\text\new.txt' -Value 'first'
        Get-Content -Path 'mem:\text\new.txt' | Should -Be 'first'
    }

    It 'writes a file through the variable syntax, replacing what it had' {
        ${mem:\text\var.txt} = 'from a variable'
        ${mem:\text\var.txt} = 'again'
        Get-Content -Path 'mem:\text\var.txt' | Should -Be 'again'
    }

    It 'writes every piped line in order' {
        'p1', 'p2', 'p3' | Set-Content -Path 'mem:\text\piped.txt'
        'p4', 'p5' | Add-Content -Path 'mem:\text\piped.txt'
        @(Get-Content -Path 'mem:\text\piped.txt') | Should -Be @('p1', 'p2', 'p3', 'p4', 'p5')
    }

    It 'raises the error of a write under a missing parent' {
        { Set-Content -Path 'mem:\text\nope\x.txt' -Value 'x' -ErrorAction Stop } | Should -Throw '*parent of text/nope/x.txt does not exist*'
        Test-Path 'mem:\text\nope\x.txt' | Should -BeFalse
    }

    It 'raises the error of a write to a directory' {
        { Set-Content -Path 'mem:\text' -Value 'x' -ErrorAction Stop } | Should -Throw '*text is a directory*'
        { Add-Content -Path 'mem:\text' -Value 'x' -ErrorAction Stop } | Should -Throw '*text is a directory*'
    }
}

# Properties travel to and from Rust as name and value pairs, and each
# change runs under ShouldProcess.
Describe 'MemFs item properties' {
    BeforeAll {
        New-Item -Path 'mem:\props' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\props\a.txt' -Value 'a'
        Set-Content -Path 'mem:\props\b.txt' -Value 'b'
    }

    It 'writes nothing for an item with no properties' {
        @(Get-ItemProperty -Path 'mem:\props\a.txt').Count | Should -Be 0
    }

    It 'sets properties and reads them back with their types' {
        Set-ItemProperty -Path 'mem:\props\a.txt' -Name Color -Value 'red'
        Set-ItemProperty -Path 'mem:\props\a.txt' -Name Size -Value 5
        $p = Get-ItemProperty -Path 'mem:\props\a.txt'
        $p.Color | Should -Be 'red'
        $p.Size | Should -Be 5
        $p.Size | Should -BeOfType [int]
        $p.PSChildName | Should -Be 'a.txt'
    }

    It 'reads only the properties named, and one value with Get-ItemPropertyValue' {
        $p = Get-ItemProperty -Path 'mem:\props\a.txt' -Name Size
        @($p.PSObject.Properties.Name | Where-Object { $_ -notlike 'PS*' }) | Should -Be @('Size')
        Get-ItemPropertyValue -Path 'mem:\props\a.txt' -Name Color | Should -Be 'red'
    }

    It 'refuses a property the item does not have' {
        { Get-ItemProperty -Path 'mem:\props\a.txt' -Name Missing -ErrorAction Stop } | Should -Throw '*has no property Missing*'
    }

    It 'writes the property it set only under -PassThru' {
        (Set-ItemProperty -Path 'mem:\props\a.txt' -Name Color -Value 'blue' -PassThru).Color | Should -Be 'blue'
        @(Set-ItemProperty -Path 'mem:\props\a.txt' -Name Color -Value 'blue').Count | Should -Be 0
    }

    It 'changes nothing under -WhatIf' {
        Set-ItemProperty -Path 'mem:\props\a.txt' -Name Color -Value 'green' -WhatIf
        (Get-ItemProperty -Path 'mem:\props\a.txt').Color | Should -Be 'blue'
    }

    It 'clears a value and leaves the property in place' {
        Clear-ItemProperty -Path 'mem:\props\a.txt' -Name Size
        $p = Get-ItemProperty -Path 'mem:\props\a.txt'
        $p.PSObject.Properties.Name | Should -Contain 'Size'
        $p.Size | Should -BeNullOrEmpty
    }

    It 'adds a property, and replaces one that exists only under -Force' {
        (New-ItemProperty -Path 'mem:\props\b.txt' -Name Owner -Value 'ops').Owner | Should -Be 'ops'
        { New-ItemProperty -Path 'mem:\props\b.txt' -Name Owner -Value 'dev' -ErrorAction Stop } | Should -Throw '*already has a property Owner*'
        (New-ItemProperty -Path 'mem:\props\b.txt' -Name Owner -Value 'dev' -Force).Owner | Should -Be 'dev'
    }

    It 'refuses a property type, which memfs does not store' {
        { New-ItemProperty -Path 'mem:\props\b.txt' -Name Typed -Value 1 -PropertyType DWord -ErrorAction Stop } | Should -Throw '*no property types*'
    }

    It 'renames a property' {
        Rename-ItemProperty -Path 'mem:\props\b.txt' -Name Owner -NewName Team
        $p = Get-ItemProperty -Path 'mem:\props\b.txt'
        $p.Team | Should -Be 'dev'
        $p.PSObject.Properties.Name | Should -Not -Contain 'Owner'
    }

    It 'copies a property to another item and leaves it on the first' {
        Copy-ItemProperty -Path 'mem:\props\b.txt' -Destination 'mem:\props\a.txt' -Name Team
        (Get-ItemProperty -Path 'mem:\props\a.txt').Team | Should -Be 'dev'
        (Get-ItemProperty -Path 'mem:\props\b.txt').Team | Should -Be 'dev'
    }

    It 'moves a property to another item' {
        Move-ItemProperty -Path 'mem:\props\a.txt' -Destination 'mem:\props\b.txt' -Name Color
        (Get-ItemProperty -Path 'mem:\props\b.txt').Color | Should -Be 'blue'
        (Get-ItemProperty -Path 'mem:\props\a.txt').PSObject.Properties.Name | Should -Not -Contain 'Color'
    }

    It 'removes a property, and refuses one the item does not have' {
        Remove-ItemProperty -Path 'mem:\props\b.txt' -Name Color
        (Get-ItemProperty -Path 'mem:\props\b.txt').PSObject.Properties.Name | Should -Not -Contain 'Color'
        { Remove-ItemProperty -Path 'mem:\props\b.txt' -Name Color -ErrorAction Stop } | Should -Throw '*has no property Color*'
    }

    It 'moves the properties with a renamed item and drops them with the item' {
        Rename-Item -Path 'mem:\props\b.txt' -NewName 'c.txt'
        (Get-ItemProperty -Path 'mem:\props\c.txt').Team | Should -Be 'dev'
        Remove-Item -Path 'mem:\props\c.txt'
        Set-Content -Path 'mem:\props\c.txt' -Value 'new'
        @(Get-ItemProperty -Path 'mem:\props\c.txt').Count | Should -Be 0
    }
}

# Get-Acl and Set-Acl exist only on Windows, in both hosts. A memfs item's
# descriptor reaches PowerShell as SDDL text and the kind memfs names for
# it, built as the .NET object of that kind.
Describe 'MemFs security descriptors' -Skip:([IO.Path]::DirectorySeparatorChar -ne '\') {
    BeforeAll {
        New-Item -Path 'mem:\acl' -ItemType Directory | Out-Null
        Set-Content -Path 'mem:\acl\f.txt' -Value 'f'
        Set-Content -Path 'mem:\acl\g.txt' -Value 'g'
        $script:sd = $ExecutionContext.SessionState.InvokeProvider.SecurityDescriptor
        function script:Sid([System.Security.AccessControl.AuthorizationRule] $rule) {
            $rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
        }
    }

    It 'gives a file a FileSecurity with its owner, group and access list, and no audit list' {
        $acl = Get-Acl -Path 'mem:\acl\f.txt'
        $acl | Should -BeOfType [System.Security.AccessControl.FileSecurity]
        $acl.Sddl | Should -Be 'O:BAG:BAD:(A;;FR;;;WD)(A;;FA;;;BA)'
    }

    It 'gives a directory a DirectorySecurity' {
        Get-Acl -Path 'mem:\acl' | Should -BeOfType [System.Security.AccessControl.DirectorySecurity]
    }

    It 'names the rights of each rule' {
        $rights = @{}
        foreach ($rule in (Get-Acl -Path 'mem:\acl\f.txt').Access) { $rights[(Sid $rule)] = $rule.FileSystemRights.ToString() }
        $rights['S-1-5-32-544'] | Should -Be 'FullControl'
        $rights['S-1-1-0'] | Should -Be 'Read, Synchronize'
        (Get-Acl -Path 'mem:\acl\f.txt').AccessToString | Should -Match 'Allow  FullControl'
    }

    It 'adds the audit list under -Audit' {
        $acl = Get-Acl -Path 'mem:\acl\f.txt' -Audit
        $acl.Sddl | Should -BeLike '*S:(AU;FA;FW;;;WD)'
        @($acl.Audit).Count | Should -Be 1
    }

    It 'stores what Set-Acl sets and does not change the audit list the descriptor left out' {
        $acl = Get-Acl -Path 'mem:\acl\f.txt'
        $users = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-545')
        $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($users, 'Write', 'Allow'))
        Set-Acl -Path 'mem:\acl\f.txt' -AclObject $acl
        $after = Get-Acl -Path 'mem:\acl\f.txt' -Audit
        @($after.Access | Where-Object { (Sid $_) -eq 'S-1-5-32-545' }).FileSystemRights.ToString() | Should -BeLike 'Write*'
        $after.Sddl | Should -BeLike '*S:(AU;FA;FW;;;WD)'
    }

    It 'writes the descriptor under Set-Acl -Passthru, and sets nothing under -WhatIf' {
        Set-Acl -Path 'mem:\acl\g.txt' -AclObject (Get-Acl -Path 'mem:\acl\g.txt') -Passthru | Should -BeOfType [System.Security.AccessControl.FileSecurity]
        $acl = Get-Acl -Path 'mem:\acl\g.txt'
        $acl.SetOwner([System.Security.Principal.SecurityIdentifier]::new('S-1-5-18'))
        Set-Acl -Path 'mem:\acl\g.txt' -AclObject $acl -WhatIf
        (Get-Acl -Path 'mem:\acl\g.txt').Sddl | Should -BeLike 'O:BA*'
    }

    It 'makes an empty descriptor of each kind, by name without regard to case' {
        $sd.NewOfType('MemFs', 'File', 'All') | Should -BeOfType [System.Security.AccessControl.FileSecurity]
        $sd.NewOfType('MemFs', 'Directory', 'All') | Should -BeOfType [System.Security.AccessControl.DirectorySecurity]
        $sd.NewOfType('MemFs', 'RegistryKey', 'All') | Should -BeOfType [System.Security.AccessControl.RegistrySecurity]
        $sd.NewOfType('MemFs', 'other', 'All').GetType().FullName | Should -Be 'Pwrs.ItemSecurity'
        { $sd.NewOfType('MemFs', 'Printer', 'All') } | Should -Throw "*there is no security descriptor kind 'Printer'*"
    }

    It 'makes an empty descriptor of the kind an item has' {
        $sd.NewFromPath('mem:\acl\f.txt', 'All') | Should -BeOfType [System.Security.AccessControl.FileSecurity]
        $sd.NewFromPath('mem:\acl', 'All') | Should -BeOfType [System.Security.AccessControl.DirectorySecurity]
    }

    It 'gives the rights of an Other descriptor as a number, with its inheritance flags unchanged' {
        $other = $sd.NewOfType('MemFs', 'Other', 'All')
        $other.SetSecurityDescriptorSddlForm('O:BAG:BAD:(A;OICI;0x3;;;WD)')
        $rule = @($other.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))[0]
        $rule.Rights | Should -Be 3
        $rule.Rights | Should -BeOfType [int]
        $rule.IdentityReference.Value | Should -Be 'S-1-1-0'
        $rule.InheritanceFlags.ToString() | Should -Be 'ContainerInherit, ObjectInherit'
    }
}
