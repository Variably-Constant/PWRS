# The module's commands and objects in other runspaces of the process and
# in a background job. PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $manifest = Join-Path $module 'Hello.psd1'
    Import-Module $manifest -Force -ErrorAction Stop

    # Runs $script in a runspace of its own, with $vars set there, writes
    # its output, and throws its first error record.
    function script:Invoke-InRunspace([string] $script, [hashtable] $vars = @{}) {
        $rs = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspace()
        $rs.Open()
        try {
            foreach ($k in $vars.Keys) { $rs.SessionStateProxy.SetVariable($k, $vars[$k]) }
            $ps = [System.Management.Automation.PowerShell]::Create()
            $ps.Runspace = $rs
            try {
                $ps.AddScript($script).Invoke()
                if ($ps.Streams.Error.Count) { throw $ps.Streams.Error[0] }
            } finally {
                $ps.Dispose()
            }
        } finally {
            $rs.Dispose()
        }
    }
}

Describe 'a runspace of its own' {
    It 'has the module''s commands only once it imports the module' {
        { Invoke-InRunspace 'Get-Person Ann' } | Should -Throw -ErrorId 'CommandNotFoundException'
        Invoke-InRunspace 'Import-Module $manifest; (Get-Person Ann).GetType().FullName' @{ manifest = $manifest } | Should -Be 'Hello.Person'
    }

    It 'gets the same proxy object, whose value it changes for both runspaces' {
        $c = New-Counter shared -Value 5
        Invoke-InRunspace '$c.Advance(2)' @{ c = $c } | Should -Be 7
        $c.Value | Should -Be 7
    }

    It 'runs calls on one proxy from four runspaces one at a time' {
        $c = New-Counter race
        $runs = foreach ($i in 1..4) {
            $rs = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspace()
            $rs.Open()
            $rs.SessionStateProxy.SetVariable('c', $c)
            $ps = [System.Management.Automation.PowerShell]::Create()
            $ps.Runspace = $rs
            $null = $ps.AddScript('foreach ($n in 1..2000) { $null = $c.Advance(1) }')
            @{ Rs = $rs; Ps = $ps; Handle = $ps.BeginInvoke() }
        }
        $errors = 0
        foreach ($r in $runs) {
            $null = $r.Ps.EndInvoke($r.Handle)
            $errors += $r.Ps.Streams.Error.Count
            $r.Ps.Dispose()
            $r.Rs.Dispose()
        }
        $errors | Should -Be 0
        $c.Value | Should -Be 8000
        @($c.History | Sort-Object -Unique).Count | Should -Be 8000
    }

    It 'has the module in every runspace of a pool whose InitialSessionState imports it' {
        $iss = [System.Management.Automation.Runspaces.InitialSessionState]::CreateDefault()
        $iss.ImportPSModule($manifest)
        $pool = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspacePool(1, 4, $iss, $Host)
        $pool.Open()
        try {
            $runs = foreach ($i in 1..4) {
                $ps = [System.Management.Automation.PowerShell]::Create()
                $ps.RunspacePool = $pool
                $null = $ps.AddScript("(Get-Person p$i).GetType().FullName")
                @{ Ps = $ps; Handle = $ps.BeginInvoke() }
            }
            $types = foreach ($r in $runs) { $r.Ps.EndInvoke($r.Handle); $r.Ps.Dispose() }
        } finally {
            $pool.Dispose()
        }
        @($types) | Should -Be @('Hello.Person', 'Hello.Person', 'Hello.Person', 'Hello.Person')
    }
}

Describe 'ForEach-Object -Parallel and Start-ThreadJob' -Skip:($PSVersionTable.PSVersion.Major -lt 7) {
    It 'has the module''s commands only in a runspace that imports it' {
        # -Parallel takes no -ErrorAction, so the preference makes the error stop.
        { $ErrorActionPreference = 'Stop'; 1 | ForEach-Object -Parallel { Get-Person Ann } } | Should -Throw -ErrorId 'CommandNotFoundException'
        @(1..2 | ForEach-Object -Parallel { Import-Module $using:manifest; (Get-Person "p$_").GetType().FullName }) | Should -Be @('Hello.Person', 'Hello.Person')
    }

    It 'counts every Advance(1) eight runspaces make on one proxy' {
        $c = New-Counter par
        1..8 | ForEach-Object -ThrottleLimit 8 -Parallel { $c = $using:c; foreach ($n in 1..1000) { $null = $c.Advance(1) } }
        $c.Value | Should -Be 8000
    }

    It 'brings back the proxies made there, which still have their methods' {
        $out = @(1..2 | ForEach-Object -Parallel { Import-Module $using:manifest; New-Counter "back$_" -Value $_ } | Sort-Object Label)
        $out[0].GetType().FullName | Should -Be 'Hello.Counter'
        $out[0].Advance(1) | Should -Be 2
        $out[1].Advance(1) | Should -Be 3
    }

    # Start-ThreadJob comes with the module Microsoft.PowerShell.ThreadJob,
    # which pwsh 7.6.7 on Windows and 7.6.6 on Linux have and the pwsh
    # 7.5.5 built for FreeBSD does not.
    It 'returns a thread job''s objects as they are' -Skip:(-not (Get-Command Start-ThreadJob -ErrorAction SilentlyContinue)) {
        $made = Start-ThreadJob { Import-Module $using:manifest; New-Counter made -Value 3 } | Receive-Job -Wait -AutoRemoveJob
        $made.GetType().FullName | Should -Be 'Hello.Counter'
        $made.Advance(1) | Should -Be 4
    }
}

Describe 'a background job' {
    BeforeAll {
        $back = @(Start-Job { Import-Module $using:manifest; Get-Person Ann -Age 30 -Tag a, b; New-Counter j -Value 5; Get-Note hi -Priority 2 } | Receive-Job -Wait -AutoRemoveJob)
        $person, $counter, $note = $back
    }

    It 'has the module''s commands only once the job imports it' {
        { Start-Job { Get-Person Ann } | Receive-Job -Wait -AutoRemoveJob -ErrorAction Stop } | Should -Throw -ErrorId 'CommandNotFoundException'
    }

    It 'brings each object back as a copy of its properties' {
        $person.PSObject.TypeNames[0] | Should -Be 'Deserialized.Hello.Person'
        $person.Age | Should -Be 30
        $person.Tags -join ',' | Should -Be 'a,b'
        $counter.PSObject.TypeNames[0] | Should -Be 'Deserialized.Hello.Counter'
        $counter.Value | Should -Be 5
        $note.PSObject.TypeNames[0] | Should -Be 'Deserialized.Hello.Note'
        $note.Text | Should -Be 'hi'
    }

    It 'brings a proxy back without its methods' {
        { $counter.Advance(1) } | Should -Throw -ErrorId 'MethodNotFound'
    }

    It 'binds a copied object that came back to a parameter of its class' {
        $team = New-RustTeam -Lead $person
        $team.Lead.GetType().FullName | Should -Be 'Hello.Person'
        $team.Lead.Name | Should -Be 'Ann'
    }

    It 'refuses a proxy object that came back, which its constructor makes again' {
        { Get-RustCounterText -Counter $counter } | Should -Throw -ErrorId 'CannotConvertArgumentNoMessage*'
        [Hello.Counter]::new($counter.Label, $counter.Value).Advance(1) | Should -Be 6
    }
}
