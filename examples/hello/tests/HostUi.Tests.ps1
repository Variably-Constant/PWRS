# Runs under Pester 6 in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# The host these tests run in is -NonInteractive and cannot reply to a
# prompt, so each prompting case runs the cmdlet in a runspace whose
# host replies from a queue and records what was written to it. The
# cmdlet reaches that host through the same $Host.UI a console gives
# it, so what is exercised is the module's own path to the host. The
# host that cannot reply is its own case. The prompts -Debug makes the
# engine show are answered the same way, and a PWRS cmdlet writes,
# prompts and stops under them as the built-in compiled Write-* cmdlets do.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $manifest = Join-Path $module 'Hello.psd1'
    Import-Module $manifest -Force -ErrorAction Stop
    $script:manifest = $manifest

    # Written for the C# the Windows PowerShell compiler accepts, so
    # no expression-bodied members.
    if (-not ('Pwrs.Tests.ScriptedHost' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Globalization;
using System.Management.Automation;
using System.Management.Automation.Host;
using System.Security;

namespace Pwrs.Tests
{
    public sealed class ScriptedUi : PSHostUserInterface
    {
        public Queue<string> Answers = new Queue<string>();
        public List<string> Written = new List<string>();
        public List<string> Offered = new List<string>();

        public override PSHostRawUserInterface RawUI { get { return null; } }

        public override string ReadLine() { return Answers.Dequeue(); }

        public override SecureString ReadLineAsSecureString()
        {
            SecureString secret = new SecureString();
            foreach (char c in Answers.Dequeue()) { secret.AppendChar(c); }
            secret.MakeReadOnly();
            return secret;
        }

        public override int PromptForChoice(string caption, string message, Collection<ChoiceDescription> choices, int defaultChoice)
        {
            foreach (ChoiceDescription choice in choices) { Offered.Add(choice.Label); }
            string answer = Answers.Dequeue();
            if (answer.Length == 0) { return defaultChoice; }
            for (int i = 0; i < choices.Count; i++)
            {
                string label = choices[i].Label.Replace("&", "");
                if (string.Equals(label, answer, StringComparison.OrdinalIgnoreCase)
                    || string.Equals(label.Substring(0, 1), answer, StringComparison.OrdinalIgnoreCase))
                {
                    return i;
                }
            }
            return -1;
        }

        public override void Write(string value) { Written.Add(value); }
        public override void Write(ConsoleColor foreground, ConsoleColor background, string value) { Written.Add(value); }
        public override void WriteLine(string value) { Written.Add(value); }
        public override void WriteErrorLine(string value) { Written.Add(value); }
        public override void WriteDebugLine(string message) { }
        public override void WriteProgress(long sourceId, ProgressRecord record) { }
        public override void WriteVerboseLine(string message) { }
        public override void WriteWarningLine(string message) { }

        public override Dictionary<string, PSObject> Prompt(string caption, string message, Collection<FieldDescription> descriptions)
        {
            throw new NotSupportedException();
        }

        // The user name the prompt names is recorded in Offered; the next
        // answer is the password.
        public override PSCredential PromptForCredential(string caption, string message, string userName, string targetName)
        {
            Offered.Add(userName);
            SecureString secret = new SecureString();
            foreach (char c in Answers.Dequeue()) { secret.AppendChar(c); }
            secret.MakeReadOnly();
            return new PSCredential(userName, secret);
        }

        public override PSCredential PromptForCredential(string caption, string message, string userName, string targetName, PSCredentialTypes allowedCredentialTypes, PSCredentialUIOptions options)
        {
            return PromptForCredential(caption, message, userName, targetName);
        }
    }

    public sealed class ScriptedHost : PSHost
    {
        // Named for what it is rather than Ui: a public field
        // differing from the inherited UI property only in casing
        // is not CLS compliant, and the engine's type adapter
        // refuses the whole type over it.
        public ScriptedUi Scripted = new ScriptedUi();
        private readonly Guid id = Guid.NewGuid();

        public override CultureInfo CurrentCulture { get { return CultureInfo.InvariantCulture; } }
        public override CultureInfo CurrentUICulture { get { return CultureInfo.InvariantCulture; } }
        public override Guid InstanceId { get { return id; } }
        public override string Name { get { return "Scripted"; } }
        public override PSHostUserInterface UI { get { return Scripted; } }
        public override Version Version { get { return new Version(1, 0); } }
        public override void EnterNestedPrompt() { }
        public override void ExitNestedPrompt() { }
        public override void NotifyBeginApplication() { }
        public override void NotifyEndApplication() { }
        public override void SetShouldExit(int exitCode) { }
    }
}
'@
    }

    # Defined here rather than at the top of the file: a function the
    # file defines outside BeforeAll is not in scope while the tests
    # run. Imports into the scripted host's runspace and removes the
    # module again, so the import and removal hooks stay paired for
    # the lifecycle counts.
    function Answer([string] $typed, [string] $command) {
        $fake = New-Object Pwrs.Tests.ScriptedHost
        $fake.Scripted.Answers.Enqueue($typed)
        $runspace = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspace($fake)
        $runspace.Open()
        $shell = $null
        try {
            $shell = [System.Management.Automation.PowerShell]::Create()
            $shell.Runspace = $runspace
            $null = $shell.AddScript("Import-Module '$script:manifest' -ErrorAction Stop; try { $command } finally { Remove-Module Hello }")
            $output = @($shell.Invoke())
            if ($shell.Streams.Error.Count -gt 0) { throw $shell.Streams.Error[0].Exception }
            [pscustomobject]@{ Output = $output; Written = @($fake.Scripted.Written); Offered = @($fake.Scripted.Offered) }
        } finally {
            if ($shell) { $shell.Dispose() }
            $runspace.Close()
        }
    }

    # Runs $command in a scripted host's runspace, every stream merged into
    # output, answering each question with the next of $typed. Gives the
    # type name of each record written before the command ended, the full
    # name of the exception it ended with, if any, and the choices offered.
    function Answered([string[]] $typed, [string] $command) {
        $fake = New-Object Pwrs.Tests.ScriptedHost
        foreach ($one in $typed) { $fake.Scripted.Answers.Enqueue($one) }
        $runspace = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspace($fake)
        $runspace.Open()
        $shell = $null
        try {
            $shell = [System.Management.Automation.PowerShell]::Create()
            $shell.Runspace = $runspace
            $null = $shell.AddScript("Import-Module '$script:manifest' -ErrorAction Stop; `$seen = New-Object System.Collections.Generic.List[string]; try { $command *>&1 | ForEach-Object { `$seen.Add(`$_.GetType().Name) } } finally { Remove-Module Hello }")
            $ended = $null
            try {
                $null = $shell.Invoke()
            } catch {
                $e = $_.Exception
                while ($e -is [System.Management.Automation.MethodInvocationException] -and $e.InnerException) { $e = $e.InnerException }
                $ended = $e.GetType().FullName
            }
            [pscustomobject]@{ Seen = @($runspace.SessionStateProxy.GetVariable('seen')); Ended = $ended; Offered = @($fake.Scripted.Offered) }
        } finally {
            if ($shell) { $shell.Dispose() }
            $runspace.Close()
        }
    }
}

