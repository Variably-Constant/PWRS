# The helper executable Hello ships beside its native library: cargo pwrs
# build puts hello-helper in runtimes/<rid>/native/ because
# [package.metadata.pwrs] helpers names it, on macOS inside the app bundle
# Hello Helper.app that macos-bundles declares, and the module starts it
# from the copy pwrs::helper_path stages for this process. PWRS_MODULE
# points at the built module folder.
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
        @(Get-ChildItem -LiteralPath $native.FullName -File | Where-Object Name -like '*pwrs_example_hello*').Count | Should -Be 1
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
        $ran | Should -BeLike "*pwrs-load*$PID*native*$bundle.*$bundle/Contents/MacOS/hello-helper"
        Test-Path -LiteralPath (Join-Path (Split-Path (Split-Path $ran)) 'Info.plist') | Should -BeTrue
        Invoke-HelloHelper bundle | Should -Be 'io.github.variably-constant.pwrs.hello-helper'
    }

    It 'passes its arguments and writes what it prints' {
        Invoke-HelloHelper echo hello world | Should -Be 'hello world'
    }

    It 'runs from a copy staged for this process, outside the module folder' {
        $ran = @(Invoke-HelloHelper where)
        $ran.Count | Should -Be 1
        $ran[0] | Should -Not -BeLike "$moduleRoot*"
        $ran[0] | Should -BeLike "*pwrs-load*$PID*hello-helper*"
        Test-Path -LiteralPath $ran[0] | Should -BeTrue
    }

    It 'is staged once, so every run starts the same copy' {
        $first = Invoke-HelloHelper where
        Invoke-HelloHelper where | Should -Be $first
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
        # can arrive with its helper not executable. Where files carry a
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

# pwrs::native_file_path stages a file runtimes/<rid>/native/ holds by its own
# name, as [package.metadata.pwrs] native-files ships one; the library's own
# file is one every build ships, so Get-HelloNativeFile asks for it.
Describe 'A file the module ships beside its library' {
    It 'is staged for this process by its own name, outside the module folder, with the shipped bytes' {
        $library = @(Get-ChildItem -LiteralPath $native.FullName -File | Where-Object Name -like '*pwrs_example_hello*')[0]
        $staged = Get-HelloNativeFile
        Split-Path -Leaf $staged | Should -Be $library.Name
        $staged | Should -BeLike ([IO.Path]::Combine([IO.Path]::GetTempPath(), 'pwrs-load', '*'))
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
