# Checks that the tutorial's pages print what they say they print.
#
# Every chapter under wiki/content/docs/tutorials shows, after its
# lesson, the whole of each file the FileFacts module has at that point,
# each fenced block preceded by `<!-- file: <path in the crate> -->`. A
# chapter that does not show a file uses the one the chapter before it
# showed. For each chapter in the order of their weights, the gate lays
# the crate out under target/tutorial-gate with the pwrs dependency
# pointed at this checkout, builds it with this checkout's cargo-pwrs, and
# runs the chapter's PowerShell examples against the module it built,
# comparing what each prints with the text block under it. On Windows the
# examples run in pwsh and in Windows PowerShell, elsewhere in pwsh. A
# chapter whose files include one under tests/ is built with `cargo pwrs
# test` instead: its Rust tests and Pester suites must pass in every host,
# and the surface check must report nothing. The last chapter's files
# must match examples/file-facts, the finished module, and every
# chapter's pwrs dependency must name the workspace's version.
#
# Examples run one console line at a time, as they would be typed: each
# line is made into a script block and run at script scope, so an error
# shows the command and its message as it does at the prompt, the
# statements of one line run together, and a line that fails does not
# end the page. Each page runs in a process of its own per host, in an
# empty folder of its own, with the module imported. Output is formatted
# at 120 columns, in plain text, without progress bars, as it arrives,
# so it falls among the warning, verbose and host lines as it does in a
# console.
#
# Page conventions:
#   ```powershell     an example; the ```text block after it, with only
#                     blank lines between, is its output. An example
#                     without one must print nothing.
#   <!-- skip: why -->
#                     on the line before an example: not run, and counted.
#   <!-- varies: what -->
#                     on the line before an example: its numbers differ
#                     from run to run, so every run of digits compares as
#                     one digit.
#   <!-- powershell -->
#                     between an example's text block and a second one:
#                     the second is Windows PowerShell 5.1's output, for an
#                     example that prints differently there.
#   ![what it shows](/images/<name>.png)
#                     right after an example's text block: a picture of
#                     the console once the example has run, which -Capture
#                     takes; right after Windows PowerShell 5.1's text
#                     block, or right after <!-- powershell --> when 5.1
#                     prints the same text, a picture of Windows
#                     PowerShell's.
#   <!-- during: <ms> -->
#                     on the line before a picture: take it that many
#                     milliseconds after the example is typed, while it
#                     runs, for what shows only then, such as a progress
#                     bar.
#   ```ps1, or a block in any other language
#                     shown and not run, such as the lines of a test file.
#
#   pwsh -File tools/tutorial_gate.ps1 [-Target <triple>] [-Capture -Monitor <n> [-Probe]]
#   pwsh -File tools/tutorial_gate.ps1 -Module <psd1> -Page <md> [-Fill] [-Capture -Monitor <n> [-Probe] [-CaptureFolder <dir>]]
#
# The second form runs one page against a module already built, such as
# a how-to page against the hello example's, and, with -Fill, writes the
# first host's output into the page's text blocks.
#
# -Capture also takes a page's pictures, on Windows, once its text checks
# out: for each host with a picture to take, one console session on
# monitor <n> (tools/console_capture.ps1), in an empty -CaptureFolder
# (C:\Temp\pwrs-tutorial\file-facts, where the tutorial's chapter 1
# builds the module, unless named) with the page's module imported, runs
# every example in order as typed at the prompt. An example with a
# picture is typed on a cleared screen and captured from its prompt down
# to the next, into wiki/static/images. -Probe runs the sessions on a
# desktop nothing displays and takes no pictures.
[CmdletBinding()]
param(
    # A triple for `cargo pwrs build --target`, such as
    # x86_64-unknown-linux-gnu.2.35 for a pwsh on an older glibc than
    # the building machine's.
    [string] $Target = '',
    [string] $Module = '',
    [string] $Page = '',
    [switch] $Fill,
    [int] $Deadline = 300,
    [switch] $Capture,
    # The number Windows gives the monitor, as in \\.\DISPLAY2.
    [int] $Monitor = 0,
    [switch] $Probe,
    # The folder the console sessions run in, which their prompt shows;
    # C:\Temp\pwrs-tutorial\file-facts when not given.
    [string] $CaptureFolder = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$onWindows = [System.Environment]::OSVersion.Platform -eq 'Win32NT'
$hosts = if ($onWindows) { @('pwsh', 'powershell') } else { @('pwsh') }
if ($Capture) {
    if (-not $onWindows) { throw '-Capture takes pictures of Windows consoles and runs on Windows' }
    if ($Monitor -lt 1) { throw '-Capture needs -Monitor, the number of the monitor the consoles open on' }
    . (Join-Path $PSScriptRoot 'console_capture.ps1')
}
$imageLine = '^!\[[^\]]*\]\(/images/([a-z0-9-]+)\.png\)\s*$'

# The picture line at line At or after it, past blank lines and the
# <!-- during: <ms> --> line that may come first: its name, the
# milliseconds into the run to take it at (0 for after the run), and the
# index of its line. Nothing when no picture comes next.
function Read-Picture([string[]] $Lines, [int] $At) {
    $n = $At
    while ($n -lt $Lines.Count -and $Lines[$n].Trim() -eq '') { $n++ }
    $during = 0
    if ($n -lt $Lines.Count -and $Lines[$n] -match '^<!-- during: (\d+) -->$') {
        $during = [int]$Matches[1]
        $n++
        while ($n -lt $Lines.Count -and $Lines[$n].Trim() -eq '') { $n++ }
    }
    if ($n -lt $Lines.Count -and $Lines[$n] -match $imageLine) { [pscustomobject]@{ Name = $Matches[1]; During = $during; End = $n } }
}

function Read-Page([string] $Path) {
    $lines = [System.IO.File]::ReadAllLines($Path)
    $examples = [System.Collections.Generic.List[object]]::new()
    $files = [ordered]@{}
    $weight = [int]::MaxValue
    $i = 0
    while ($i -lt $lines.Count) {
        $line = $lines[$i]
        if ($i -lt 10 -and $line -match '^weight:\s*(\d+)\s*$') { $weight = [int]$Matches[1] }
        if ($line -match '^<!-- file: (\S+) -->$') {
            $name = $Matches[1]
            $open = $i + 1
            if ($open -ge $lines.Count -or $lines[$open] -notmatch '^```') { throw "$Path line $($i + 1): a file marker is not followed by a fenced block" }
            $close = $open + 1
            while ($close -lt $lines.Count -and $lines[$close] -notmatch '^```\s*$') { $close++ }
            if ($close -ge $lines.Count) { throw "$Path line $($open + 1): the block for $name never closes" }
            $files[$name] = if ($close -gt $open + 1) { ($lines[($open + 1)..($close - 1)] -join "`n") + "`n" } else { '' }
            $i = $close + 1
            continue
        }
        if ($line -match '^```powershell\s*$') {
            $skip = $null
            $varies = $false
            $k = $i - 1
            while ($k -ge 0 -and ($lines[$k].Trim() -eq '' -or $lines[$k] -match '^<!-- (skip|varies): .* -->$')) {
                if ($lines[$k] -match '^<!-- skip: (.*) -->$') { $skip = $Matches[1] }
                if ($lines[$k] -match '^<!-- varies: .* -->$') { $varies = $true }
                if ($lines[$k].Trim() -eq '' -and $k -lt $i - 1) { break }
                $k--
            }
            $start = $i + 1
            $j = $start
            while ($j -lt $lines.Count -and $lines[$j] -notmatch '^```\s*$') { $j++ }
            if ($j -ge $lines.Count) { throw "$Path line $($i + 1): an example's block never closes" }
            $code = if ($j -gt $start) { $lines[$start..($j - 1)] -join "`n" } else { '' }
            $outStart = -1
            $outEnd = -1
            $n = $j + 1
            while ($n -lt $lines.Count -and $lines[$n].Trim() -eq '') { $n++ }
            if ($n -lt $lines.Count -and $lines[$n] -match '^```text\s*$') {
                $m = $n + 1
                while ($m -lt $lines.Count -and $lines[$m] -notmatch '^```\s*$') { $m++ }
                if ($m -ge $lines.Count) { throw "$Path line $($n + 1): an output block never closes" }
                $outStart = $n + 1
                $outEnd = $m - 1
                $j = $m
            }
            $shown = $null
            if ($outStart -ge 0) { $shown = if ($outEnd -ge $outStart) { $lines[$outStart..$outEnd] -join "`n" } else { '' } }
            $image = $null
            if ($outStart -ge 0) {
                $image = Read-Picture $lines ($j + 1)
                if ($image) { $j = $image.End }
            }
            $desktopStart = -1
            $desktopEnd = -1
            $desktop = $null
            $desktopImage = $null
            if ($outStart -ge 0) {
                $n = $j + 1
                while ($n -lt $lines.Count -and $lines[$n] -ne '<!-- powershell -->' -and $lines[$n] -notmatch '^(```|#)') { $n++ }
                if ($n -lt $lines.Count -and $lines[$n] -eq '<!-- powershell -->') {
                    $n++
                    while ($n -lt $lines.Count -and $lines[$n].Trim() -eq '') { $n++ }
                    $desktopImage = Read-Picture $lines $n
                    if ($desktopImage) {
                        $j = $desktopImage.End
                    } else {
                        if ($n -ge $lines.Count -or $lines[$n] -notmatch '^```text\s*$') { throw "$Path line $($n + 1): <!-- powershell --> is followed by neither a text block nor a picture" }
                        $m = $n + 1
                        while ($m -lt $lines.Count -and $lines[$m] -notmatch '^```\s*$') { $m++ }
                        $desktopStart = $n + 1
                        $desktopEnd = $m - 1
                        $desktop = if ($desktopEnd -ge $desktopStart) { $lines[$desktopStart..$desktopEnd] -join "`n" } else { '' }
                        $j = $m
                        $desktopImage = Read-Picture $lines ($j + 1)
                        if ($desktopImage) { $j = $desktopImage.End }
                    }
                }
            }
            $examples.Add([pscustomobject]@{
                Index = $examples.Count + 1; Line = $i + 1; Code = $code; Skip = $skip; Varies = $varies
                OutStart = $outStart; OutEnd = $outEnd; Shown = $shown; Image = $image
                DesktopStart = $desktopStart; DesktopEnd = $desktopEnd; Desktop = $desktop; DesktopImage = $desktopImage
            })
            $i = $j
        }
        $i++
    }
    [pscustomobject]@{ Path = $Path; Lines = $lines; Examples = $examples; Files = $files; Weight = $weight }
}

# The console lines of an example: its statements, those that share a
# line joined into one.
function Split-Lines([string] $Code) {
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput($Code, [ref]$tokens, [ref]$errors)
    if ($errors.Count) { throw "an example does not parse: $($errors[0].Message) at line $($errors[0].Extent.StartLineNumber)" }
    $units = [System.Collections.Generic.List[object]]::new()
    foreach ($s in $ast.EndBlock.Statements) {
        $last = if ($units.Count) { $units[$units.Count - 1] } else { $null }
        if ($last -and $s.Extent.StartLineNumber -eq $last.EndLine) {
            $last.End = $s.Extent.EndOffset
            $last.EndLine = $s.Extent.EndLineNumber
        } else {
            $units.Add([pscustomobject]@{ Start = $s.Extent.StartOffset; End = $s.Extent.EndOffset; EndLine = $s.Extent.EndLineNumber })
        }
    }
    foreach ($u in $units) { $Code.Substring($u.Start, $u.End - $u.Start) }
}

function Build-Script($PageInfo, [string] $Folder, [string] $Psd1) {
    $q = { param($s) "'" + $s.Replace("'", "''") + "'" }
    $sb = [System.Text.StringBuilder]::new()
    [void]$sb.AppendLine('[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)')
    [void]$sb.AppendLine('$ErrorActionPreference = ''Continue''')
    [void]$sb.AppendLine('$ProgressPreference = ''SilentlyContinue''')
    [void]$sb.AppendLine('if ($PSVersionTable.PSVersion.Major -ge 7) { $PSStyle.OutputRendering = ''PlainText''; $ErrorView = ''ConciseView'' }')
    [void]$sb.AppendLine("Set-Location -LiteralPath $(& $q $Folder)")
    [void]$sb.AppendLine("Import-Module $(& $q $Psd1)")
    # One formatter for a whole line, fed as each object arrives, so output
    # is written when the console would write it, among the warning,
    # verbose and host lines written in the meantime.
    [void]$sb.AppendLine('function __Emit { begin { $p = { Out-String -Width 120 -Stream | ForEach-Object { [Console]::Out.WriteLine($_.TrimEnd()) } }.GetSteppablePipeline(); $p.Begin($true) } process { $p.Process($_) } end { $p.End() } }')
    foreach ($ex in $PageInfo.Examples) {
        [void]$sb.AppendLine("[Console]::Out.WriteLine('@@EX $($ex.Index)@@')")
        if ($ex.Skip) { continue }
        foreach ($unit in (Split-Lines $ex.Code)) {
            [void]$sb.AppendLine("try { . ([scriptblock]::Create($(& $q $unit))) 2>&1 | __Emit } catch { `$_ | __Emit }")
        }
    }
    [void]$sb.AppendLine("[Console]::Out.WriteLine('@@END@@')")
    $sb.ToString()
}

function Invoke-Page($PageInfo, [string] $HostName, [string] $Psd1) {
    $tag = [guid]::NewGuid().ToString('N').Substring(0, 8)
    $folder = Join-Path ([System.IO.Path]::GetTempPath()) "pwrs-tutorial-$tag"
    $null = New-Item -ItemType Directory -Path $folder
    $script = Join-Path ([System.IO.Path]::GetTempPath()) "pwrs-tutorial-$tag.ps1"
    [System.IO.File]::WriteAllText($script, (Build-Script $PageInfo $folder $Psd1), [System.Text.UTF8Encoding]::new($true))
    $exe = (Get-Command $HostName).Source
    $psi = [System.Diagnostics.ProcessStartInfo]::new($exe)
    foreach ($a in @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $script)) { $psi.ArgumentList.Add($a) }
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false)
    $psi.StandardErrorEncoding = [System.Text.UTF8Encoding]::new($false)
    $psi.WorkingDirectory = $folder
    $p = [System.Diagnostics.Process]::Start($psi)
    $outTask = $p.StandardOutput.ReadToEndAsync()
    $errTask = $p.StandardError.ReadToEndAsync()
    $timedOut = -not $p.WaitForExit($Deadline * 1000)
    if ($timedOut) { $p.Kill($true); $p.WaitForExit() }
    $out = $outTask.Result
    $err = $errTask.Result
    Remove-Item -LiteralPath $script
    Remove-Item -LiteralPath $folder -Recurse -Force
    $byExample = @{}
    $current = $null
    $ended = $false
    foreach ($l in ($out -split "`r?`n")) {
        if ($l -match '^@@EX (\d+)@@$') { $current = [int]$Matches[1]; $byExample[$current] = [System.Collections.Generic.List[string]]::new(); continue }
        if ($l -eq '@@END@@') { $ended = $true; $current = $null; continue }
        if ($null -ne $current) { $byExample[$current].Add($l) }
    }
    [pscustomobject]@{ Host = $HostName; ByExample = $byExample; TimedOut = $timedOut; Ended = $ended; Last = $current; Stderr = $err.Trim(); Exit = $p.ExitCode }
}

