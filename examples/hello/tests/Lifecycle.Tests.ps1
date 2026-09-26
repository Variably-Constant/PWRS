# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# The counts live in the native library, which a removal does not
# unload, so they carry across a Remove-Module and the import that
# follows it. That is the only way the removal hook can be observed:
# once the module is removed its cmdlets are gone, and only the next
# import can report what ran.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $manifest = Join-Path $module 'Hello.psd1'
    Import-Module $manifest -Force -ErrorAction Stop
}

Describe 'module lifecycle hooks' {
    It 'ran the import hook when the module was imported' {
        Get-RustLifecycle | Should -BeGreaterOrEqual 1
    }

    It 'runs the removal hook on Remove-Module and the import hook again on the next import' {
        $imports = Get-RustLifecycle
        $removes = Get-RustLifecycle -Removes
        Remove-Module Hello -Force -ErrorAction Stop
        Import-Module $manifest -Force -ErrorAction Stop
        Get-RustLifecycle -Removes | Should -Be ($removes + 1)
        Get-RustLifecycle | Should -Be ($imports + 1)
    }

    It 'has run the import hook exactly once more than the removal hook' {
        # Every removal in a session was preceded by an import, and
        # the module is imported now, so the two differ by one.
        Get-RustLifecycle | Should -Be ((Get-RustLifecycle -Removes) + 1)
    }
}
