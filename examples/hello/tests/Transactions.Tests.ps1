# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Set-RustSetting and Remove-RustSetting declare supports_transactions.
# Windows PowerShell gives such a cmdlet -UseTransaction, and inside
# Start-Transaction the change it enlists lands at Complete-Transaction
# and is dropped by Undo-Transaction; PowerShell 7 gives no compiled
# cmdlet -UseTransaction. The oracle is a compiled cmdlet declaring
# SupportsTransactions that stores a setting the same way, through
# CurrentPSTransaction, run in the same host: each case asserts the PWRS
# cmdlet does what it does.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # Written for the C# the Windows PowerShell compiler accepts. Given
    # references, PowerShell 7's Add-Type takes none of its own, so it is
    # given System.Management.Automation too.
    if (-not ('Pwrs.Tests.SetCompiledSettingCommand' -as [type])) {
        $references = if ($PSVersionTable.PSEdition -eq 'Core') {
            @([psobject].Assembly.Location, 'System.Transactions.Local', 'System.Collections', 'System.Threading', 'System.Runtime', 'netstandard')
        } else {
            @('System.Transactions')
        }
        Add-Type -ReferencedAssemblies $references -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Management.Automation;
using System.Transactions;

namespace Pwrs.Tests
{
    public static class CompiledSettings
    {
        public static readonly Dictionary<string, string> Values = new Dictionary<string, string>();
    }

    public sealed class CompiledSettingChange : IEnlistmentNotification
    {
        private readonly string name;
        private readonly string value;
        private readonly bool refuse;

        public CompiledSettingChange(string name, string value, bool refuse)
        {
            this.name = name;
            this.value = value;
            this.refuse = refuse;
        }

        public void Prepare(PreparingEnlistment enlistment)
        {
            if (refuse) enlistment.ForceRollback(new InvalidOperationException("the setting " + name + " changed after this transaction set it"));
            else enlistment.Prepared();
        }

        public void Commit(Enlistment enlistment)
        {
            lock (CompiledSettings.Values) CompiledSettings.Values[name] = value;
            enlistment.Done();
        }

        public void Rollback(Enlistment enlistment) { enlistment.Done(); }

        public void InDoubt(Enlistment enlistment) { enlistment.Done(); }
    }

    [Cmdlet("Set", "CompiledSetting", SupportsTransactions = true)]
    public sealed class SetCompiledSettingCommand : PSCmdlet
    {
        [Parameter(Mandatory = true, Position = 0)]
        public string Name { get; set; }

        [Parameter(Mandatory = true, Position = 1)]
        public string Value { get; set; }

        [Parameter]
        public SwitchParameter Refuse { get; set; }

        protected override void ProcessRecord()
        {
            if (TransactionAvailable())
            {
                using (CurrentPSTransaction)
                {
                    Transaction.Current.EnlistVolatile(new CompiledSettingChange(Name, Value, Refuse.IsPresent), EnlistmentOptions.None);
                }
            }
            else
            {
                lock (CompiledSettings.Values) CompiledSettings.Values[Name] = Value;
            }
        }
    }
}
'@
    }
    Import-Module -Assembly ([Pwrs.Tests.SetCompiledSettingCommand].Assembly) -Force

    # The value the compiled cmdlet stores for a setting, or nothing.
    function script:Compiled([string] $name) {
        $value = $null
        if ([Pwrs.Tests.CompiledSettings]::Values.TryGetValue($name, [ref] $value)) { $value }
    }

    # The record Complete-Transaction raises, or $null when it commits.
    function script:Completed {
        try {
            Complete-Transaction -ErrorAction Stop
            $null
        } catch {
            $_
        }
    }
}

