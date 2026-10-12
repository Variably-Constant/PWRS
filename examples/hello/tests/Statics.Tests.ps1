# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A shell type resolves by name on both hosts once the shell is
# imported as a module, which the enum tests already rely on, so
# [Hello.Counter] reaches the generated class, its constructor and its
# statics.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'a constructor a script reaches as ::new' {
    It 'makes a proxy object holding the Rust value' {
        $c = [Hello.Counter]::new('made', 5)
        try {
            $c | Should -BeOfType [Hello.Counter]
            $c.Label | Should -Be 'made'
            $c.Value | Should -Be 5
            $c.IsDisposed | Should -BeFalse
        } finally {
            $c.Dispose()
        }
    }

    It 'gives an object whose methods run against that value' {
        $c = [Hello.Counter]::new('m', 10)
        try {
            $c.Advance(5) | Should -Be 15
            $c.Value | Should -Be 15
            $c.Describe() | Should -Be 'm=15'
        } finally {
            $c.Dispose()
        }
    }

    It 'frees the value exactly once on Dispose' {
        $c = [Hello.Counter]::new('d', 1)
        $c.Dispose()
        $c.IsDisposed | Should -BeTrue
        { $c.Advance(1) } | Should -Throw
    }

    It 'raises the Rust error of a constructor, and the collector finalizes the object it left without ending the process' {
        { [Hello.Counter]::new('a=b', 1) } | Should -Throw "*a counter's label cannot hold '=': a=b*"
        # A child host of this edition makes the failed object, then
        # collects and runs finalizers: a finalizer that threw would end
        # it before it printed survived.
        $hostPath = (Get-Process -Id $PID).Path
        $script = "Import-Module '$(Join-Path $env:PWRS_MODULE 'Hello.psd1')' -ErrorAction Stop; " +
            "try { `$null = [Hello.Counter]::new('a=b', 1); 'made' } catch { 'refused' }; " +
            "[GC]::Collect(); [GC]::WaitForPendingFinalizers(); [GC]::Collect(); [GC]::WaitForPendingFinalizers(); 'survived'"
        $out = @(& $hostPath -NoProfile -NonInteractive -Command $script 2>&1 | ForEach-Object { "$_" })
        $LASTEXITCODE | Should -Be 0 -Because ($out -join "`n")
        $out | Should -Contain 'refused'
        $out | Should -Contain 'survived'
    }
}

Describe 'a static method' {
    It 'answers without an object to call it on' {
        [Hello.Counter]::Limit() | Should -Be ([long]::MaxValue)
    }

    It 'returns a new proxy object when it returns the class' {
        $c = [Hello.Counter]::Parse('parsed=42')
        try {
            $c | Should -BeOfType [Hello.Counter]
            $c.Label | Should -Be 'parsed'
            $c.Value | Should -Be 42
        } finally {
            $c.Dispose()
        }
    }

    It 'raises a Rust error as a terminating one' {
        { [Hello.Counter]::Parse('no equals sign') } | Should -Throw
        { [Hello.Counter]::Parse('x=notanumber') } | Should -Throw
    }

    It 'is on the type and not on an instance' {
        ([Hello.Counter] | Get-Member -Static -Name Limit -MemberType Method) | Should -Not -BeNullOrEmpty
        $c = [Hello.Counter]::new('i', 1)
        try {
            ($c | Get-Member -Name Limit -MemberType Method) | Should -BeNullOrEmpty
        } finally {
            $c.Dispose()
        }
    }
}
