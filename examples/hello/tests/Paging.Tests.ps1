# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Get-RustPage declares supports_paging, so the engine gives it -First,
# -Skip and -IncludeTotalCount, and the cmdlet applies them itself.
# The oracle is a compiled cmdlet declaring SupportsPaging that does the
# same through PagingParameters, run in the same host under the same
# switches: each case asserts the PWRS cmdlet writes what it writes,
# the count object included, whose text the engine localizes. That text
# is the object's ToString, which display uses; interpolating it in a
# string gives the bare number.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # Written for the C# the Windows PowerShell compiler accepts, so no
    # property initializers or expression-bodied members.
    if (-not ('Pwrs.Tests.GetCompiledPageCommand' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Management.Automation;

namespace Pwrs.Tests
{
    [Cmdlet("Get", "CompiledPage", SupportsPaging = true)]
    public sealed class GetCompiledPageCommand : PSCmdlet
    {
        private double accuracy = 1.0;

        [Parameter(Mandatory = true, Position = 0)]
        public long Count { get; set; }

        [Parameter]
        public double Accuracy { get { return accuracy; } set { accuracy = value; } }

        protected override void ProcessRecord()
        {
            ulong total = (ulong)Math.Max(0L, Count);
            if (PagingParameters.IncludeTotalCount.IsPresent)
            {
                WriteObject(PagingParameters.NewTotalCount(total, accuracy));
            }
            ulong start = Math.Min(PagingParameters.Skip, total);
            ulong end = start + Math.Min(PagingParameters.First, total - start);
            for (ulong n = start; n < end; n++)
            {
                WriteObject((long)n + 1);
            }
        }
    }
}
'@
    }
    Import-Module -Assembly ([Pwrs.Tests.GetCompiledPageCommand].Assembly) -Force
}

Describe 'a cmdlet declaring supports_paging' {
    It 'writes the page a compiled cmdlet writes under each -Skip and -First' {
        foreach ($case in @(@{ Skip = 2; First = 3 }, @{ Skip = 0; First = 1 }, @{ Skip = 8; First = 5 }, @{ Skip = 20; First = 2 }, @{ First = 0 }, @{ Skip = 4 }, @{})) {
            $rust = @(Get-RustPage 10 @case | ForEach-Object { "$_" }) -join ','
            $compiled = @(Get-CompiledPage 10 @case | ForEach-Object { "$_" }) -join ','
            $rust | Should -Be $compiled -Because (($case.Keys | ForEach-Object { "-$_ $($case[$_])" }) -join ' ')
        }
        @(Get-RustPage 10 -Skip 2 -First 3) | Should -Be @(3, 4, 5)
    }

    It 'writes the count first under -IncludeTotalCount, as the engine makes it' {
        $rust = @(Get-RustPage 10 -Skip 2 -First 3 -IncludeTotalCount)
        $compiled = @(Get-CompiledPage 10 -Skip 2 -First 3 -IncludeTotalCount)
        $rust.Count | Should -Be 4
        $rust[0].ToString() | Should -Be $compiled[0].ToString()
        $rust[0].ToString() | Should -Not -Be '10' -Because 'the count has the engine''s text, not only the number'
        $rust[0].psobject.BaseObject | Should -BeOfType [uint64]
        $rust[0].psobject.BaseObject | Should -Be 10
        $rust[0].Accuracy | Should -Be 1.0
        @($rust[1..3]) | Should -Be @(3, 4, 5)
    }

    It 'says a count not known and an estimate as the engine says them' {
        foreach ($accuracy in 0.0, 0.5) {
            $rust = @(Get-RustPage 10 -First 0 -IncludeTotalCount -Accuracy $accuracy)
            $compiled = @(Get-CompiledPage 10 -First 0 -IncludeTotalCount -Accuracy $accuracy)
            $rust.Count | Should -Be 1
            $rust[0].ToString() | Should -Be $compiled[0].ToString()
            $rust[0].Accuracy | Should -Be $compiled[0].Accuracy
        }
        $exact = @(Get-RustPage 10 -First 0 -IncludeTotalCount)[0].ToString()
        $unknown = @(Get-RustPage 10 -First 0 -IncludeTotalCount -Accuracy 0.0)[0].ToString()
        $estimate = @(Get-RustPage 10 -First 0 -IncludeTotalCount -Accuracy 0.5)[0].ToString()
        @($exact, $unknown, $estimate | Select-Object -Unique).Count | Should -Be 3
    }

    It 'has the parameters the engine adds for paging, and a cmdlet without paging has none' {
        $parameters = (Get-Command Get-RustPage).Parameters
        foreach ($name in 'First', 'Skip', 'IncludeTotalCount') {
            $parameters.ContainsKey($name) | Should -BeTrue -Because $name
        }
        $parameters['First'].ParameterType | Should -Be ([uint64])
        (Get-Command Test-RustUnpaged).Parameters.ContainsKey('First') | Should -BeFalse
    }

    It 'refuses to read paging for a cmdlet that does not declare it' {
        { Test-RustUnpaged -ErrorAction Stop } | Should -Throw '*Test-RustUnpaged does not declare supports_paging*'
    }
}

Describe 'the help address and remoting capability a cmdlet declares' {
    It 'gives Get-Help -Online the help_uri' {
        (Get-Command Get-RustPage).HelpUri | Should -Be 'https://variably-constant.github.io/PWRS/'
    }

    It 'reports the remoting capability declared, and PowerShell for a cmdlet declaring none' {
        (Get-Command Get-RustPage).RemotingCapability | Should -Be ([System.Management.Automation.RemotingCapability]::None)
        (Get-Command Get-Greeting).RemotingCapability | Should -Be ([System.Management.Automation.RemotingCapability]::PowerShell)
    }
}