# Runs a command in Directory and returns its exit code and every line it
# wrote, its output and then its errors, read as plain text: a pwsh that
# captures a native command's errors itself decodes the CLIXML Windows
# PowerShell writes its progress in, fails, and replaces what follows.
function Invoke-Raw([string] $File, [string[]] $Arguments, [string] $Directory) {
    $psi = [System.Diagnostics.ProcessStartInfo]::new($File)
    foreach ($a in $Arguments) { $psi.ArgumentList.Add($a) }
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.WorkingDirectory = $Directory
    $p = [System.Diagnostics.Process]::Start($psi)
    $outTask = $p.StandardOutput.ReadToEndAsync()
    $errTask = $p.StandardError.ReadToEndAsync()
    $p.WaitForExit()
    [pscustomobject]@{ Code = $p.ExitCode; Lines = @(($outTask.Result.TrimEnd() + "`n" + $errTask.Result.TrimEnd()) -split "`r?`n") }
}

function Format-Output($Lines) {
    if ($null -eq $Lines) { return '' }
    $list = @($Lines | ForEach-Object { $_.TrimEnd() })
    $first = 0
    while ($first -lt $list.Count -and $list[$first] -eq '') { $first++ }
    $last = $list.Count - 1
    while ($last -ge $first -and $list[$last] -eq '') { $last-- }
    if ($last -lt $first) { return '' }
    ($list[$first..$last] -join "`n")
}

