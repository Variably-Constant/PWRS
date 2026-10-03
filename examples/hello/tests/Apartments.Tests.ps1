# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# pwrs::thread::enter_sta puts a thread the module starts in a
# single-threaded COM apartment on Windows and does nothing elsewhere.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'A module thread under pwrs::thread::enter_sta' {
    It 'is single-threaded while it holds the guard on Windows, and has no apartment anywhere else' {
        $r = Get-RustApartment
        if ([IO.Path]::DirectorySeparatorChar -eq '\') {
            # MainSta when the thread is the process's first to enter one.
            $r.Inside | Should -BeIn 'Sta', 'MainSta'
            $r.Before | Should -Not -BeIn 'Sta', 'MainSta'
            $r.After | Should -Not -BeIn 'Sta', 'MainSta'
        } else {
            $r.Before | Should -Be 'NoCom'
            $r.Inside | Should -Be 'NoCom'
            $r.After | Should -Be 'NoCom'
        }
    }
}
