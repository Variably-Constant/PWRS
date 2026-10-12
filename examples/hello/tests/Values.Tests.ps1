# Dates, time spans, GUIDs, chars, secure strings and credentials as
# parameters, class fields and outputs. PWRS_MODULE points at the built
# module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'dates and time spans' {
    It 'carries a DateTime, a TimeSpan, GUIDs, a char and enums on a copied class' {
        $at = Get-Date -Year 2024 -Month 3 -Day 5 -Hour 6 -Minute 7 -Second 8 -Millisecond 0
        $id = [guid]::NewGuid()
        $s = New-RustStamp -At $at -Took ([timespan]::FromMinutes(90)) -Id $id -Label rust -History $id, $id -Signals Red, Green
        $s.GetType().FullName | Should -Be 'Hello.Stamp'
        $s.At | Should -Be $at
        $s.At.Kind | Should -Be $at.Kind
        $s.AtUtc | Should -Be $at.ToUniversalTime()
        $s.AtUtc.Kind | Should -Be 'Utc'
        $s.Took | Should -Be ([timespan]::FromMinutes(90))
        $s.Took.GetType().Name | Should -Be 'TimeSpan'
        $s.Id | Should -Be $id
        $s.Id.GetType().Name | Should -Be 'Guid'
        $s.Initial | Should -Be ([char]'r')
        $s.Initial.GetType().Name | Should -Be 'Char'
        $s.History.Count | Should -Be 2
        $s.History.GetType().Name | Should -Be 'Guid[]'
        $s.Signals.GetType().Name | Should -Be 'Signal[]'
        $s.Signals[1] | Should -Be ([Hello.Signal]::Green)
    }

    It 'defaults an absent span to zero' {
        (New-RustStamp -At (Get-Date) -Id ([guid]::Empty) -Label x).Took | Should -Be ([timespan]::Zero)
    }

    It 'rejects an empty label with a non-terminating error' {
        { New-RustStamp -At (Get-Date) -Id ([guid]::Empty) -Label '' -ErrorAction Stop } | Should -Throw
    }

    It 'adds a span to a UTC date, keeps the kind, and negates the span' {
        $start = ([datetime]'2024-01-01T00:00:00Z').ToUniversalTime()
        $r = @(Add-RustTime -At $start -Span ([timespan]::FromHours(25)))
        $r[0] | Should -Be $start.AddHours(25)
        $r[0].Kind | Should -Be 'Utc'
        $r[0].GetType().Name | Should -Be 'DateTime'
        $r[1] | Should -Be ([timespan]::FromHours(-25))
    }
}

Describe 'GUIDs and chars' {
    It 'formats a GUID and writes it back typed' {
        $g = [guid]'8f1c2b3a-4d5e-6f70-8192-a3b4c5d6e7f8'
        $r = @(Test-RustValues -Id $g -Letter x -Ids $g, ([guid]::Empty))
        $r[0] | Should -Be '8f1c2b3a-4d5e-6f70-8192-a3b4c5d6e7f8'
        $r[1] | Should -Be $g
        $r[1].GetType().Name | Should -Be 'Guid'
        $r[2] | Should -Be ([char]'x')
        $r[2].GetType().Name | Should -Be 'Char'
        $r[3] | Should -Be 2
    }

    It 'binds a GUID from its plain string form' {
        (Test-RustValues -Id '8f1c2b3a4d5e6f708192a3b4c5d6e7f8' -Letter y)[0] | Should -Be '8f1c2b3a-4d5e-6f70-8192-a3b4c5d6e7f8'
    }

    It 'rejects two characters for a char parameter' {
        { Test-RustValues -Id ([guid]::Empty) -Letter xy -ErrorAction Stop } | Should -Throw
    }
}

Describe 'secure strings and credentials' {
    It 'reveals a credential and builds one in Rust' {
        $password = ConvertTo-SecureString 'hunter2' -AsPlainText -Force
        $credential = [pscredential]::new('ada', $password)
        $secret = ConvertTo-SecureString 'topsecret' -AsPlainText -Force
        $r = @(Test-RustCredential -Credential $credential -Reverse -Secret $secret)
        $r[0] | Should -Be 'ada:7'
        $r[1].GetType().Name | Should -Be 'PSCredential'
        $r[1].UserName | Should -Be 'ada'
        $r[1].GetNetworkCredential().Password | Should -Be '2retnuh'
        $r[2] | Should -Be 'topsecret'
    }
}
