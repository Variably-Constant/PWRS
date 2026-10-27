# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Each case runs in a child process of this host, so the imports other
# suites make in this process do not count. The counts live in the
# native library, which a removal does not unload, so a Remove-Module and
# the import after it do not reset them. The registration hello's load
# hook makes is a variable of the process environment, which every
# runspace of the process reads. A child inherits the environment, and
# with it the registration of each copy loaded in this process, so the
# child clears those first.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $script:manifest = Join-Path $module 'Hello.psd1'
    $script:hostExe = (Get-Process -Id $PID).Path
    $info = [System.Runtime.InteropServices.RuntimeInformation]
    $platform = [System.Runtime.InteropServices.OSPlatform]
    $onWindows = [IO.Path]::DirectorySeparatorChar -eq '\'
    $onMac = -not $onWindows -and $info::IsOSPlatform($platform::OSX)
    $os = if ($onWindows) { 'win' } elseif ($onMac) { 'osx' } elseif ($info::IsOSPlatform($platform::Linux)) { 'linux' } else { 'freebsd' }
    $rid = $os + '-' + $info::ProcessArchitecture.ToString().ToLowerInvariant()
    $file = if ($onWindows) { 'pwrs_example_hello.dll' } elseif ($onMac) { 'libpwrs_example_hello.dylib' } else { 'libpwrs_example_hello.so' }
    $script:library = [IO.Path]::Combine($module, 'runtimes', $rid, 'native', $file)

    # What every child script starts with: Registrations lists the
    # registrations in the process, Counts reads the counts of the copy
    # in use, In runs a script in a runspace and returns its output and
    # then its errors as text, and Open-Runspace opens a runspace with
    # $manifest and the first two set there.
    $script:prelude = {
        $ErrorActionPreference = 'Stop'
        $ProgressPreference = 'SilentlyContinue'
        foreach ($inherited in @(Get-ChildItem Env: | Where-Object { $_.Name -like 'PWRS_HELLO_LOAD_*' })) {
            Remove-Item -LiteralPath "Env:$($inherited.Name)"
        }
        $defs = {
            function Registrations { @(Get-ChildItem Env: | Where-Object { $_.Name -like 'PWRS_HELLO_LOAD_*' } | ForEach-Object { $_.Name }) }
            function Counts { "loads $(Get-RustLifecycle Load) imports $(Get-RustLifecycle Import) removes $(Get-RustLifecycle Remove) unloads $(Get-RustLifecycle Unload)" }
        }
        . $defs
        function In($rs, [string] $script) {
            $ps = [powershell]::Create()
            $ps.Runspace = $rs
            try {
                foreach ($o in $ps.AddScript($script).Invoke()) { "$o" }
                foreach ($e in $ps.Streams.Error) { "error $($e.FullyQualifiedErrorId) [$($e.CategoryInfo.Category)] $($e.Exception.Message)" }
            } finally { $ps.Dispose() }
        }
        function Open-Runspace {
            $rs = [runspacefactory]::CreateRunspace()
            $rs.Open()
            $rs.SessionStateProxy.SetVariable('manifest', $manifest)
            $null = In $rs $defs.ToString()
            $rs
        }
    }

    # Runs $body in a child process of this host, after the prelude and
    # with $manifest and $library set, and returns its output lines, its
    # standard error and its exit code. The script goes encoded, since
    # Windows PowerShell strips the double quotes inside a -Command
    # argument. With its streams redirected, pwsh writes its errors to
    # standard error as text under -OutputFormat Text and as CLIXML
    # without it; Windows PowerShell writes CLIXML either way.
    function script:Invoke-Child([scriptblock] $body) {
        $paths = "`$manifest = '{0}'`n`$library = '{1}'`n" -f $manifest.Replace("'", "''"), $library.Replace("'", "''")
        $text = $paths + $prelude.ToString() + "`n" + $body.ToString()
        $start = New-Object System.Diagnostics.ProcessStartInfo $hostExe
        $start.Arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -OutputFormat Text -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($text))
        $start.UseShellExecute = $false
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $child = [System.Diagnostics.Process]::Start($start)
        $stderr = $child.StandardError.ReadToEndAsync()
        $stdout = $child.StandardOutput.ReadToEnd()
        $child.WaitForExit()
        [pscustomobject]@{ Lines = @($stdout -split '\r?\n' | Where-Object { $_ }); Error = $stderr.Result; ExitCode = $child.ExitCode }
    }
}

