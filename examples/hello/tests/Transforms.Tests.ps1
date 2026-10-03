# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A transform runs before the binder coerces the argument to the
# parameter's declared type, so what these cases prove is where it
# runs as much as what it computes: -Size is a long, and a string
# with a suffix reaches it only because the transform turned it into
# a number first. A refusal is a binding failure, not an error the
# cmdlet wrote, so it is caught as a terminating error even without
# -ErrorAction Stop.
#
# The mirror-enum cases use System.ConsoleColor, a CLR enum the
# module never declared. The parameter is typed as the real thing, so
# the binder converts its member names and the value written back is
# that type and not a number.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'an argument transformation' {
    It 'turns a suffixed string into bytes before the parameter sees it' {
        Get-RustSize -Size 4KB | Should -Be 4096
        Get-RustSize -Size '2MB' | Should -Be 2097152
        Get-RustSize -Size '1GB' | Should -Be 1073741824
    }

    It 'leaves a plain number alone' {
        Get-RustSize -Size 512 | Should -Be 512
        Get-RustSize 0 | Should -Be 0
    }

    It 'refuses a suffix on something that is not a number, before the body runs' {
        { Get-RustSize -Size 'twelveMB' } | Should -Throw
    }

    It 'names the parameter in the refusal, the way a binding failure does' {
        $message = ''
        try { Get-RustSize -Size 'twelveMB' } catch { $message = $_.Exception.Message }
        $message | Should -Match 'twelveMB'
    }

    It 'lets the binder refuse what the transform passed through' {
        # Handed back untouched, so the failure is the engine's own
        # conversion of a word to a long.
        { Get-RustSize -Size 'not a size at all' } | Should -Throw
    }
}

Describe 'a CLR enum this module did not declare' {
    It 'binds a parameter typed as the real System.ConsoleColor' {
        Get-RustInk -Color Red -Describe | Should -Be 'Red'
        Get-RustInk -Color ([System.ConsoleColor]::DarkBlue) -Describe | Should -Be 'DarkBlue'
    }

    It 'writes a value of that type and not a number' {
        $next = Get-RustInk -Color Black
        $next.GetType().FullName | Should -Be 'System.ConsoleColor'
        $next | Should -Be ([System.ConsoleColor]::DarkBlue)
        [int] $next | Should -Be 1
    }

    It 'wraps around at the last of the four it names' {
        Get-RustInk -Color White | Should -Be ([System.ConsoleColor]::Black)
    }

    It 'refuses a member of the CLR enum the Rust enum does not name' {
        # Green is a ConsoleColor, so the binder accepts it; the Rust
        # enum has no variant for 10 and says so.
        { Get-RustInk -Color Green -ErrorAction Stop } | Should -Throw
    }

    It 'refuses a name that is not a ConsoleColor at all, in the binder' {
        { Get-RustInk -Color Puce } | Should -Throw
    }

    It 'declares the parameter as the CLR type, so the engine completes its members' {
        (Get-Command Get-RustInk).Parameters['Color'].ParameterType.FullName | Should -Be 'System.ConsoleColor'
    }
}
