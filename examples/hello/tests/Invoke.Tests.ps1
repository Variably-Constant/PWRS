# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Get-RustComposed runs another command from Rust by name, with no
# script block built, so what it writes is that command's own output
# and what it raises is that command's own failure.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # A module function whose dynamicparam block reads its module's
    # $script: state, as a CDXML function such as New-ScheduledTaskAction
    # does. Run anywhere but in its module's scope, that block fails with
    # a method call on a null-valued expression.
    New-Module -Name PwrsScopeProbe -ScriptBlock {
        $script:mode = 'fast'
        function Get-PwrsScopeProbe {
            [CmdletBinding()]
            param([string]$Name)
            dynamicparam {
                $attributes = [System.Collections.ObjectModel.Collection[System.Attribute]]::new()
                $attributes.Add([System.Management.Automation.ParameterAttribute]::new())
                $key = $script:mode.ToUpperInvariant()
                $dynamic = [System.Management.Automation.RuntimeDefinedParameterDictionary]::new()
                $dynamic.Add($key, [System.Management.Automation.RuntimeDefinedParameter]::new($key, [string], $attributes))
                $dynamic
            }
            end { "$Name in $script:mode" }
        }
        Export-ModuleMember -Function Get-PwrsScopeProbe
    } | Import-Module -Global
}

AfterAll {
    Remove-Module PwrsScopeProbe
}

Describe 'invoking a command by name from Rust' {
    It 'runs a cmdlet of this module with a parameter bound by name' {
        Get-RustComposed -Name Ada | Should -Be (Get-Greeting -Name Ada)
    }

    It 'runs an engine cmdlet and writes what it wrote' {
        (Get-RustComposed -Command Get-Location).Path | Should -Be (Get-Location).Path
    }

    It 'runs another module''s function in that module''s scope, where its dynamicparam block reads the module''s state' {
        Get-RustComposed -Command Get-PwrsScopeProbe -Name Ada | Should -Be 'Ada in fast'
    }

    It 'pipes input into a command, unrolling a collection' {
        Get-RustComposed -Sort 3, 1, 2 | Should -Be @(1, 2, 3)
    }

    It 'raises a terminating error for a command the session cannot see, with PowerShell''s own message' {
        { Get-RustComposed -Command No-SuchCommandAnywhere -ErrorAction Stop } | Should -Throw -ExpectedMessage "*CommandNotFoundException*'No-SuchCommandAnywhere'*"
    }

    It 'forwards the command''s non-terminating error to this cmdlet''s stream and keeps going' {
        # Get-Item on a path that does not exist writes an error record
        # and no output; with the error reaching the caller's stream it
        # is a record here, not an exception, and -ErrorAction decides.
        $records = @(Get-RustComposed -Command Get-Item -Name ([System.IO.Path]::Combine($TestDrive, 'absent')) -ErrorAction SilentlyContinue -ErrorVariable e)
        $records.Count | Should -Be 0
        $e.Count | Should -BeGreaterOrEqual 1
    }
}