Describe 'the load and unload hooks' {
    It 'count imports across runspaces, so a removal in one leaves the state another runspace uses' {
        $run = Invoke-Child {
            Import-Module $manifest
            $rs = Open-Runspace
            $null = In $rs 'Import-Module $manifest'
            "both imported: $((Registrations).Count) $(Counts)"
            Remove-Module Hello
            In $rs '"after one removal: $((Registrations).Count) $(Counts) $((Get-Person Ann).Name)"'
            $null = In $rs 'Remove-Module Hello'
            "after the last removal: $((Registrations).Count)"
            Import-Module $manifest
            "imported again: $((Registrations).Count) $(Counts)"
            $rs.Dispose()
        }
        $run.Error | Should -BeNullOrEmpty
        $run.ExitCode | Should -Be 0
        ($run.Lines -join "`n") | Should -Be (@(
            'both imported: 1 loads 1 imports 2 removes 0 unloads 0'
            'after one removal: 1 loads 1 imports 2 removes 1 unloads 0 Ann'
            'after the last removal: 0'
            'imported again: 1 loads 2 imports 3 removes 2 unloads 1'
        ) -join "`n")
    }

    It 'count a runspace closed without Remove-Module as its removal, and the last close releases the state' {
        $run = Invoke-Child {
            $rs = Open-Runspace
            $null = In $rs 'Import-Module $manifest'
            "imported there: $((Registrations).Count)"
            $rs.Dispose()
            "after its close: $((Registrations).Count)"
            Import-Module $manifest
            "imported here: $((Registrations).Count) $(Counts)"
        }
        $run.Error | Should -BeNullOrEmpty
        $run.ExitCode | Should -Be 0
        ($run.Lines -join "`n") | Should -Be (@(
            'imported there: 1'
            'after its close: 0'
            'imported here: 1 loads 2 imports 2 removes 1 unloads 1'
        ) -join "`n")
    }

    It 'release the state when a pool, a thread job, or a parallel run in pwsh, that imported the module ends' {
        $run = Invoke-Child {
            $iss = [initialsessionstate]::CreateDefault()
            $iss.ImportPSModule($manifest)
            $pool = [runspacefactory]::CreateRunspacePool(1, 2, $iss, $Host)
            $pool.Open()
            $runs = foreach ($i in 1, 2) {
                $ps = [powershell]::Create()
                $ps.RunspacePool = $pool
                $null = $ps.AddScript('(Get-Person Ann).Name')
                [pscustomobject]@{ Ps = $ps; Handle = $ps.BeginInvoke() }
            }
            foreach ($r in $runs) { "the pool ran: $($r.Ps.EndInvoke($r.Handle))"; $r.Ps.Dispose() }
            "while the pool is open: $((Registrations).Count)"
            $pool.Dispose()
            "after the pool: $((Registrations).Count)"
            if (Get-Command Start-ThreadJob -ErrorAction SilentlyContinue) {
                $null = Start-ThreadJob { Import-Module $using:manifest; $null = Get-Person Ann } | Receive-Job -Wait -AutoRemoveJob
                "after the thread job: $((Registrations).Count)"
            }
            if ($PSVersionTable.PSVersion.Major -ge 7) {
                $null = 1..3 | ForEach-Object -ThrottleLimit 2 -Parallel { Import-Module $using:manifest; $null = Get-Person Ann }
                "after the parallel run: $((Registrations).Count)"
            }
            Import-Module $manifest
            "imported here: loads $(Get-RustLifecycle Load) unloads $(Get-RustLifecycle Unload)"
        }
        $run.Error | Should -BeNullOrEmpty
        $run.ExitCode | Should -Be 0
        # The child is a process of this host on this machine, so it has
        # Start-ThreadJob exactly when this process does.
        $expected = @('the pool ran: Ann', 'the pool ran: Ann', 'while the pool is open: 1', 'after the pool: 0')
        $loads = 2
        if (Get-Command Start-ThreadJob -ErrorAction SilentlyContinue) { $expected += 'after the thread job: 0'; $loads++ }
        if ($PSVersionTable.PSVersion.Major -ge 7) { $expected += 'after the parallel run: 0'; $loads++ }
        $expected += "imported here: loads $loads unloads $($loads - 1)"
        ($run.Lines -join "`n") | Should -Be ($expected -join "`n")
    }

    It 'at a reload, run the old copy''s unload hook and the new copy''s load hook once each, while another runspace has the module' {
        $run = Invoke-Child {
            Import-Module $manifest
            $rs = Open-Runspace
            $null = In $rs 'Import-Module $manifest'
            $before = @(Registrations)
            "before: $($before.Count)"
            $file = Get-Item -LiteralPath $library
            $stamp = $file.LastWriteTimeUtc
            try {
                $file.LastWriteTimeUtc = [DateTime]::UtcNow
                In $rs 'Import-Module $manifest -Force'
            } finally {
                $file.LastWriteTimeUtc = $stamp
            }
            $after = @(Registrations)
            "after: $($after.Count), the new copy's own: $($after[0] -ne $before[0])"
            "here: $(Counts)"
            In $rs '"there: $(Counts)"'
            $rs.Dispose()
        }
        $run.Error | Should -BeNullOrEmpty
        $run.ExitCode | Should -Be 0
        ($run.Lines -join "`n") | Should -Be (@(
            'before: 1'
            "after: 1, the new copy's own: True"
            'here: loads 1 imports 1 removes 0 unloads 0'
            'there: loads 1 imports 1 removes 0 unloads 0'
        ) -join "`n")
    }
}