Describe 'a cmdlet declaring supports_transactions, in Windows PowerShell' -Skip:($PSVersionTable.PSEdition -eq 'Core') {
    AfterEach {
        $open = Get-Transaction -ErrorAction SilentlyContinue
        if ($open -and $open.Status -eq 'Active') { Undo-Transaction }
    }

    It 'has -UseTransaction, as a compiled cmdlet declaring SupportsTransactions has' {
        (Get-Command Set-CompiledSetting).Parameters.ContainsKey('UseTransaction') | Should -BeTrue
        (Get-Command Set-RustSetting).Parameters.ContainsKey('UseTransaction') | Should -BeTrue
        (Get-Command Remove-RustSetting).Parameters.ContainsKey('UseTransaction') | Should -BeTrue
        (Get-Command Get-RustSetting).Parameters.ContainsKey('UseTransaction') | Should -BeFalse
    }

    It 'applies a change Complete-Transaction commits, which nothing sees before it' {
        Set-RustSetting kept old
        Set-CompiledSetting kept old
        Start-Transaction
        Set-RustSetting kept new -UseTransaction
        Set-CompiledSetting kept new -UseTransaction
        Get-RustSetting kept | Should -Be 'old'
        Compiled kept | Should -Be 'old'
        Complete-Transaction
        Get-RustSetting kept | Should -Be 'new'
        Compiled kept | Should -Be 'new'
    }

    It 'drops a change Undo-Transaction rolls back' {
        Set-RustSetting dropped old
        Set-CompiledSetting dropped old
        Start-Transaction
        Set-RustSetting dropped new -UseTransaction
        Set-CompiledSetting dropped new -UseTransaction
        Undo-Transaction
        Get-RustSetting dropped | Should -Be 'old'
        Compiled dropped | Should -Be 'old'
    }

    It 'makes a change at once without -UseTransaction, inside a transaction too' {
        Start-Transaction
        Set-RustSetting direct now
        Set-CompiledSetting direct now
        Get-RustSetting direct | Should -Be 'now'
        Undo-Transaction
        Get-RustSetting direct | Should -Be 'now'
        Compiled direct | Should -Be 'now'
    }

    It 'removes a setting through on_complete when the transaction commits, and leaves it when it rolls back' {
        Set-RustSetting gone here
        Start-Transaction
        Remove-RustSetting gone -UseTransaction
        Get-RustSetting gone | Should -Be 'here'
        Undo-Transaction
        Get-RustSetting gone | Should -Be 'here'
        Start-Transaction
        Remove-RustSetting gone -UseTransaction
        Complete-Transaction
        @(Get-RustSetting gone).Count | Should -Be 0
    }

    It 'fails Complete-Transaction as a compiled resource refusing at prepare does, when the setting changed after the transaction set it' {
        Start-Transaction
        Set-RustSetting raced staged -UseTransaction
        Set-RustSetting raced direct
        $rust = Completed
        (Get-Transaction).Status | Should -Be 'RolledBack'
        Start-Transaction
        Set-CompiledSetting raced staged -UseTransaction -Refuse
        $compiled = Completed
        $rust.FullyQualifiedErrorId | Should -Be $compiled.FullyQualifiedErrorId
        $rust.FullyQualifiedErrorId | Should -Be 'System.Transactions.TransactionAbortedException,Microsoft.PowerShell.Commands.CompleteTransactionCommand'
        $rust.Exception.Message | Should -Be $compiled.Exception.Message
        $rust.Exception.InnerException.Message | Should -BeLike '*the setting raced changed after this transaction set it'
        Get-RustSetting raced | Should -Be 'direct'
    }

    It 'rolls back every change of a transaction when one of them refuses' {
        Set-RustSetting pair-a old
        Set-RustSetting pair-b old
        Start-Transaction
        Set-RustSetting pair-a new -UseTransaction
        Set-RustSetting pair-b new -UseTransaction
        Set-RustSetting pair-b other
        (Completed).Exception.InnerException.Message | Should -BeLike '*the setting pair-b changed after this transaction set it'
        Get-RustSetting pair-a | Should -Be 'old'
        Get-RustSetting pair-b | Should -Be 'other'
    }
}

Describe 'a cmdlet declaring supports_transactions, in PowerShell 7' -Skip:($PSVersionTable.PSEdition -ne 'Core') {
    It 'has no -UseTransaction, as a compiled cmdlet declaring SupportsTransactions has none' {
        (Get-Command Set-CompiledSetting).Parameters.ContainsKey('UseTransaction') | Should -BeFalse
        (Get-Command Set-RustSetting).Parameters.ContainsKey('UseTransaction') | Should -BeFalse
    }

    # PowerShell 7 has the transaction cmdlets on Windows and none of them
    # on Linux or FreeBSD, so the transaction is opened only where
    # Start-Transaction is.
    It 'makes a change at once, inside Start-Transaction where there is one, since it has no transaction' {
        $transactions = [bool] (Get-Command Start-Transaction -ErrorAction SilentlyContinue)
        Set-RustSetting eight then
        if ($transactions) { Start-Transaction }
        Set-RustSetting seven now
        Remove-RustSetting eight
        if ($transactions) { Undo-Transaction }
        Get-RustSetting seven | Should -Be 'now'
        @(Get-RustSetting eight).Count | Should -Be 0
    }
}

Describe 'a cmdlet that does not declare supports_transactions' {
    It 'is refused its transaction, by name' {
        { Test-RustUntransacted -ErrorAction Stop } | Should -Throw '*Test-RustUntransacted does not declare supports_transactions*'
    }
}