Describe 'prompting through the host' {
    It 'reads the line the person typed' {
        (Answer 'typed text' 'Read-RustHost Line').Output | Should -Be 'typed text'
    }

    It 'reads a line without echo and reports only its length' {
        $asked = Answer 'hunter2' 'Read-RustHost Secure'
        $asked.Output | Should -Be 7
        $asked.Written | Should -Not -Contain 'hunter2'
    }

    It 'offers choices and returns the index of the one chosen' {
        $asked = Answer 'N' "Read-RustHost Choice -Choices '&Yes', '&No'"
        $asked.Offered | Should -Be @('&Yes', '&No')
        $asked.Output | Should -Be 1
    }

    It 'takes the default on an empty reply' {
        (Answer '' "Read-RustHost Choice -Choices '&Yes', '&No'").Output | Should -Be 0
    }

    It 'writes to the host before prompting, off the pipeline' {
        $asked = Answer 'ok' "Read-RustHost Line -Say 'first, on the host'"
        $asked.Written | Should -Be @('first, on the host')
        $asked.Output | Should -Be @('ok')
    }

    It 'refuses in a host that cannot prompt, with the engine''s own error' {
        { Read-RustHost Line -ErrorAction Stop } | Should -Throw
    }
}

# Test-RustCredential's -Credential is declared with [Credential()], as
# Get-Credential's own -Credential is, so a user name given in place of
# a credential binds by prompting for the password through the host.
# Get-Credential
# is the compiled cmdlet each case is checked against.
Describe 'a credential parameter' {
    It 'is declared with [Credential()]' {
        $attributes = (Get-Command Test-RustCredential).Parameters['Credential'].Attributes
        @($attributes | Where-Object { $_ -is [System.Management.Automation.CredentialAttribute] }).Count | Should -Be 1
    }

    It 'takes a user name and prompts for the password through the host, as Get-Credential''s -Credential does' {
        $pwrs = Answer 'hunter2' 'Test-RustCredential -Credential ada'
        $compiled = Answer 'hunter2' '$c = Get-Credential -Credential ada; "$($c.UserName):$($c.GetNetworkCredential().Password.Length)"'
        $pwrs.Output | Should -Be @('ada:7')
        $pwrs.Output | Should -Be $compiled.Output
        $pwrs.Offered | Should -Be @('ada')
        $pwrs.Offered | Should -Be $compiled.Offered
    }

    # Windows PowerShell's console host prompts for a credential in a
    # dialog, -NonInteractive or not, so only PowerShell 7's refuses.
    It 'refuses a user name in a host that cannot prompt for the password' -Skip:($PSVersionTable.PSEdition -eq 'Desktop') {
        { Test-RustCredential -Credential ada } | Should -Throw
    }
}

