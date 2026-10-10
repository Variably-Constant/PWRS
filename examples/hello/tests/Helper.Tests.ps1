# The helper executable Hello ships beside its native library: cargo pwrs
# build puts hello-helper in runtimes/<rid>/native/ because
# [package.metadata.pwrs] helpers names it, and the module starts it from
# the copy pwrs::helper_path stages for this process. PWRS_MODULE points at
# the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
    $script:moduleRoot = (Resolve-Path -LiteralPath $module).ProviderPath
    $file = if ([IO.Path]::DirectorySeparatorChar -eq '\') { 'hello-helper.exe' } else { 'hello-helper' }
    $script:shipped = @(Get-ChildItem -LiteralPath (Join-Path $module 'runtimes') -Recurse -Filter $file -File)

    # The error record a failing call raises, or $null when it succeeds.
    function script:Get-Failure([scriptblock]$call) {
        try { & $call; $null } catch { $_ }
    }
}

Describe 'A helper the module ships' {
    It 'is shipped in runtimes/<rid>/native/ beside the library' {
        $shipped.Count | Should -Be 1
        $shipped[0].Directory.Name | Should -Be 'native'
        $shipped[0].Directory.Parent.Parent.Name | Should -Be 'runtimes'
        @(Get-ChildItem -LiteralPath $shipped[0].DirectoryName -File | Where-Object Name -like '*pwrs_example_hello*').Count | Should -Be 1
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
            $rid = $shipped[0].Directory.Parent.Name
            $helper = [IO.Path]::Combine($copy, 'runtimes', $rid, 'native', $shipped[0].Name)
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