Describe 'a hook that fails' {
    It 'is written as an error of Remove-Module when it is the removal or unload hook, and the removal finishes' {
        # Under the Stop preference the prelude sets, and with
        # -ErrorAction SilentlyContinue, which the errors are shown
        # despite.
        $run = Invoke-Child {
            Import-Module $manifest
            Set-RustLifecycleFailure Remove
            Set-RustLifecycleFailure Unload
            Remove-Module Hello -ErrorAction SilentlyContinue -ErrorVariable failed
            "errors: $($failed.Count)"
            foreach ($e in $failed) { "$($e.FullyQualifiedErrorId) [$($e.CategoryInfo.Category)] $($e.Exception.Message)" }
            "still imported: $([bool](Get-Module Hello)), registrations: $((Registrations).Count)"
        }
        $run.Error | Should -Match 'the remove hook failed because Set-RustLifecycleFailure set it to fail'
        $run.Error | Should -Match 'the unload hook failed because Set-RustLifecycleFailure set it to fail'
        $run.ExitCode | Should -Be 0
        $run.Lines[0] | Should -Be 'errors: 2'
        $run.Lines[1] | Should -BeLike 'HelloLifecycleFailure* `[InvalidOperation`] the remove hook failed because Set-RustLifecycleFailure set it to fail'
        $run.Lines[2] | Should -BeLike 'HelloLifecycleFailure* `[InvalidOperation`] the unload hook failed because Set-RustLifecycleFailure set it to fail'
        $run.Lines[3] | Should -Be 'still imported: False, registrations: 0'
    }

    It 'fails the import when it is the load hook, and the next import runs it again' {
        $run = Invoke-Child {
            Import-Module $manifest
            Set-RustLifecycleFailure Load
            Remove-Module Hello
            try { Import-Module $manifest; 'imported' }
            catch { "refused: $($_.FullyQualifiedErrorId) $($_.Exception.Message), listed: $([bool](Get-Module Hello)), registrations: $((Registrations).Count)" }
            Import-Module $manifest
            "imported again: $((Registrations).Count) $(Counts)"
        }
        $run.Error | Should -BeNullOrEmpty
        $run.ExitCode | Should -Be 0
        $run.Lines[0] | Should -BeLike 'refused: HelloLifecycleFailure* the load hook failed because Set-RustLifecycleFailure set it to fail, listed: False, registrations: 0'
        $run.Lines[1] | Should -Be 'imported again: 1 loads 3 imports 2 removes 1 unloads 1'
    }

    It 'is written during Import-Module when it is the old copy''s unload hook at a reload, and the import goes on' {
        $run = Invoke-Child {
            Import-Module $manifest
            $rs = Open-Runspace
            $null = In $rs 'Import-Module $manifest'
            Set-RustLifecycleFailure Unload
            $file = Get-Item -LiteralPath $library
            $stamp = $file.LastWriteTimeUtc
            try {
                $file.LastWriteTimeUtc = [DateTime]::UtcNow
                In $rs 'Import-Module $manifest -Force; "imported: $([bool](Get-Module Hello))"'
            } finally {
                $file.LastWriteTimeUtc = $stamp
            }
            "registrations: $((Registrations).Count) $(Counts)"
            $rs.Dispose()
        }
        $run.Error | Should -BeNullOrEmpty
        $run.ExitCode | Should -Be 0
        $run.Lines[0] | Should -Be 'imported: True'
        $run.Lines[1] | Should -BeLike 'error HelloLifecycleFailure* `[InvalidOperation`] the unload hook failed because Set-RustLifecycleFailure set it to fail'
        $run.Lines[2] | Should -Be 'registrations: 1 loads 1 imports 1 removes 0 unloads 0'
    }

    It 'goes to standard error when the runspace closing ran it, as at the end of the process' {
        $run = Invoke-Child {
            Import-Module $manifest
            Set-RustLifecycleFailure Unload
            'imported'
        }
        $run.ExitCode | Should -Be 0
        $run.Lines | Should -Be 'imported'
        $run.Error | Should -Match 'pwrs: the unload hook of \S+ failed as runspace [0-9a-f-]+ closed: \[HelloLifecycleFailure\] the unload hook failed because Set-RustLifecycleFailure set it to fail'
    }
}

Describe 'importing in a fresh process' {
    It 'leaves Microsoft.PowerShell.Utility unloaded, so the import runs nothing that sends PowerShell searching the module path' {
        # A command from a module not yet loaded makes PowerShell analyze
        # every module on PSModulePath that is not in its cache.
        $exe = (Get-Process -Id $PID).Path
        $loaded = & $exe -NoProfile -NonInteractive -Command "Import-Module '$manifest' -ErrorAction Stop; [bool](Get-Module Microsoft.PowerShell.Utility)"
        $LASTEXITCODE | Should -Be 0
        $loaded | Should -Be 'False'
    }
}