Describe 'the prompts -Debug makes the engine show' {
    # The built-in Write-* cmdlets are compiled cmdlets, so the records
    # they write and whether they prompt, under the same switches and
    # preferences, are what a PWRS cmdlet must match. Every prompt is
    # answered Yes to All, so each command runs through.
    It 'writes the records a compiled cmdlet writes under <Switches> <Preferences>' -ForEach @(
        @{ Switches = '-Debug'; Preferences = '' }
        @{ Switches = '-Debug -Verbose:$false'; Preferences = '' }
        @{ Switches = '-Debug:$false -Verbose'; Preferences = '' }
        @{ Switches = '-Debug -WarningAction SilentlyContinue'; Preferences = '' }
        @{ Switches = '-Verbose'; Preferences = '$WarningPreference = ''Ignore''' }
        @{ Switches = '-Debug'; Preferences = '$WarningPreference = ''Ignore''' }
    ) {
        $yes = @('Yes to All') * 8
        $pwrs = Answered $yes "$Preferences; Write-RustStreams -Message m $Switches"
        $compiled = Answered $yes "$Preferences; & { Write-Verbose 'verbose: m' $Switches; Write-Debug 'debug: m' $Switches; Write-Warning 'warning: m' $Switches; Write-Information 'information: m' $Switches }"
        $compiled.Ended | Should -BeNullOrEmpty
        $pwrs.Ended | Should -BeNullOrEmpty
        $pwrs.Seen | Should -Be $compiled.Seen
        $pwrs.Offered.Count -gt 0 | Should -Be ($compiled.Offered.Count -gt 0)
    }

    It 'stops where a question is answered Halt, as a compiled cmdlet stops' {
        $compiled = Answered 'Halt Command' "& { Write-Output 1; Write-Warning 'w' -Debug; Write-Output 2 }"
        $pwrs = Answered 'Halt Command' 'Invoke-RustWorker -Debug'
        $compiled.Ended | Should -Not -BeNullOrEmpty
        $pwrs.Ended | Should -Be $compiled.Ended
        $compiled.Seen | Should -Be @('Int32', 'WarningRecord')
        $pwrs.Seen | Should -Be @('Int64', 'WarningRecord')
    }
}

