# Importing the module from several runspaces at once in a process that
# has not imported it yet. PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $manifest = Join-Path $module 'Hello.psd1'
}

Describe 'a cold import from eight runspaces at once' {
    It 'succeeds in every runspace, in a first round and a second' {
        # A child host of this edition, so no import precedes the eight.
        $hostPath = (Get-Process -Id $PID).Path
        $script = "`$pool = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspacePool(8, 8); `$pool.Open(); " +
            "foreach (`$round in 1, 2) { " +
            "`$jobs = @(1..8 | ForEach-Object { `$ps = [PowerShell]::Create(); `$ps.RunspacePool = `$pool; " +
            "`$null = `$ps.AddScript(`"Import-Module '$manifest' -ErrorAction Stop; Get-Greeting -Name r`" + `$round); " +
            "@{ Ps = `$ps; Handle = `$ps.BeginInvoke() } }); " +
            "foreach (`$j in `$jobs) { try { `$out = `$j.Ps.EndInvoke(`$j.Handle); foreach (`$o in `$out) { 'OUT ' + `$o } } catch { 'ERR ' + `$_.Exception.Message } " +
            "foreach (`$e in `$j.Ps.Streams.Error) { 'ERR ' + `$e.Exception.Message }; `$j.Ps.Dispose() } }; `$pool.Close()"
        # Encoded, since Windows PowerShell strips the double quotes inside
        # a -Command argument and the script carries them.
        $encoded = [System.Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($script))
        $out = @(& $hostPath -NoProfile -NonInteractive -EncodedCommand $encoded 2>&1 | ForEach-Object { "$_" })
        $LASTEXITCODE | Should -Be 0
        @($out | Where-Object { $_ -like 'ERR *' }) | Should -BeNullOrEmpty
        @($out | Where-Object { $_ -eq 'OUT Hello, r1!' }).Count | Should -Be 8
        @($out | Where-Object { $_ -eq 'OUT Hello, r2!' }).Count | Should -Be 8
    }
}
