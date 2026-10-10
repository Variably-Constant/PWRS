# Pipeline and error surface that had no consumer until these cmdlets
# reached it: progress records, streaming from a worker thread, an
# error carrying a target object, and a completion tooltip.
# PWRS_MODULE points at the built module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'stream_from_thread and progress' {
    It 'streams every value a worker produced, in order' {
        $r = @(Get-RustStream -Count 5)
        $r.Count | Should -Be 5
        $r | Should -Be @(1, 2, 3, 4, 5)
    }

    It 'streams a larger run without loss' {
        $r = @(Get-RustStream -Count 2000)
        $r.Count | Should -Be 2000
        $r[0] | Should -Be 1
        $r[-1] | Should -Be 2000
    }

    It 'stops early when the pipeline stops' {
        $r = @(Get-RustStream -Count 100000 | Select-Object -First 3)
        $r | Should -Be @(1, 2, 3)
    }

    It 'rejects a count outside the validated range' {
        { Get-RustStream -Count 0 -ErrorAction Stop } | Should -Throw
    }
}

Describe 'an error that names its target' {
    It 'carries the offending object on the record' {
        Test-RustTarget -Value 'thing' -ErrorAction SilentlyContinue -ErrorVariable err | Out-Null
        $err.Count | Should -Be 1
        $err[0].FullyQualifiedErrorId | Should -Match 'TargetedFailure'
        $err[0].Exception.Message | Should -Match 'refusing thing'
        "$($err[0].TargetObject)" | Should -Be 'thing'
    }

    It 'reads the display string of a non-string object' {
        Test-RustTarget -Value 42 -ErrorAction SilentlyContinue -ErrorVariable err | Out-Null
        $err[0].Exception.Message | Should -Match 'refusing 42'
        "$($err[0].TargetObject)" | Should -Be '42'
    }
}

Describe 'completion tooltips' {
    It 'offers the tooltip the completer attached' {
        $line = 'Get-RustColor -Name cr'
        $r = TabExpansion2 -inputScript $line -cursorColumn $line.Length
        $match = $r.CompletionMatches | Where-Object { $_.CompletionText -eq 'crimson' }
        $match | Should -Not -BeNullOrEmpty
        $match.ToolTip | Should -Be 'the crimson color'
    }
}
