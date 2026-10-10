# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Send-RustEvent raises engine events through PsEvents from a thread the
# command starts; script receives them through Wait-Event, Get-Event and
# Register-EngineEvent -Action.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # Drops every subscription, action job and queued event these tests made.
    function Clear-HelloTestEvents {
        Get-EventSubscriber -Force | Where-Object SourceIdentifier -like 'Hello.Test.*' | ForEach-Object { Unregister-Event -SubscriptionId $_.SubscriptionId -Force }
        Get-Job | Where-Object Name -like 'Hello.Test.*' | Remove-Job -Force
        Get-Event | Where-Object SourceIdentifier -like 'Hello.Test.*' | Remove-Event
    }
}

Describe 'An engine event raised from a Rust thread' {
    AfterEach { Clear-HelloTestEvents }

    It 'reaches Wait-Event with its MessageData' {
        Send-RustEvent Hello.Test.Wait -Text 'from rust'
        $e = Wait-Event -SourceIdentifier Hello.Test.Wait -Timeout 10
        $e | Should -Not -BeNullOrEmpty
        $e.SourceIdentifier | Should -Be 'Hello.Test.Wait'
        $e.MessageData.GetType().FullName | Should -Be 'Hello.EventData'
        $e.MessageData.Text | Should -Be 'from rust'
        $e.MessageData.Number | Should -Be 1
        $e.MessageData.Number | Should -BeOfType [long]
        $e.Sender | Should -BeNullOrEmpty
        @($e.SourceArgs).Count | Should -Be 0
    }

    It 'is queued before the command returns, in the order the thread raised it' {
        Send-RustEvent Hello.Test.Order -Text n -Count 200
        $got = @(Get-Event -SourceIdentifier Hello.Test.Order)
        $got.Count | Should -Be 200
        ($got | ForEach-Object { $_.MessageData.Number }) -join ',' | Should -Be ((1..200) -join ',')
    }

    It 'carries a sender and arguments' {
        Send-RustEvent Hello.Test.Sender -Text s -Sender 'hello' -Count 2
        $got = @(Get-Event -SourceIdentifier Hello.Test.Sender)
        $got.Count | Should -Be 2
        $got[1].Sender | Should -Be 'hello'
        $got[1].SourceArgs.Count | Should -Be 2
        $got[0].SourceArgs[0] | Should -Be 1
        $got[1].SourceArgs[0] | Should -Be 2
        $got[1].SourceArgs[1] | Should -Be 2
        $got[1].SourceArgs[1] | Should -BeOfType [long]
    }

    It 'runs an -Action on the pipeline thread, in order, and is not queued' {
        $global:HelloTestActions = [System.Collections.ArrayList]::new()
        $null = Register-EngineEvent -SourceIdentifier Hello.Test.Action -Action {
            $null = $global:HelloTestActions.Add([pscustomobject]@{
                Thread = [System.Threading.Thread]::CurrentThread.ManagedThreadId
                Text   = $Event.MessageData.Text
                Number = $Event.MessageData.Number
            })
        }
        $pipelineThread = [System.Threading.Thread]::CurrentThread.ManagedThreadId
        Send-RustEvent Hello.Test.Action -Text acted -Count 3
        $clock = [System.Diagnostics.Stopwatch]::StartNew()
        while ($global:HelloTestActions.Count -lt 3 -and $clock.ElapsedMilliseconds -lt 10000) { Start-Sleep -Milliseconds 20 }
        $global:HelloTestActions.Count | Should -Be 3
        $global:HelloTestActions.Thread | Sort-Object -Unique | Should -Be $pipelineThread
        $global:HelloTestActions.Number -join ',' | Should -Be '1,2,3'
        $global:HelloTestActions.Text | Sort-Object -Unique | Should -Be 'acted'
        @(Get-Event -SourceIdentifier Hello.Test.Action -ErrorAction SilentlyContinue).Count | Should -Be 0
    }

    It 'is raised by a thread the module keeps after the command returned' {
        Send-RustEvent Hello.Test.Later -Text later -DelayMs 500
        $returned = [DateTime]::Now
        $e = Wait-Event -SourceIdentifier Hello.Test.Later -Timeout 10
        $e.MessageData.Text | Should -Be 'later'
        $e.TimeGenerated | Should -BeGreaterThan $returned
    }

    It 'reaches Wait-Event in a host whose pipeline thread is not single-threaded, started with -MTA where apartments exist' {
        $hostPath = (Get-Process -Id $PID).Path
        $windows = [IO.Path]::DirectorySeparatorChar -eq '\'
        $flags = @(if ($windows) { '-MTA' })
        $script = "Import-Module '$(Join-Path $env:PWRS_MODULE 'Hello.psd1')' -ErrorAction Stop; " +
            "Send-RustEvent Hello.Test.Mta -Text mta; " +
            "`$e = Wait-Event -SourceIdentifier Hello.Test.Mta -Timeout 10; " +
            "'apartment=' + [System.Threading.Thread]::CurrentThread.GetApartmentState(); 'text=' + `$e.MessageData.Text"
        $out = @(& $hostPath -NoProfile -NonInteractive @flags -Command $script 2>&1 | ForEach-Object { "$_" })
        $LASTEXITCODE | Should -Be 0 -Because ($out -join "`n")
        $out | Should -Contain 'text=mta'
        $out | Should -Not -Contain 'apartment=STA'
        if ($windows) { $out | Should -Contain 'apartment=MTA' }
    }
}
