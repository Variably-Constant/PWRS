# Importing the module from several runspaces at once in a process that
# has not imported it yet. PWRS_MODULE points at the built module folder.
#
# Each runspace has its own session state. A RunspacePool's runspaces
# share one, whose list of format files PowerShell adds to and walks
# without a lock while it imports a module that names any, so a pool
# importing such a module in several runspaces at once can fail inside
# PowerShell with "Collection was modified", whatever the module. What
# PWRS shares between runspaces is process-wide: the staged assemblies,
# the loader's tables, the native library and the import lock, which
# separate runspaces in one process share as a pool's do.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $manifest = Join-Path $module 'Hello.psd1'
}

Describe 'a cold import from eight runspaces at once' {
    It 'succeeds in every runspace, in a first round and a second' {
        # A child host of this edition, so no import precedes the eight.
        $hostPath = (Get-Process -Id $PID).Path
        $script = "`$runspaces = @(1..8 | ForEach-Object { `$rs = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspace(); `$rs.Open(); `$rs }); " +
            "foreach (`$round in 1, 2) { " +
            "`$jobs = @(`$runspaces | ForEach-Object { `$ps = [PowerShell]::Create(); `$ps.Runspace = `$_; " +
            "`$null = `$ps.AddScript(`"Import-Module '$manifest' -ErrorAction Stop; Get-Greeting -Name r`" + `$round); " +
            "@{ Ps = `$ps; Handle = `$ps.BeginInvoke() } }); " +
            "foreach (`$j in `$jobs) { try { `$out = `$j.Ps.EndInvoke(`$j.Handle); foreach (`$o in `$out) { 'OUT ' + `$o } } catch { 'ERR ' + `$_.Exception.Message } " +
            "foreach (`$e in `$j.Ps.Streams.Error) { 'ERR ' + `$e.Exception.Message }; `$j.Ps.Dispose() } }; " +
            "foreach (`$rs in `$runspaces) { `$rs.Dispose() }"
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
