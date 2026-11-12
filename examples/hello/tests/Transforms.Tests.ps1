# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A transform runs before the binder converts the argument to the
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

    It 'lets the binder refuse what the transform returned unchanged' {
        # Returned untouched, so the failure is the engine's own
        # conversion of a word to a long.
        { Get-RustSize -Size 'not a size at all' } | Should -Throw
    }
}

# Split-RustEven's -Number names even_only with validate_with. The
# engine runs it after converting the argument to a long, so text and a
# double that convert to an even number pass, and a refusal is the
# binder's validation failure. The oracle for a piped value is
# Start-Sleep, a compiled cmdlet whose piped -Seconds a ValidateRange
# checks: the value it refuses fails its record alone.
Describe 'a validate_with function' {
    It 'lets through an argument the function accepts' {
        Split-RustEven -Number 10 | Should -Be 5
        Split-RustEven 4 | Should -Be 2
    }

    It 'sees the value after the binder converted it to the parameter''s type' {
        Split-RustEven -Number '8' | Should -Be 4
        Split-RustEven -Number 6.0 | Should -Be 3
        { Split-RustEven -Number '7' } | Should -Throw '*7 is odd*'
    }

    It 'refuses an argument with the function''s message, as a validation failure naming the parameter' {
        $refusal = $null
        try { Split-RustEven -Number 3 } catch { $refusal = $_ }
        $refusal | Should -Not -BeNullOrEmpty
        $refusal.Exception | Should -BeOfType [System.Management.Automation.ParameterBindingValidationException]
        $refusal.FullyQualifiedErrorId | Should -BeLike 'ParameterArgumentValidationError,*'
        $refusal.Exception.Message | Should -BeLike "*'Number'*3 is odd; -Number takes an even number*"
    }

    It 'refuses a piped value for its record alone, as a compiled cmdlet''s validation does' {
        $rustErrors = $null
        $written = @(2, 3, 6 | Split-RustEven -ErrorAction SilentlyContinue -ErrorVariable rustErrors)
        $written | Should -Be @(1, 3)
        $compiledErrors = $null
        0, -1, 0 | Start-Sleep -ErrorAction SilentlyContinue -ErrorVariable compiledErrors
        @($rustErrors).Count | Should -Be 1
        @($rustErrors).Count | Should -Be @($compiledErrors).Count
        $rustErrors[0].Exception.GetType() | Should -Be $compiledErrors[0].Exception.GetType()
        ($rustErrors[0].FullyQualifiedErrorId -split ',')[0] | Should -Be ($compiledErrors[0].FullyQualifiedErrorId -split ',')[0]
    }
}

# A refusal's text is the binder's sentence naming the parameter and then
# the function's message alone, as PowerShell's own attributes give theirs.
# Each case reads that sentence from a script function's refusal of a
# parameter of the same name. The error's id and category are in the Data
# of the error's own exception, which is inside the binder's.
Describe 'the text of a refusal, and the id it keeps' {
    BeforeAll {
        # A script function whose -Level, -Number and -Size the engine
        # refuses: a range for a validation failure, a word for a
        # transformation failure.
        function Test-Oracle {
            param([ValidateRange(0, 11)][long] $Level, [ValidateRange(0, 0)][long] $Number, [long] $Size)
        }

        # The binder's sentence before the refusal's own message.
        function Get-Sentence([scriptblock] $call, [string] $parameter) {
            $message = ''
            try { & $call } catch { $message = $_.Exception.Message }
            $named = "'$parameter'. "
            $message.Substring(0, $message.IndexOf($named) + $named.Length)
        }

        # The first exception in the chain whose Data has an error's id.
        function Find-Error([Exception] $e) {
            while ($e) {
                if ($e.Data.Contains('PwrsErrorId')) { return $e }
                $e = $e.InnerException
            }
            $null
        }
    }

    It 'is the binder''s sentence and the message alone for a validate_with function' {
        $sentence = Get-Sentence { Test-Oracle -Level 12 } 'Level'
        $sentence | Should -BeLike "*'Level'. "
        $refusal = $null
        try { Get-RustLevel 12 } catch { $refusal = $_ }
        $refusal.Exception | Should -BeOfType [System.Management.Automation.ParameterBindingValidationException]
        $refusal.Exception.Message | Should -BeExactly ($sentence + '12 is not a level from 0 to 11')
        $sentence = Get-Sentence { Test-Oracle -Number 3 } 'Number'
        $refusal = $null
        try { Split-RustEven -Number 3 } catch { $refusal = $_ }
        $refusal.Exception.Message | Should -BeExactly ($sentence + '3 is odd; -Number takes an even number')
    }

    It 'is the binder''s sentence and the message alone for a transform' {
        $sentence = Get-Sentence { Test-Oracle -Level loud } 'Level'
        $sentence | Should -BeLike "*'Level'. "
        $refusal = $null
        try { Get-RustLevel loud } catch { $refusal = $_ }
        $refusal.Exception | Should -BeOfType [System.Management.Automation.ParameterBindingArgumentTransformationException]
        $refusal.Exception.Message | Should -BeExactly ($sentence + 'loud is not a level; give a number, off or max')
        $sentence = Get-Sentence { Test-Oracle -Size xMB } 'Size'
        $refusal = $null
        try { Get-RustSize -Size xMB } catch { $refusal = $_ }
        $refusal.Exception.Message | Should -BeExactly ($sentence + 'xMB is not a number followed by KB, MB or GB')
    }

    It 'keeps the id of an error that names no exception type, in a PwrsException inside the binder''s' {
        $refusal = $null
        try { Get-RustLevel 12 } catch { $refusal = $_ }
        $kept = Find-Error $refusal.Exception
        $kept.GetType().FullName | Should -Be 'Pwrs.PwrsException'
        $kept.Message | Should -BeExactly '12 is not a level from 0 to 11'
        $kept.Data['PwrsErrorId'] | Should -Be 'HelloLevel'
        $kept.Data['PwrsErrorCategory'] | Should -Be 'InvalidArgument'
        $kept.InnerException.Message | Should -BeExactly 'the dial stops at 11'
        $refusal = $null
        try { Get-RustLevel loud } catch { $refusal = $_ }
        $kept = Find-Error $refusal.Exception
        $kept.GetType().FullName | Should -Be 'Pwrs.PwrsException'
        $kept.Message | Should -BeExactly 'loud is not a level; give a number, off or max'
        $kept.Data['PwrsErrorId'] | Should -Be 'HelloLevelWord'
        $kept.InnerException | Should -BeNullOrEmpty
    }

    It 'binds what the transform and the function let through' {
        Get-RustLevel off | Should -Be 'level 0'
        Get-RustLevel max | Should -Be 'level 11'
        Get-RustLevel 7 | Should -Be 'level 7'
        Get-RustLevel '7' | Should -Be 'level 7'
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