Describe 'every confirmation form' {
    BeforeAll {
        # Runs $command in a scripted host's runspace, answering each
        # prompt with the next of $typed; gives the output as text, the
        # number of prompts shown and the answers left.
        function script:Confirmed([string[]] $typed, [string] $command) {
            $fake = New-Object Pwrs.Tests.ScriptedHost
            foreach ($one in $typed) { $fake.Scripted.Answers.Enqueue($one) }
            $runspace = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspace($fake)
            $runspace.Open()
            $shell = $null
            try {
                $shell = [System.Management.Automation.PowerShell]::Create()
                $shell.Runspace = $runspace
                $null = $shell.AddScript("Import-Module '$script:manifest' -ErrorAction Stop; try { $command } finally { Remove-Module Hello }")
                $output = @($shell.Invoke() | ForEach-Object { "$_" })
                if ($shell.Streams.Error.Count -gt 0) { throw $shell.Streams.Error[0].Exception }
                $asked = @($fake.Scripted.Offered | Where-Object { $_ -match '^&?Yes$|^&Yes$' }).Count
                [pscustomobject]@{ Output = $output; Asked = $asked; Left = $fake.Scripted.Answers.Count }
            } finally {
                if ($shell) { $shell.Dispose() }
                $runspace.Close()
            }
        }
    }

    It 'returns true from each ShouldProcess form without a prompt, and false under -WhatIf with the reason' {
        Confirm-RustThing a -Form Target | Should -Be 'a true'
        Confirm-RustThing a -Form Described | Should -Be 'a true'
        Confirm-RustThing a -Form Reason | Should -Be 'a true None'
        Confirm-RustThing a -Form Target -WhatIf | Should -Be 'a false'
        Confirm-RustThing a -Form Described -WhatIf | Should -Be 'a false'
        Confirm-RustThing a -Form Reason -WhatIf | Should -Be 'a false WhatIf'
    }

    It 'prompts for each ShouldProcess form under -Confirm and returns the answer typed' {
        (Confirmed @('Y') "Confirm-RustThing a -Form Target -Confirm").Output | Should -Be 'a true'
        (Confirmed @('N') "Confirm-RustThing a -Form Described -Confirm").Output | Should -Be 'a false'
        (Confirmed @('Y') "Confirm-RustThing a -Form Reason -Confirm").Output | Should -Be 'a true None'
    }

    It 'applies Yes to All and No to All to the rest of the invocation''s records' {
        $all = Confirmed @('Yes to All') "'a', 'b', 'c' | Confirm-RustThing -Form ContinueAll"
        $all.Output | Should -Be @('a true', 'b true', 'c true')
        $all.Asked | Should -Be 1
        $none = Confirmed @('No to All') "'a', 'b', 'c' | Confirm-RustThing -Form ContinueAll"
        $none.Output | Should -Be @('a false', 'b false', 'c false')
        $none.Asked | Should -Be 1
        $each = Confirmed @('Y', 'N') "'a', 'b' | Confirm-RustThing -Form ContinueAll"
        $each.Output | Should -Be @('a true', 'b false')
        $each.Asked | Should -Be 2
    }

    It 'takes No as the default of a question with a security impact' {
        $default = Confirmed @('', 'Yes to All') "'a', 'b', 'c' | Confirm-RustThing -Form ContinueAllImpact"
        $default.Output | Should -Be @('a false', 'b true', 'c true')
        $default.Asked | Should -Be 2
    }
}