# Text with each run of spaces and of dashes read as one, for a table
# whose columns are as wide as another host makes them.
function Squeeze([string] $Text) {
    (($Text -split "`n") | ForEach-Object { ($_ -replace '-{2,}', '-' -replace '\s+', ' ').Trim() } | Where-Object { $_ -ne '' }) -join "`n"
}

# Windows PowerShell 5.1 names the unsigned integer types uint32 and uint64
# where PowerShell 7 writes uint and ulong.
function Format-TypeSpelling([string] $Text) {
    $Text -creplace '\buint64\b', 'ulong' -creplace '\buint32\b', 'uint'
}

function Format-Varying([string] $Text) {
    ($Text -creplace '[0-9a-f]{8,}', 'X') -replace '[0-9]+(,[0-9]{3})*(\.[0-9]+)?', '9'
}

# Runs one page against one built module in every host. Returns the number
# of hosts whose output differs from the page; with -Fill, writes the
# first host's output into the page instead.
function Test-Page($PageInfo, [string] $Psd1, [string] $Label) {
    $failed = 0
    $runs = foreach ($h in $hosts) { Invoke-Page $PageInfo $h $Psd1 }
    foreach ($run in $runs) {
        $problems = [System.Collections.Generic.List[string]]::new()
        if ($run.TimedOut) { $problems.Add("killed at the $Deadline s deadline while running example $($run.Last)") }
        elseif (-not $run.Ended) { $problems.Add("the process ended (exit $($run.Exit)) during example $($run.Last)") }
        if ($run.Stderr) { $problems.Add("stderr: $($run.Stderr)") }
        $widthOnly = 0
        foreach ($ex in $PageInfo.Examples) {
            if ($ex.Skip -or $Fill) { continue }
            $got = Format-Output $run.ByExample[$ex.Index]
            $want = if ($run.Host -eq 'powershell' -and $null -ne $ex.Desktop) { Format-Output ($ex.Desktop -split "`n") }
                    elseif ($null -eq $ex.Shown) { '' }
                    else { Format-Output ($ex.Shown -split "`n") }
            if ($got -ceq $want) { continue }
            if ($run.Host -eq 'powershell' -and $null -eq $ex.Desktop) {
                $got = Format-TypeSpelling $got
                $want = Format-TypeSpelling $want
                if ($got -ceq $want) { continue }
            }
            if ($ex.Varies) {
                $got = Format-Varying $got
                $want = Format-Varying $want
                if ($got -ceq $want -or (Squeeze $got) -ceq (Squeeze $want)) { continue }
            }
            if ((Squeeze $got) -ceq (Squeeze $want)) { $widthOnly++; continue }
            $problems.Add("example $($ex.Index) (line $($ex.Line)) differs`n--- page`n$want`n--- $($run.Host)`n$got")
        }
        $skipped = @($PageInfo.Examples | Where-Object Skip).Count
        $verdict = if ($problems.Count) { 'DIFFERS' } elseif ($Fill) { 'RAN' } elseif ($widthOnly) { "same apart from column widths in $widthOnly" } else { 'same' }
        Write-Host ("PAGE {0} host={1} examples={2} skipped={3}: {4}" -f $Label, $run.Host, $PageInfo.Examples.Count, $skipped, $verdict)
        foreach ($p in $problems) { Write-Host "  $p" }
        if ($problems.Count) { $failed++ }
    }
    if ($Fill) {
        $first = $runs[0]
        $desktopRun = $runs | Where-Object Host -eq 'powershell' | Select-Object -First 1
        $lines = [System.Collections.Generic.List[string]]::new([string[]]$PageInfo.Lines)
        foreach ($ex in ($PageInfo.Examples | Sort-Object Line -Descending)) {
            if ($ex.Skip) { continue }
            if ($ex.DesktopStart -ge 0 -and $desktopRun) {
                $theirs = Format-Output $desktopRun.ByExample[$ex.Index]
                $count = $ex.DesktopEnd - $ex.DesktopStart + 1
                if ($count -gt 0) { $lines.RemoveRange($ex.DesktopStart, $count) }
                if ($theirs) { $lines.InsertRange($ex.DesktopStart, [string[]]($theirs -split "`n")) }
            }
            $got = Format-Output $first.ByExample[$ex.Index]
            if ($ex.OutStart -lt 0) {
                if ($got) { Write-Host "  example $($ex.Index) (line $($ex.Line)) printed output and the page has no text block for it:`n$got"; $failed++ }
                continue
            }
            if (-not $got) { Write-Host "  example $($ex.Index) (line $($ex.Line)) printed nothing and the page has a text block for it"; $failed++ }
            $count = $ex.OutEnd - $ex.OutStart + 1
            if ($count -gt 0) { $lines.RemoveRange($ex.OutStart, $count) }
            if ($got) { $lines.InsertRange($ex.OutStart, [string[]]($got -split "`n")) }
        }
        [System.IO.File]::WriteAllText($PageInfo.Path, (($lines -join "`n") + "`n"), [System.Text.UTF8Encoding]::new($false))
    }
    $failed
}

