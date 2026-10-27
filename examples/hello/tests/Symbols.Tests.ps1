# The symbol files a build writes and what the loader copies of them: a
# debug build, or one with --debug-symbols, writes a .pdb for each .NET
# assembly and, on Windows, for the Rust library and the helper, and each
# goes beside the staged copy of its file. .NET reads a stack trace's file
# and line from there, and a debugger that searches a library's own folder
# finds the Rust symbols. A build without symbols has no .pdb, and the
# staged folders then have none. PWRS_MODULE points at the built module
# folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
    $tfm = if ($PSVersionTable.PSEdition -eq 'Core') { 'net10.0' } else { 'netstandard2.0' }
    $script:managed = Join-Path $module $tfm
    $script:symbols = Test-Path -LiteralPath (Join-Path $managed 'Pwrs.Runtime.pdb')
    $script:shell = [AppDomain]::CurrentDomain.GetAssemblies() | Where-Object { $_.GetName().Name -like 'Hello.Shell.*' } | Select-Object -Last 1
    $script:onWindows = [IO.Path]::DirectorySeparatorChar -eq '\'
    if ($onWindows) {
        $script:native = (Get-ChildItem -LiteralPath (Join-Path $module 'runtimes') -Recurse -File -Filter 'pwrs_example_hello.dll' | Select-Object -First 1).DirectoryName
        $script:library = @((Get-Process -Id $PID).Modules | Where-Object { $_.ModuleName -like 'pwrs_example_hello.*.dll' })[-1].FileName
    }
}

Describe 'Symbol files' {
    It 'copies each .NET assembly''s .pdb beside its staged copy when the build wrote one' {
        $staged = @{
            "$($shell.GetName().Name).pdb" = Split-Path -Parent $shell.Location
            'Pwrs.Runtime.pdb' = Split-Path -Parent $shell.Location
            'Pwrs.Bootstrap.pdb' = Split-Path -Parent ([Pwrs.Bootstrap.Loader].Assembly.Location)
        }
        foreach ($pdb in $staged.Keys) {
            $built = Test-Path -LiteralPath (Join-Path $managed $pdb)
            $built | Should -Be $symbols -Because "the build writes $pdb with the others"
            Test-Path -LiteralPath (Join-Path $staged[$pdb] $pdb) | Should -Be $built -Because "$pdb beside its copy in $($staged[$pdb])"
        }
    }

    It 'gives a stack trace through the shell a file and line only when the build wrote symbols' {
        $thrown = $null
        try { [Hello.Counter]::Parse('nope') } catch { $thrown = $_.Exception }
        while ($thrown.InnerException) { $thrown = $thrown.InnerException }
        $frame = @($thrown.StackTrace -split "`n" | Where-Object { $_ -match 'Hello\.Counter\.Parse' })
        $frame.Count | Should -Be 1
        $frame[0] -match ' in .*Hello\.Shell\.cs:line \d+' | Should -Be $symbols -Because $frame[0]
    }

    It 'ships the Rust library''s .pdb with the .NET ones, on Windows' -Skip:(-not $onWindows) {
        Test-Path -LiteralPath (Join-Path $native 'pwrs_example_hello.pdb') | Should -Be $symbols
        Test-Path -LiteralPath (Join-Path $native 'hello_helper.pdb') | Should -Be $symbols
    }

    It 'stages the Rust library in a folder of its own with its .pdb, on Windows' -Skip:(-not $onWindows) {
        $folder = Split-Path -Parent $library
        Split-Path -Leaf $folder | Should -Be ([IO.Path]::GetFileNameWithoutExtension($library))
        $staged = Join-Path $folder 'pwrs_example_hello.pdb'
        Test-Path -LiteralPath $staged | Should -Be $symbols
        if ($symbols) {
            (Get-FileHash -LiteralPath $staged).Hash | Should -Be (Get-FileHash -LiteralPath (Join-Path $native 'pwrs_example_hello.pdb')).Hash
        }
    }

    It 'stages the helper''s .pdb beside its copy, on Windows' -Skip:(-not $onWindows) {
        $ran = Invoke-HelloHelper where
        Test-Path -LiteralPath (Join-Path (Split-Path -Parent $ran) 'hello_helper.pdb') | Should -Be $symbols
    }

    # .NET's own PE reader, which Windows PowerShell does not have, reads
    # the names the debugger searches for.
    It 'stages each Rust .pdb under the name its file records, on Windows' -Skip:(-not $onWindows -or $PSVersionTable.PSEdition -ne 'Core') {
        $ran = Invoke-HelloHelper where
        foreach ($image in $library, $ran) {
            $stream = [IO.File]::OpenRead($image)
            try {
                $pe = [System.Reflection.PortableExecutable.PEReader]::new($stream)
                $entry = @($pe.ReadDebugDirectory() | Where-Object Type -EQ 'CodeView')[0]
                $recorded = [IO.Path]::GetFileName($pe.ReadCodeViewDebugDirectoryData($entry).Path)
            } finally {
                $stream.Dispose()
            }
            $recorded | Should -BeIn 'pwrs_example_hello.pdb', 'hello_helper.pdb'
            Test-Path -LiteralPath (Join-Path (Split-Path -Parent $image) $recorded) | Should -Be $symbols -Because "$recorded beside $image"
        }
    }
}
