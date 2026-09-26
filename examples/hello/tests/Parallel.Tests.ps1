# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# `par_map` and `par_for_each` run the work on a pool of Rust threads
# and write from the pipeline thread only. These pin what a caller can
# rely on: every item is processed exactly once, input order is kept
# when it is asked for, and the as-ready order is a permutation of the
# same set rather than a different one.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'par_map' {
    It 'writes every result in input order' {
        $out = @(Get-RustParallel -Count 64)
        $out.Count | Should -Be 64
        $out[0] | Should -Be 1
        $out[63] | Should -Be 4096
        ($out -join ',') | Should -Be ((1..64 | ForEach-Object { $_ * $_ }) -join ',')
    }

    It 'writes every result exactly once when the order is as ready' {
        $out = @(Get-RustParallel -Count 256 -AsReady)
        $out.Count | Should -Be 256
        (($out | Sort-Object) -join ',') | Should -Be (((1..256 | ForEach-Object { $_ * $_ }) | Sort-Object) -join ',')
    }

    It 'handles a single item, where the pool is one worker wide' {
        @(Get-RustParallel -Count 1) | Should -Be @(1)
    }

    It 'survives being run many times over' {
        foreach ($round in 1..20) {
            (Get-RustParallel -Count 8 | Measure-Object -Sum).Sum | Should -Be 204
        }
    }
}

Describe 'par_for_each' {
    It 'runs every item and writes only what the cmdlet writes' {
        Measure-RustParallel -Count 1000 | Should -Be 500500
    }

    It 'claims each item once however many workers there are' {
        foreach ($count in 1, 2, 7, 64, 4096) {
            Measure-RustParallel -Count $count | Should -Be ([int64]$count * ($count + 1) / 2)
        }
    }
}

Describe 'A PsObject method on a worker the module started' {
    # cargo pwrs test sets PWRS_THREAD_CHECK=1 for the hosts it runs;
    # a host started any other way runs a release build unchecked.
    It 'is refused while the thread check is on, and runs when it is off' {
        $out = Test-RustOffThread -InputObject 'abc'
        if ($env:PWRS_THREAD_CHECK -eq '1') {
            $out | Should -Be 'refused: PwrsOffThread'
        } else {
            $out | Should -Be 'ran: System.String'
        }
    }

    It 'leaves the same call on the thread the cmdlet runs on alone' {
        Get-RustTypeName -Value 'abc' | Should -Be 'System.String'
    }
}
