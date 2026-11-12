# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Measure-RustString's -InputObject is a PsString: declared [string], so
# it binds as a compiled cmdlet's [string] parameter does, which the cases
# check against one compiled here, and held in Rust as the caller's own
# string object. The identity hash Rust computes for it is compared with
# the caller's, and the module counts each string object it is handed in
# a ConditionalWeakTable, which keeps no string alive.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # A compiled cmdlet with the same two parameters, as the module's shell
    # is one, writing each string it was given.
    if (-not (Get-Command Test-StringOracle -ErrorAction SilentlyContinue)) {
        $oracle = Add-Type -PassThru -TypeDefinition @'
using System.Management.Automation;
[Cmdlet(VerbsDiagnostic.Test, "StringOracle")]
public sealed class TestStringOracleCommand : PSCmdlet
{
    [Parameter(Mandatory = true, Position = 0, ValueFromPipeline = true)]
    public string InputObject { get; set; }
    [Parameter]
    public string[] Also { get; set; }
    protected override void ProcessRecord()
    {
        WriteObject(InputObject);
        if (Also != null) { foreach (string s in Also) { WriteObject(s); } }
    }
}
'@
        Import-Module -Assembly ($oracle | Select-Object -First 1).Assembly
    }

    function Get-UnitSum([string] $Text) {
        $sum = 0
        foreach ($c in $Text.ToCharArray()) { $sum += [int]$c }
        $sum
    }

    # What one call gives, in one line: the length and unit sum of each
    # string, or the error's id without its command and the message.
    function Get-Outcome([scriptblock] $Call, [switch] $Readings) {
        try {
            $out = @(& $Call)
            $parts = foreach ($o in $out) {
                if ($Readings) { "$($o.Units)/$($o.UnitSum)" } else { "$($o.Length)/$(Get-UnitSum $o)" }
            }
            'text ' + ($parts -join ' ')
        } catch {
            "error $(($_.FullyQualifiedErrorId -split ',')[0]): $($_.Exception.Message)"
        }
    }
}

Describe 'a PsString parameter' {
    It 'is declared [string], and an array of them [string[]]' {
        $parameters = (Get-Command Measure-RustString).Parameters
        $parameters['InputObject'].ParameterType | Should -Be ([string])
        $parameters['Also'].ParameterType | Should -Be ([string[]])
    }

    It 'binds as a compiled cmdlet''s [string] parameter does, value or refusal' {
        $file = Get-Item -LiteralPath (Join-Path $env:PWRS_MODULE 'Hello.psd1')
        $cases = [ordered]@{
            'an int'                = { param($c) & $c 5 }
            'a double'              = { param($c) & $c 2.5 }
            'a FileInfo argument'   = { param($c) & $c $file }
            'a piped FileInfo'      = { param($c) $file | & $c }
            'an array'              = { param($c) & $c @('a', 'b') }
            'null'                  = { param($c) & $c $null }
            'an empty string'       = { param($c) & $c '' }
            'a hashtable'           = { param($c) & $c @{ a = 1 } }
        }
        foreach ($name in $cases.Keys) {
            $case = $cases[$name]
            $compiled = Get-Outcome { & $case 'Test-StringOracle' }
            $rust = Get-Outcome { & $case 'Measure-RustString' } -Readings
            $rust | Should -Be $compiled -Because $name
        }
    }

    It 'holds the caller''s own string, by position, by pipeline and in an array' {
        $text = 'abc' * 10000
        $first = 'first'
        $second = 'second'
        $hash = { param($s) [System.Runtime.CompilerServices.RuntimeHelpers]::GetHashCode($s) }
        (Measure-RustString $text).Identity | Should -Be (& $hash $text)
        ($text | Measure-RustString).Identity | Should -Be (& $hash $text)
        $readings = @(Measure-RustString $text -Also $first, $second)
        $readings.Count | Should -Be 3
        $readings[1].Identity | Should -Be (& $hash $first)
        $readings[2].Identity | Should -Be (& $hash $second)
    }

    It 'reads the UTF-16 units in place, past the BMP and at a megabyte' {
        $accented = -join [char[]](0x68, 0xE9, 0x6C, 0x6C, 0x6F, 0x20, 0x77, 0xF6, 0x72, 0x6C, 0x64)
        $astral = 'clef ' + [char]::ConvertFromUtf32(0x1D11E) + ' ' + [char]0x2713
        $large = 'x' * (1024 * 1024)
        foreach ($text in 'abc', $accented, $astral, $large) {
            $reading = Measure-RustString $text
            $reading.Units | Should -Be $text.Length
            $reading.UnitSum | Should -Be (Get-UnitSum $text)
        }
    }

    It 'counts one string object each time it is handed, and an equal copy apart' {
        $text = "seen $([guid]::NewGuid())"
        $copy = [string]::new($text.ToCharArray())
        [object]::ReferenceEquals($text, $copy) | Should -BeFalse
        (Measure-RustString $text).Seen | Should -Be 1
        ($text | Measure-RustString).Seen | Should -Be 2
        (Measure-RustString $copy).Seen | Should -Be 1
        $readings = @(Measure-RustString $copy -Also $text)
        $readings[0].Seen | Should -Be 2
        $readings[1].Seen | Should -Be 3
    }

    It 'reads a $null element of -Also as no units and never seen' {
        $readings = @(Measure-RustString 'x' -Also 'a', $null)
        $readings.Count | Should -Be 3
        $readings[2].Units | Should -Be 0
        $readings[2].Identity | Should -Be 0
        $readings[2].Seen | Should -Be 0
    }

    It 'keeps no string alive once the command has ended' {
        function Measure-AndForget {
            $text = "forgotten $([guid]::NewGuid()) " * 100
            $null = Measure-RustString $text
            [WeakReference]::new($text)
        }
        $weak = Measure-AndForget
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
        [GC]::Collect()
        $weak.IsAlive | Should -BeFalse
    }
}
