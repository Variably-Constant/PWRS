# The helper executable Hello ships beside its native library: cargo pwrs
# build puts hello-helper in runtimes/<rid>/native/ because
# [package.metadata.pwrs] helpers names it, on macOS inside the app bundle
# Hello Helper.app that macos-bundles declares, and the module starts it
# from the copy pwrs::helper_path stages for the module folder, in a folder
# every process of this user shares. PWRS_MODULE points at the built
# module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
    $script:moduleRoot = (Resolve-Path -LiteralPath $module).ProviderPath
    $file = if ([IO.Path]::DirectorySeparatorChar -eq '\') { 'hello-helper.exe' } else { 'hello-helper' }
    $script:shipped = @(Get-ChildItem -LiteralPath (Join-Path $module 'runtimes') -Recurse -Filter $file -File)
    # The library's folder, and the helper's path from it: in the bundle's
    # Contents/MacOS on macOS, beside the library elsewhere.
    $script:bundle = 'Hello Helper.app'
    $script:native = if ($IsMacOS) { $shipped[0].Directory.Parent.Parent.Parent } else { $shipped[0].Directory }
    $script:inNative = if ($IsMacOS) { [IO.Path]::Combine($bundle, 'Contents', 'MacOS', $file) } else { $file }

    # The error record a failing call raises, or $null when it succeeds.
    function script:Get-Failure([scriptblock]$call) {
        try { & $call; $null } catch { $_ }
    }
}

