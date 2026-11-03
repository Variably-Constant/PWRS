# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Hello.Draft implements INotifyPropertyChanged and ICommand, whose events
# PWRS declares for it, declares Saved and Ticked of its own, and raises
# them from Rust. Watch-RustFolder and Watch-RustFolderQueue subscribe Rust
# to a FileSystemWatcher's Created event, the one through a callback on
# the raising thread and the other through the runspace's event queue.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $script:manifest = Join-Path $module 'Hello.psd1'
    Import-Module $manifest -Force -ErrorAction Stop

    # A new empty folder under the temporary folder.
    function script:New-TestFolder {
        $dir = Join-Path ([IO.Path]::GetTempPath()) ('pwrs-events-' + [guid]::NewGuid().ToString('n'))
        $null = New-Item -ItemType Directory -Path $dir
        $dir
    }

    # Drops every subscription and queued event these tests made.
    function script:Clear-HelloTestEvents {
        Get-EventSubscriber -Force | Where-Object SourceIdentifier -like 'Hello.Test.*' | ForEach-Object { Unregister-Event -SubscriptionId $_.SubscriptionId -Force }
        Get-Event | Where-Object SourceIdentifier -like 'Hello.Test.*' | Remove-Event
    }
}

Describe 'A class with events of its own and of the interfaces it implements' {
    It 'implements INotifyPropertyChanged and ICommand' {
        $d = [Hello.Draft]::new()
        $d | Should -BeOfType [System.ComponentModel.INotifyPropertyChanged]
        $d | Should -BeOfType [System.Windows.Input.ICommand]
        $d.CanExecute($null) | Should -BeFalse
    }

    It 'delivers PropertyChanged once the setter returns, so a handler reads the new value' {
        $d = [Hello.Draft]::new()
        $seen = [System.Collections.Generic.List[string]]::new()
        $d.add_PropertyChanged({ param($s, $e) $seen.Add("$($e.PropertyName)=$($s.Text)") })
        $d.SetText('one')
        $d.SetText('two')
        @($seen) | Should -Be @('Text=one', 'Text=two')
    }

    It 'raises CanExecuteChanged when the text becomes empty or stops being empty' {
        $d = [Hello.Draft]::new()
        $seen = [System.Collections.Generic.List[bool]]::new()
        $d.add_CanExecuteChanged({ param($s, $e) $seen.Add($s.CanExecute($null)) })
        $d.SetText('a')
        $d.SetText('b')
        $d.SetText('')
        @($seen) | Should -Be @($true, $false)
    }

    It 'raises an event of its own with EventArgs.Empty when Rust gives no argument' {
        $d = [Hello.Draft]::new()
        $saves = [System.Collections.Generic.List[string]]::new()
        $d.add_Saved({ param($s, $e) $saves.Add("$($s.Saves) $([object]::ReferenceEquals($e, [EventArgs]::Empty)) $([object]::ReferenceEquals($s, $d))") })
        $d.Execute('first')
        $d.Execute($null)
        @($saves) | Should -Be @('1 True True', '2 True True')
        $d.Text | Should -Be 'first'
    }

    It 'makes a handler''s exception the setter''s, after the value changed' {
        $d = [Hello.Draft]::new()
        $d.add_PropertyChanged({ throw 'the handler refused' })
        { $d.SetText('kept') } | Should -Throw '*the handler refused*'
        $d.Text | Should -Be 'kept'
    }
}

Describe 'An event raised from a thread the module started' {
    AfterEach { Clear-HelloTestEvents }

    # Register-ObjectEvent queues an event raised on another thread
    # through the thread pool, so it receives each event but not in their order.
    It 'reaches Register-ObjectEvent once for each raise, with the object as its sender' {
        $d = [Hello.Draft]::new()
        $null = Register-ObjectEvent -InputObject $d -EventName Ticked -SourceIdentifier Hello.Test.Ticked
        $d.Tick(5)
        $got = foreach ($i in 1..5) {
            $e = Wait-Event -SourceIdentifier Hello.Test.Ticked -Timeout 10
            if (-not $e) { break }
            Remove-Event -EventIdentifier $e.EventIdentifier
            $e
        }
        @($got).Count | Should -Be 5
        ($got | ForEach-Object { $_.SourceArgs[1].Number } | Sort-Object) -join ',' | Should -Be '1,2,3,4,5'
        [object]::ReferenceEquals($got[0].Sender, $d) | Should -BeTrue
        $got[0].SourceArgs[1].GetType().FullName | Should -Be 'Hello.TickArgs'
    }

    # The thread raises as fast as it can, so the queue takes a steady
    # stream while the phase reads it.
    It 'reaches a phase through the event queue in the order the thread raised it, at the rate it raised it' {
        $d = [Hello.Draft]::new()
        $ticks = @(Receive-RustTicks -Draft $d -Count 5000)
        $ticks.Count | Should -Be 5000
        $ticks -join ',' | Should -Be ((1..5000) -join ',')
        @(Get-Event -SourceIdentifier Hello.Ticks -ErrorAction Ignore).Count | Should -Be 0
    }

    It 'fails for a script block handler, which runs only on the thread of a runspace' {
        # Windows PowerShell makes each line a native command writes to
        # standard error an error record, which a Stop preference throws.
        $ErrorActionPreference = 'Continue'
        $hostExe = (Get-Process -Id $PID).Path
        $script = "Import-Module '$($manifest.Replace("'", "''"))'; `$d = [Hello.Draft]::new(); `$d.add_Ticked({ param(`$s, `$e) }); `$d.Tick(1); Start-Sleep -Seconds 3"
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
        $lines = & $hostExe -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded 2>&1 | ForEach-Object { "$_" }
        ($lines -join "`n") | Should -Match 'raising Ticked 1 of 1 failed: .*There is no Runspace available to run scripts in this thread'
    }
}

Describe 'Rust subscribed to an event of a .NET object' {
    AfterEach { Clear-HelloTestEvents }

    It 'runs a callback on the thread that raised, once for each file a FileSystemWatcher reports' {
        $dir = New-TestFolder
        try {
            $lines = @(Watch-RustFolder -Path $dir -Create 3)
        } finally {
            Remove-Item -LiteralPath $dir -Recurse -Force
        }
        $lines | Should -Be @('watched-1.txt true', 'watched-2.txt true', 'watched-3.txt true')
    }

    It 'reads each file back from the runspace''s event queue, and leaves no event behind' {
        $dir = New-TestFolder
        try {
            $lines = @(Watch-RustFolderQueue -Path $dir -Create 3 -SourceIdentifier Hello.Test.Folder)
        } finally {
            Remove-Item -LiteralPath $dir -Recurse -Force
        }
        $lines | Should -Be @('queued-1.txt', 'queued-2.txt', 'queued-3.txt')
        @(Get-Event -SourceIdentifier Hello.Test.Folder -ErrorAction Ignore).Count | Should -Be 0
    }
}