# An example as typed at the prompt: each console line ends with Enter,
# and the lines of a statement that spans several are joined with
# Shift+Enter, so the statement is one input as it is when pasted.
function Get-TypedText([string] $Code) {
    if ($Code.Contains("`t")) { throw 'an example contains a tab, which the console takes as a request to complete' }
    (@(Split-Lines $Code) | ForEach-Object { $_ + "`r" }) -join ''
}

# The folder the console sessions run in, -CaptureFolder, made empty. A
# folder this gate made for an earlier session, which has a hidden
# .pwrs-capture, is moved into the temp folder under a name of its own;
# any other stops the run.
function Initialize-CaptureFolder {
    $folder = if ($CaptureFolder) { $CaptureFolder } else { Join-Path $env:SystemDrive 'Temp\pwrs-tutorial\file-facts' }
    if (Test-Path -LiteralPath $folder) {
        if (-not (Test-Path -LiteralPath (Join-Path $folder '.pwrs-capture'))) { throw "$folder was not made by this gate; move it elsewhere to take pictures" }
        Move-Item -LiteralPath $folder -Destination (Join-Path ([System.IO.Path]::GetTempPath()) "pwrs-tutorial-capture-$([guid]::NewGuid().ToString('N'))")
    }
    $null = New-Item -ItemType Directory -Force -Path $folder
    $mark = New-Item -ItemType File -Path (Join-Path $folder '.pwrs-capture')
    $mark.Attributes = $mark.Attributes -bor [System.IO.FileAttributes]::Hidden
    $folder
}

