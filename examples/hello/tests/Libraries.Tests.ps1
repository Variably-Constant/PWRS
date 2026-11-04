# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A library the `libraries` table names ships in the folder its entry
# gives, beside native under runtimes/<rid>, with its crates in the notices
# beside the libraries, and pwrs::library_folder stages that folder for the
# process. hello-plugin has no PWRS in it, as a plugin another program
# loads has none, so its test loads it as such a program would.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
    $script:runtime = @(Get-ChildItem -LiteralPath (Join-Path $module 'runtimes') -Directory)[0].FullName
    $script:folder = Join-Path $runtime 'plugins'
    $script:library = @(Get-ChildItem -LiteralPath $folder -File | Where-Object { $_.Extension -in '.dll', '.so', '.dylib' })
}

Describe 'a library of the libraries table' {
    It 'ships in the folder its entry names, and not beside the module''s own' {
        $library.Count | Should -Be 1
        $library[0].BaseName | Should -Match 'hello_plugin$'
        Test-Path -LiteralPath (Join-Path (Join-Path $runtime 'native') $library[0].Name) | Should -BeFalse
    }

    It 'has its crate in the notices beside the libraries' {
        $notices = Get-Content -Raw -LiteralPath (Join-Path $runtime 'THIRD-PARTY-NOTICES.txt')
        $notices | Should -Match "Third-party notices for $([regex]::Escape($library[0].Name)), a native library of the Hello module"
        $notices | Should -Match 'pwrs-example-hello-plugin'
    }
}

Describe 'pwrs::library_folder' {
    It 'stages the folder for this process, outside the module folder, with the shipped bytes' {
        $staged = Get-HelloLibraryFolder -Name plugins
        Split-Path -Leaf $staged | Should -Be 'plugins'
        $staged | Should -BeLike ([IO.Path]::Combine([AppDomain]::CurrentDomain.GetData('pwrs-load'), [string]$PID, '*'))
        $staged | Should -Not -BeLike "$module*"
        $copy = Join-Path $staged $library[0].Name
        (Get-FileHash -LiteralPath $copy).Hash | Should -Be (Get-FileHash -LiteralPath $library[0].FullName).Hash
        Get-HelloLibraryFolder -Name plugins | Should -Be $staged
    }

    It 'gives a folder a library loads and runs from' {
        $file = Join-Path (Get-HelloLibraryFolder -Name plugins) $library[0].Name
        $name = 'HelloPlugin' + [guid]::NewGuid().ToString('N')
        Add-Type -TypeDefinition @"
public static class $name {
    [System.Runtime.InteropServices.DllImport(@"$file")]
    public static extern int hello_plugin_answer();
}
"@
        ($name -as [type])::hello_plugin_answer() | Should -Be 42
    }

    It 'names the folder and where it looked when the module ships no such folder' {
        $e = $null
        try { Get-HelloLibraryFolder -Name nosuch -ErrorAction Stop } catch { $e = $_ }
        $e | Should -Not -BeNullOrEmpty
        $e.Exception.Message | Should -BeLike '*nosuch*not found in*runtimes*libraries*'
    }

    It 'refuses a name that is not a bare folder name' {
        $e = $null
        try { Get-HelloLibraryFolder -Name '../native' -ErrorAction Stop } catch { $e = $_ }
        $e | Should -Not -BeNullOrEmpty
        $e.Exception.Message | Should -BeLike '*without a path*'
    }
}