Describe 'A helper the module ships' {
    It 'is shipped in runtimes/<rid>/native/ beside the library' {
        $shipped.Count | Should -Be 1
        $shipped[0].FullName | Should -Be ([IO.Path]::Combine($native.FullName, $inNative))
        $native.Name | Should -Be 'native'
        $native.Parent.Parent.Name | Should -Be 'runtimes'
        @(Get-ChildItem -LiteralPath $native.FullName -File | Where-Object { $_.Name -like '*pwrs_example_hello*' -and $_.Extension -ne '.pdb' }).Count | Should -Be 1
    }

    It 'ships on macOS as the app bundle its manifest declares, with no bare executable beside it' -Skip:(-not $IsMacOS) {
        $plist = Get-Content -LiteralPath ([IO.Path]::Combine($native.FullName, $bundle, 'Contents', 'Info.plist')) -Raw
        $plist | Should -Match '<key>CFBundleIdentifier</key>\s*<string>io\.github\.variably-constant\.pwrs\.hello-helper</string>'
        $plist | Should -Match '<key>CFBundleExecutable</key>\s*<string>hello-helper</string>'
        $plist | Should -Match '<key>CFBundleName</key>\s*<string>Hello Helper</string>'
        Test-Path -LiteralPath ([IO.Path]::Combine($native.FullName, 'hello-helper')) | Should -BeFalse
    }

    It 'runs on macOS from a staged copy of its whole bundle, and reads the identifier the bundle declares' -Skip:(-not $IsMacOS) {
        $ran = Invoke-HelloHelper where
        $ran | Should -BeLike "*pwrs-load*helpers*$bundle.*$bundle/Contents/MacOS/hello-helper"
        Test-Path -LiteralPath (Join-Path (Split-Path (Split-Path $ran)) 'Info.plist') | Should -BeTrue
        Invoke-HelloHelper bundle | Should -Be 'io.github.variably-constant.pwrs.hello-helper'
    }

    It 'passes its arguments and writes what it prints' {
        Invoke-HelloHelper echo hello world | Should -Be 'hello world'
    }

    It 'runs from a copy staged for the module folder, outside it' {
        $ran = @(Invoke-HelloHelper where)
        $ran.Count | Should -Be 1
        $ran[0] | Should -Not -BeLike "$moduleRoot*"
        $ran[0] | Should -BeLike ([IO.Path]::Combine([AppDomain]::CurrentDomain.GetData('pwrs-load'), 'helpers', '*', '*hello-helper*'))
        $ran[0] | Should -Not -BeLike "*$([IO.Path]::DirectorySeparatorChar)$PID$([IO.Path]::DirectorySeparatorChar)*"
        Test-Path -LiteralPath $ran[0] | Should -BeTrue
    }

    It 'is staged once, so every run starts the same copy' {
        $first = Invoke-HelloHelper where
        Invoke-HelloHelper where | Should -Be $first
    }

    It 'starts from the same copy in another process, as Windows stores a notification icon''s settings by that path' {
        $exe = (Get-Process -Id $PID).Path
        $there = & $exe -NoProfile -NonInteractive -Command "Import-Module '$(Join-Path $moduleRoot 'Hello.psd1')'; Invoke-HelloHelper where"
        $there | Should -Be (Invoke-HelloHelper where)
    }

    It 'removes the copies its other builds staged, and no other folder' {
        $ran = Invoke-HelloHelper where
        # The folder named for this build: <file>.<mark>, containing the
        # executable, or on macOS the bundle it is inside.
        $copy = if ($IsMacOS) { Split-Path (Split-Path (Split-Path (Split-Path $ran))) } else { Split-Path $ran }
        $parent = Split-Path $copy
        $name = (Split-Path -Leaf $copy) -replace '\.[0-9a-f]+-[0-9a-f]+$', ''
        $older = Join-Path $parent "$name.1-2"
        $otherHelper = Join-Path $parent "$name.extra.1-2"
        $unmarked = Join-Path $parent "$name.not-a-mark"
        foreach ($folder in $older, $otherHelper, $unmarked) {
            $null = New-Item -ItemType Directory -Force -Path $folder
            Set-Content -LiteralPath (Join-Path $folder 'left.txt') -Value 'left'
        }
        try {
            # Each process removes them at its first start of the helper.
            $exe = (Get-Process -Id $PID).Path
            $null = & $exe -NoProfile -NonInteractive -Command "Import-Module '$(Join-Path $moduleRoot 'Hello.psd1')'; Invoke-HelloHelper where"
            Test-Path -LiteralPath $older | Should -BeFalse
            Test-Path -LiteralPath $otherHelper | Should -BeTrue
            Test-Path -LiteralPath $unmarked | Should -BeTrue
            Test-Path -LiteralPath $ran | Should -BeTrue
        } finally {
            foreach ($folder in $older, $otherHelper, $unmarked) {
                if (Test-Path -LiteralPath $folder) { Remove-Item -LiteralPath $folder -Recurse -Force }
            }
        }
    }

    It 'leaves the shipped file free while it runs' {
        $id = Start-HelloHelper
        try {
            (Get-Process -Id $id).Id | Should -Be $id
            # A running image's file refuses a writer that shares nothing,
            # so this open succeeds only because the helper runs elsewhere.
            $stream = [IO.File]::Open($shipped[0].FullName, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            $stream.Dispose()
        } finally {
            $released = @(Stop-HelloHelper)
        }
        $released | Should -Be @('released')
    }

    It 'starts from a copied module folder whose helper has no execute permission' {
        # A .nupkg records no Unix mode for its entries, so a module folder
        # can arrive with its helper not executable. Where files have a
        # mode, the copy's helper has every execute bit cleared.
        $copy = Join-Path ([IO.Path]::GetTempPath()) ('hello-copy-' + [Guid]::NewGuid().ToString('N'))
        Copy-Item -LiteralPath $moduleRoot -Destination $copy -Recurse
        try {
            $rid = $native.Parent.Name
            $helper = [IO.Path]::Combine($copy, 'runtimes', $rid, 'native', $inNative)
            Test-Path -LiteralPath $helper | Should -BeTrue
            if ([IO.Path]::DirectorySeparatorChar -ne '\') {
                & chmod a-x $helper
                ([IO.File]::GetUnixFileMode($helper) -band [IO.UnixFileMode]'UserExecute, GroupExecute, OtherExecute') | Should -Be 0
            }
            $exe = (Get-Process -Id $PID).Path
            $ran = & $exe -NoProfile -NonInteractive -Command "Import-Module '$(Join-Path $copy 'Hello.psd1')'; Invoke-HelloHelper echo from the copy"
            $ran | Should -Be 'from the copy'
        } finally {
            Remove-Item -LiteralPath $copy -Recurse -Force
        }
    }

    It 'names the helper and the folder searched when the module ships no such helper' {
        $e = Get-Failure { Invoke-HelloHelper -Name nosuch where -ErrorAction Stop }
        $e | Should -Not -BeNullOrEmpty
        $e.Exception.Message | Should -BeLike '*nosuch*not found in*native*'
    }

    It 'refuses a name that is not a bare file name' {
        $e = Get-Failure { Invoke-HelloHelper -Name '../hello-helper' where -ErrorAction Stop }
        $e | Should -Not -BeNullOrEmpty
        $e.Exception.Message | Should -BeLike '*without a folder*'
    }
}

# Outside Windows the temporary folder is often /tmp, which every user can
# write, so the module's script stages in pwrs-load-<uid> there only after
# stat shows a folder of this user's own, and clears group and other
# permissions from it. Each check of another folder runs in a pwsh of its
# own with TMPDIR pointing at a fresh folder, since a process reads the
# folder once.
Describe 'The staging folder outside Windows' -Skip:($IsWindows -or $PSVersionTable.PSEdition -ne 'Core') {
    BeforeAll {
        $probe = New-TemporaryFile
        try { $script:uid = (Get-Item -LiteralPath $probe.FullName).UnixStat.UserId } finally { Remove-Item -LiteralPath $probe.FullName }
        $script:exe = (Get-Process -Id $PID).Path
        $script:import = "Import-Module '$(Join-Path $moduleRoot 'Hello.psd1')' 3>&1; [AppDomain]::CurrentDomain.GetData('pwrs-load'); Invoke-HelloHelper where"
        # Runs the import in a pwsh whose temporary folder is $temp, and
        # returns what it wrote: any warning, the staging folder, then the
        # helper's path.
        function script:Invoke-Import([string]$temp) {
            & env "TMPDIR=$temp/" $exe -NoProfile -NonInteractive -Command $import
        }
    }

    It 'is pwrs-load-<uid>, owned by this user and closed to everyone else' {
        $load = [AppDomain]::CurrentDomain.GetData('pwrs-load')
        $load | Should -Be ([IO.Path]::Combine([IO.Path]::GetTempPath(), "pwrs-load-$uid"))
        $stat = (Get-Item -LiteralPath $load -Force).UnixStat
        $stat.UserId | Should -Be $uid
        $stat.Mode -band 63 | Should -Be 0
        Invoke-HelloHelper where | Should -BeLike "$load*"
    }

    It 'clears group and other permissions from a folder of this user''s own' {
        $temp = Join-Path ([IO.Path]::GetTempPath()) ('pwrs-tmp-' + [Guid]::NewGuid().ToString('N'))
        $mine = Join-Path $temp "pwrs-load-$uid"
        $null = New-Item -ItemType Directory -Path $mine
        & chmod 755 $mine
        try {
            $out = @(Invoke-Import $temp)
            $out[0] | Should -Be $mine
            $out[1] | Should -BeLike "$mine*hello-helper*"
            (Get-Item -LiteralPath $mine -Force).UnixStat.Mode -band 63 | Should -Be 0
        } finally {
            Remove-Item -LiteralPath $temp -Recurse -Force
        }
    }

    It 'stages in a private folder, with a warning, when that name is a link' {
        $temp = Join-Path ([IO.Path]::GetTempPath()) ('pwrs-tmp-' + [Guid]::NewGuid().ToString('N'))
        $elsewhere = Join-Path $temp 'elsewhere'
        $null = New-Item -ItemType Directory -Path $elsewhere
        $link = Join-Path $temp "pwrs-load-$uid"
        $null = New-Item -ItemType SymbolicLink -Path $link -Target $elsewhere
        try {
            $out = @(Invoke-Import $temp)
            ($out -join "`n") | Should -Match ([regex]::Escape($link) + ' is not a folder of this user''s own')
            $private = @($out | Where-Object { $_ -is [string] -and $_ -like "$temp/pwrs-load-*" -and $_ -notlike '*hello-helper*' })
            $private.Count | Should -Be 1
            $private[0] | Should -Not -Be $link
            (Get-Item -LiteralPath $private[0] -Force).UnixStat.Mode -band 63 | Should -Be 0
            $out[-1] | Should -BeLike "$($private[0])*hello-helper*"
            @(Get-ChildItem -LiteralPath $elsewhere).Count | Should -Be 0
        } finally {
            Remove-Item -LiteralPath $temp -Recurse -Force
        }
    }

    It 'stages in a private folder, with a warning that gives the error, when that name is a file' {
        $temp = Join-Path ([IO.Path]::GetTempPath()) ('pwrs-tmp-' + [Guid]::NewGuid().ToString('N'))
        $null = New-Item -ItemType Directory -Path $temp
        $file = Join-Path $temp "pwrs-load-$uid"
        Set-Content -LiteralPath $file -Value 'not a folder'
        try {
            $out = @(Invoke-Import $temp)
            ($out -join "`n") | Should -Match ([regex]::Escape($file) + ' is not a folder of this user''s own \(')
            $private = @($out | Where-Object { $_ -is [string] -and $_ -like "$temp/pwrs-load-*" -and $_ -notlike '*hello-helper*' })
            $private.Count | Should -Be 1
            $private[0] | Should -Not -Be $file
            $out[-1] | Should -BeLike "$($private[0])*hello-helper*"
            Get-Content -LiteralPath $file | Should -Be 'not a folder'
        } finally {
            Remove-Item -LiteralPath $temp -Recurse -Force
        }
    }
}

# pwrs::native_file_path stages a file in runtimes/<rid>/native/ by its own
# name, as [package.metadata.pwrs] native-files ships one; the library's own
# file is one every build ships, so Get-HelloNativeFile stages that one.
Describe 'A file the module ships beside its library' {
    It 'is staged for this process by its own name, outside the module folder, with the shipped bytes' {
        $library = @(Get-ChildItem -LiteralPath $native.FullName -File | Where-Object { $_.Name -like '*pwrs_example_hello*' -and $_.Extension -ne '.pdb' })[0]
        $staged = Get-HelloNativeFile
        Split-Path -Leaf $staged | Should -Be $library.Name
        $staged | Should -BeLike ([IO.Path]::Combine([AppDomain]::CurrentDomain.GetData('pwrs-load'), [string]$PID, '*'))
        $staged | Should -Not -BeLike "$moduleRoot*"
        (Get-FileHash -LiteralPath $staged).Hash | Should -Be (Get-FileHash -LiteralPath $library.FullName).Hash
        Get-HelloNativeFile | Should -Be $staged
    }

    It 'names the file and the folder searched when the module ships no such file' {
        $e = Get-Failure { Get-HelloNativeFile -Name nosuch.dll -ErrorAction Stop }
        $e | Should -Not -BeNullOrEmpty
        $e.Exception.Message | Should -BeLike '*nosuch.dll*not found in*native*native-files*'
    }

    It 'refuses a name that is not a bare file name' {
        $e = Get-Failure { Get-HelloNativeFile -Name '../Hello.psd1' -ErrorAction Stop }
        $e | Should -Not -BeNullOrEmpty
        $e.Exception.Message | Should -BeLike '*without a folder*'
    }
}
