# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Write-RustHost is held to Write-Host. Each case runs both commands with
# the same arguments in a runspace whose host records every write it is
# handed, with its colors, and both records must be the one written in the
# case. The cases' records are what Write-Host hands a host in PowerShell
# 7.6.6; the comparison with Write-Host itself in the same run is what
# holds on any other version.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $manifest = Join-Path $module 'Hello.psd1'
    Import-Module $manifest -Force -ErrorAction Stop
    $script:manifest = $manifest

    # Written for the C# the Windows PowerShell compiler accepts, so
    # no expression-bodied members.
    if (-not ('Pwrs.Tests.RecordingHost' -as [type])) {
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
    public sealed class FixedRawUi : PSHostRawUserInterface
    {
        public ConsoleColor Fg = ConsoleColor.Gray;
        public ConsoleColor Bg = ConsoleColor.DarkBlue;
        public override ConsoleColor ForegroundColor { get { return Fg; } set { Fg = value; } }
        public override ConsoleColor BackgroundColor { get { return Bg; } set { Bg = value; } }
        public override Coordinates CursorPosition { get { return new Coordinates(0, 0); } set { } }
        public override int CursorSize { get { return 1; } set { } }
        public override Size BufferSize { get { return new Size(80, 25); } set { } }
        public override Coordinates WindowPosition { get { return new Coordinates(0, 0); } set { } }
        public override Size WindowSize { get { return new Size(80, 25); } set { } }
        public override Size MaxWindowSize { get { return new Size(80, 25); } }
        public override Size MaxPhysicalWindowSize { get { return new Size(80, 25); } }
        public override bool KeyAvailable { get { return false; } }
        public override string WindowTitle { get { return ""; } set { } }
        public override void FlushInputBuffer() { }
        public override BufferCell[,] GetBufferContents(Rectangle rectangle) { throw new NotSupportedException(); }
        public override KeyInfo ReadKey(ReadKeyOptions options) { throw new NotSupportedException(); }
        public override void ScrollBufferContents(Rectangle source, Coordinates destination, Rectangle clip, BufferCell fill) { }
        public override void SetBufferContents(Coordinates origin, BufferCell[,] contents) { }
        public override void SetBufferContents(Rectangle rectangle, BufferCell fill) { }
    }

    public sealed class RecordingUi : PSHostUserInterface
    {
        public List<string> Calls = new List<string>();
        public FixedRawUi Raw;

        public override PSHostRawUserInterface RawUI { get { return Raw; } }

        public override void Write(string value) { Calls.Add("Write|" + value); }
        public override void Write(ConsoleColor foreground, ConsoleColor background, string value) { Calls.Add("Write|" + foreground + "|" + background + "|" + value); }
        public override void WriteLine() { Calls.Add("WriteLine|"); }
        public override void WriteLine(string value) { Calls.Add("WriteLine|" + value); }
        public override void WriteLine(ConsoleColor foreground, ConsoleColor background, string value) { Calls.Add("WriteLine|" + foreground + "|" + background + "|" + value); }
        public override void WriteErrorLine(string value) { Calls.Add("Error|" + value); }
        public override void WriteDebugLine(string message) { }
        public override void WriteProgress(long sourceId, ProgressRecord record) { }
        public override void WriteVerboseLine(string message) { }
        public override void WriteWarningLine(string message) { }
        public override string ReadLine() { throw new NotSupportedException(); }
        public override SecureString ReadLineAsSecureString() { throw new NotSupportedException(); }
        public override Dictionary<string, PSObject> Prompt(string caption, string message, Collection<FieldDescription> descriptions) { throw new NotSupportedException(); }
        public override PSCredential PromptForCredential(string caption, string message, string userName, string targetName) { throw new NotSupportedException(); }
        public override PSCredential PromptForCredential(string caption, string message, string userName, string targetName, PSCredentialTypes allowedCredentialTypes, PSCredentialUIOptions options) { throw new NotSupportedException(); }
        public override int PromptForChoice(string caption, string message, Collection<ChoiceDescription> choices, int defaultChoice) { throw new NotSupportedException(); }
    }

    public sealed class RecordingHost : PSHost
    {
        // Named for what it is rather than Ui: a public field differing
        // from the inherited UI property only in casing is not CLS
        // compliant, and the engine's type adapter refuses the type.
        public RecordingUi Recorded = new RecordingUi();
        private readonly Guid id = Guid.NewGuid();

        public RecordingHost(bool withRawUi)
        {
            if (withRawUi) { Recorded.Raw = new FixedRawUi(); }
        }

        public override CultureInfo CurrentCulture { get { return CultureInfo.InvariantCulture; } }
        public override CultureInfo CurrentUICulture { get { return CultureInfo.InvariantCulture; } }
        public override Guid InstanceId { get { return id; } }
        public override string Name { get { return "Recording"; } }
        public override PSHostUserInterface UI { get { return Recorded; } }
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
    # file defines outside BeforeAll is not in scope while the tests run.
    # Runs $command in a runspace of a RecordingHost, with the module
    # imported and removed again so its import and removal hooks stay
    # paired, and answers what the host was handed.
    function Record([string] $command, [bool] $withRawUi) {
        $fake = New-Object Pwrs.Tests.RecordingHost $withRawUi
        $runspace = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspace($fake)
        $runspace.Open()
        $shell = $null
        try {
            $shell = [System.Management.Automation.PowerShell]::Create()
            $shell.Runspace = $runspace
            $null = $shell.AddScript("Import-Module '$script:manifest' -ErrorAction Stop; try { $command } finally { Remove-Module Hello }")
            $null = $shell.Invoke()
            if ($shell.Streams.Error.Count -gt 0) { throw $shell.Streams.Error[0].Exception }
            , @($fake.Recorded.Calls)
        } finally {
            if ($shell) { $shell.Dispose() }
            $runspace.Close()
        }
    }

    # The lines a transcript recorded between its header and its footer,
    # each of which is framed by a row of 22 stars.
    function TranscriptBody([string] $path) {
        $all = @(Get-Content -LiteralPath $path)
        $stars = @(for ($i = 0; $i -lt $all.Count; $i++) { if ($all[$i] -match '^\*{22}$') { $i } })
        , @($all[($stars[1] + 1)..($stars[2] - 1)])
    }
}

Describe 'writing to the host as Write-Host does' {
    $cases = @(
        @{ Case = 'colors and a line end'; Arguments = 'hi -ForegroundColor Red -BackgroundColor Blue'; RawUi = $false; Preface = ''; Calls = @('WriteLine|Red|Blue|hi') }
        @{ Case = 'an open line, the background filled by the engine on a host with no RawUI'; Arguments = 'hi -ForegroundColor Red -NoNewline'; RawUi = $false; Preface = ''; Calls = @('Write|Red|Black|hi') }
        @{ Case = 'no colors on a host with no RawUI'; Arguments = 'hi'; RawUi = $false; Preface = ''; Calls = @('WriteLine|hi') }
        @{ Case = 'the host''s current colors when none are given'; Arguments = 'hi'; RawUi = $true; Preface = ''; Calls = @('WriteLine|Gray|DarkBlue|hi') }
        @{ Case = 'the host''s current background beside a given foreground'; Arguments = 'hi -ForegroundColor Red'; RawUi = $true; Preface = ''; Calls = @('WriteLine|Red|DarkBlue|hi') }
        @{ Case = 'an empty line'; Arguments = "''"; RawUi = $false; Preface = ''; Calls = @('WriteLine|') }
        @{ Case = 'nothing under 6>$null'; Arguments = 'hi 6>$null'; RawUi = $false; Preface = ''; Calls = @() }
        @{ Case = 'nothing under -InformationAction Ignore'; Arguments = 'hi -InformationAction Ignore'; RawUi = $false; Preface = ''; Calls = @() }
        @{ Case = 'shown under InformationPreference SilentlyContinue'; Arguments = 'hi'; RawUi = $false; Preface = '$InformationPreference = ''SilentlyContinue''; '; Calls = @('WriteLine|hi') }
    )

    It 'hands the host what Write-Host hands it: <Case>' -TestCases $cases {
        param($Case, $Arguments, $RawUi, $Preface, $Calls)
        $expected = $Calls -join ' ; '
        (Record "${Preface}Write-Host $Arguments" $RawUi) -join ' ; ' | Should -Be $expected
        (Record "${Preface}Write-RustHost $Arguments" $RawUi) -join ' ; ' | Should -Be $expected
    }
}

Describe 'the record Write-RustHost writes' {
    It 'is an information record tagged PSHOST carrying a HostInformationMessage' {
        Write-RustHost hi -ForegroundColor Red -NoNewline -InformationVariable held 6>$null
        $held.Count | Should -Be 1
        $held[0].Tags | Should -Contain 'PSHOST'
        $held[0].MessageData.GetType().FullName | Should -Be 'System.Management.Automation.HostInformationMessage'
        $held[0].MessageData.Message | Should -Be 'hi'
        $held[0].MessageData.ForegroundColor | Should -Be ([System.ConsoleColor]::Red)
        $held[0].MessageData.NoNewLine | Should -Be $true
    }

    It 'comes out of a 6>&1 redirection as an InformationRecord' {
        $records = @(Write-RustHost hi 6>&1)
        $records.Count | Should -Be 1
        $records[0] -is [System.Management.Automation.InformationRecord] | Should -Be $true
        "$($records[0].MessageData)" | Should -Be 'hi'
    }

    It 'writes nothing to the output stream' {
        @(Write-RustHost hi 6>$null).Count | Should -Be 0
    }

    It 'is refused a color the binder does not know, before the cmdlet runs' {
        { Write-RustHost hi -ForegroundColor Mauve -ErrorAction Stop 6>$null } | Should -Throw
    }
}

Describe 'a transcript of host writes' {
    It 'records each write as a line, as it records Write-Host' {
        $path = Join-Path $TestDrive 'host-writes.txt'
        $null = Start-Transcript -LiteralPath $path
        try {
            Write-Host 'open ' -NoNewline 6>$null
            Write-Host 'closed' 6>$null
            Write-RustHost 'open ' -NoNewline 6>$null
            Write-RustHost 'closed' 6>$null
        } finally {
            $null = Stop-Transcript
        }
        # Each host records each call as a line; PowerShell 7 trims the line's
        # trailing white space, and the write records what Write-Host does
        $lines = TranscriptBody $path
        $lines.Count | Should -Be 4
        ($lines[2..3] -join ' ; ') | Should -BeExactly ($lines[0..1] -join ' ; ')
        $lines[0].TrimEnd() | Should -Be 'open'
        $lines[1] | Should -Be 'closed'
    }
}