# Takes a page's pictures, one console session for each host that has a
# picture to take, and returns how many hosts' sessions failed.
function Invoke-Capture($PageInfo, [string] $Psd1, [string] $Label) {
    $failed = 0
    $images = Join-Path $root 'wiki/static/images'
    foreach ($h in $hosts) {
        $mine = @($PageInfo.Examples | Where-Object { if ($h -eq 'pwsh') { $_.Image } else { $_.DesktopImage } })
        if (-not $mine.Count) { continue }
        $null = New-Item -ItemType Directory -Force -Path $images
        $folder = Initialize-CaptureFolder
        $setup = Join-Path ([System.IO.Path]::GetTempPath()) "pwrs-tutorial-setup-$([guid]::NewGuid().ToString('N')).ps1"
        $q = { param($s) "'" + $s.Replace("'", "''") + "'" }
        [System.IO.File]::WriteAllText($setup, "Set-Location -LiteralPath $(& $q $folder)`nImport-Module $(& $q $Psd1)`n", [System.Text.UTF8Encoding]::new($true))
        $session = $null
        try {
            $session = Start-ConsoleCapture -Monitor $Monitor -Setup $setup -Shell $h -Probe:$Probe
            Write-Host "IMAGES $Label host=${h}: $($session.Host)"
            foreach ($ex in $PageInfo.Examples) {
                if ($ex.Skip) { continue }
                $picture = if ($h -eq 'pwsh') { $ex.Image } else { $ex.DesktopImage }
                $typed = Get-TypedText $ex.Code
                if (-not $picture) { $null = Invoke-ConsoleStep $session $typed; continue }
                $null = Invoke-ConsoleStep $session "Clear-Host`r"
                $taken = Invoke-ConsoleStep $session $typed -Image (Join-Path $images "$($picture.Name).png") -During $picture.During
                Write-Host "IMAGES $Label host=$h example $($ex.Index) (line $($ex.Line)): $(if ($Probe) { "$($picture.Name) ran and fits" } else { $taken })"
            }
        } catch {
            Write-Host "IMAGES $Label host=${h}: $($_.Exception.Message)"
            $failed++
        } finally {
            if ($session) {
                $state = Stop-ConsoleCapture $session
                Write-Host "IMAGES $Label host=${h}: $state"
                if ($state -like 'LEFT OPEN*') { $failed++ }
            }
        }
    }
    $failed
}

