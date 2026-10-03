# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# The host these tests run in is -NonInteractive and cannot answer a
# prompt, so each prompting case runs the cmdlet in a runspace whose
# host answers from a queue and keeps what was written to it. The
# cmdlet reaches that host through the same $Host.UI a console hands
# it, so what is exercised is the module's own path to the host. The
# host that cannot answer is its own case.
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

        public override PSCredential PromptForCredential(string caption, string message, string userName, string targetName)
        {
            throw new NotSupportedException();
        }

        public override PSCredential PromptForCredential(string caption, string message, string userName, string targetName, PSCredentialTypes allowedCredentialTypes, PSCredentialUIOptions options)
        {
            throw new NotSupportedException();
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

    It 'takes the default on an empty answer' {
        (Answer '' "Read-RustHost Choice -Choices '&Yes', '&No'").Output | Should -Be 0
    }

    It 'writes to the host before asking, off the pipeline' {
        $asked = Answer 'ok' "Read-RustHost Line -Say 'first, on the host'"
        $asked.Written | Should -Be @('first, on the host')
        $asked.Output | Should -Be @('ok')
    }

    It 'refuses in a host that cannot prompt, with the engine''s own error' {
        { Read-RustHost Line -ErrorAction Stop } | Should -Throw
    }
}