if ($Module -or $Page) {
    if (-not ($Module -and $Page)) { throw '-Module and -Page go together' }
    $info = Read-Page (Resolve-Path -LiteralPath $Page).Path
    $psd1 = (Resolve-Path -LiteralPath $Module).Path
    $failed = Test-Page $info $psd1 (Split-Path -Leaf $Page)
    if ($Capture) {
        if ($failed) { Write-Host "IMAGES $(Split-Path -Leaf $Page): not taken, as the page's text differs" }
        else { $failed += Invoke-Capture $info $psd1 (Split-Path -Leaf $Page) }
    }
    "tutorial-verdict $(if ($failed) { "fail ($failed)" } else { 'pass' })"
    exit $(if ($failed) { 1 } else { 0 })
}

# The gate: every chapter built from its page and run.
$version = (Select-String -LiteralPath (Join-Path $root 'Cargo.toml') -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
$pwrsPath = (Join-Path $root 'crates/pwrs') -replace '\\', '/'
$gate = Join-Path $root 'target/tutorial-gate'
$crate = Join-Path $gate 'file-facts'
$env:CARGO_TARGET_DIR = Join-Path $gate 'target'
$psd1 = Join-Path $gate 'target/pwrs/FileFacts/FileFacts.psd1'
$chapters = @(Get-ChildItem (Join-Path $root 'wiki/content/docs/tutorials') -Filter *.md | Where-Object Name -ne '_index.md' | ForEach-Object { Read-Page $_.FullName } | Where-Object { $_.Files.Count } | Sort-Object Weight)
if (-not $chapters.Count) { throw 'no tutorial chapter shows its files' }
"tutorial gate on $($PSVersionTable.PSEdition) $($PSVersionTable.PSVersion): $($chapters.Count) chapters, pwrs $version"
if (Test-Path $crate) { Remove-Item $crate -Recurse -Force }
$failures = 0
$current = [ordered]@{}
foreach ($chapter in $chapters) {
    $label = Split-Path -Leaf $chapter.Path
    foreach ($name in $chapter.Files.Keys) { $current[$name] = $chapter.Files[$name] }
    if (-not $current.Contains('Cargo.toml')) { throw "$label comes before any chapter shows Cargo.toml" }
    $manifest = $current['Cargo.toml']
    $dependency = [regex]::Match($manifest, '(?m)^pwrs = \{ package = "PoWerRuSt", version = "([^"]+)" \}\r?$')
    if (-not $dependency.Success) { throw "${label}: Cargo.toml has no pwrs dependency line of the form cargo pwrs new writes" }
    if ($dependency.Groups[1].Value -ne $version) {
        "PAGE $label names pwrs $($dependency.Groups[1].Value) where the workspace is $version"
        $failures++
    }
    $local = "[workspace]`n`n" + $manifest.Replace($dependency.Value, "pwrs = { package = `"PoWerRuSt`", path = `"$pwrsPath`" }")
    foreach ($name in $current.Keys) {
        $path = Join-Path $crate $name
        $null = New-Item -ItemType Directory -Force -Path (Split-Path $path)
        $text = if ($name -eq 'Cargo.toml') { $local } else { $current[$name] }
        [System.IO.File]::WriteAllText($path, $text, [System.Text.UTF8Encoding]::new($false))
    }
    # A chapter whose module has a suite is tested as well as built.
    [string[]] $targetArgs = @()
    if ($Target) { $targetArgs = @('--target', $Target) }
    $tested = @($current.Keys | Where-Object { $_ -like 'tests/*' }).Count -gt 0
    $verb = if ($tested) { 'test' } else { 'build' }
    $run = Invoke-Raw (Get-Command cargo).Source (@('run', '--profile', 'test-fast', '-p', 'cargo-pwrs', '--', 'pwrs', $verb) + $targetArgs + @('--manifest-dir', $crate)) $root
    if ($run.Code -ne 0) {
        $run.Lines
        "PAGE $label $verb failed with $($run.Code)"
        $failures++
        continue
    }
    if ($tested) {
        $run.Lines | Where-Object { $_ -match '^pwrs pester ' } | ForEach-Object { "PAGE $label $_" }
        $clean = 'pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess cmdlet called should_process'
        $surface = @($run.Lines | Where-Object { $_ -like 'pwrs: surface check:*' })
        if ($surface.Count -ne 1 -or $surface[0] -ne $clean) {
            "PAGE $label surface check: $(if ($surface.Count) { $surface -join ' | ' } else { 'no surface check line' })"
            $run.Lines | Where-Object { $_ -match '^  \S' } | ForEach-Object { "PAGE $label $_" }
            $failures++
        }
    }
    $differs = Test-Page $chapter $psd1 $label
    $failures += $differs
    if ($Capture) {
        if ($differs) { Write-Host "IMAGES ${label}: not taken, as the page's text differs" }
        else { $failures += Invoke-Capture $chapter $psd1 $label }
    }
}
$finished = Join-Path $root 'examples/file-facts'
foreach ($name in $current.Keys) {
    $mine = Join-Path $finished $name
    if ($name -eq 'Cargo.toml') { continue }
    if (-not (Test-Path -LiteralPath $mine)) { "the last chapter shows $name, which examples/file-facts does not have"; $failures++; continue }
    $theirs = [System.IO.File]::ReadAllText($mine) -replace "`r`n", "`n"
    if ($theirs -cne $current[$name]) { "examples/file-facts/$name differs from the last chapter's"; $failures++ }
}
"tutorial-verdict $(if ($failures) { "fail ($failures)" } else { 'pass' })"
exit $(if ($failures) { 1 } else { 0 })
