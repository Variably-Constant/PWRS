# The PowerShell session tools/console_capture.ps1 starts in the console it
# captures. It writes <Marker>.host with its version, output rendering and
# NO_COLOR, and <Marker>.hwnd with its console window, dot-sources -Setup,
# and clears the screen. Each prompt it draws writes <Marker>.prompt.<n>:
# the cursor row, the window's top row and the window's height. While the
# prompt waits, it types the text of <Marker>.step.<k> into its own
# console input buffer, one step at a time and in order, where the prompt
# reads it as if typed (a carriage return as Enter, a line feed as
# Shift+Enter), and writes <Marker>.typed.<k>. It exits once
# <Marker>.done exists. PSReadLine saves no history from it and shows no
# predictions.
#
# A script run with -File and -NoExit leaves its variables in the session
# that goes on after it, so this one's own preparation runs in a scope of its own
# and the examples run with PowerShell's default preferences.
#
#   pwsh -NoLogo -NoProfile -NoExit -File console_session.ps1 -Marker <path prefix> -Setup <ps1>
param(
    [Parameter(Mandatory)] [string] $Marker,
    [Parameter(Mandatory)] [string] $Setup
)
& {
    $ErrorActionPreference = 'Stop'
    Add-Type -Namespace PwrsConsoleCapture -Name Session -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
[DllImport("kernel32.dll")] public static extern IntPtr GetStdHandle(int which);
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool WriteConsoleInputW(IntPtr h, KeyRecord[] records, int count, out int written);
[DllImport("user32.dll")] public static extern short VkKeyScanW(char c);
[DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint code, uint type);
// An INPUT_RECORD with a KEY_EVENT_RECORD in it.
[StructLayout(LayoutKind.Explicit, Size = 20)]
public struct KeyRecord {
    [FieldOffset(0)] public ushort EventType;
    [FieldOffset(4)] public int KeyDown;
    [FieldOffset(8)] public ushort RepeatCount;
    [FieldOffset(10)] public ushort VirtualKeyCode;
    [FieldOffset(12)] public ushort VirtualScanCode;
    [FieldOffset(14)] public ushort UnicodeChar;
    [FieldOffset(16)] public uint ControlKeyState;
}
// Each character becomes a press and a release of the key the keyboard
// layout gives it, with that key's shift, ctrl and alt state; a carriage
// return is Enter, and a line feed is Shift+Enter, which starts a new line
// of the same input.
public static void Type(string text) {
    var records = new System.Collections.Generic.List<KeyRecord>();
    foreach (char typed in text) {
        char c = typed == '\n' ? '\r' : typed;
        ushort vk = 0x0D;
        uint state = typed == '\n' ? 0x0010u : 0u;
        if (c != '\r') {
            short key = VkKeyScanW(c);
            vk = key == -1 ? (ushort)0 : (ushort)(key & 0xFF);
            if (key != -1 && (key & 0x100) != 0) state |= 0x0010;
            if (key != -1 && (key & 0x200) != 0) state |= 0x0008;
            if (key != -1 && (key & 0x400) != 0) state |= 0x0002;
        }
        var press = new KeyRecord();
        press.EventType = 1;
        press.KeyDown = 1;
        press.RepeatCount = 1;
        press.VirtualKeyCode = vk;
        press.VirtualScanCode = (ushort)MapVirtualKeyW(vk, 0);
        press.UnicodeChar = c;
        press.ControlKeyState = state;
        var release = press;
        release.KeyDown = 0;
        records.Add(press);
        records.Add(release);
    }
    int written;
    if (!WriteConsoleInputW(GetStdHandle(-10), records.ToArray(), records.Count, out written) || written != records.Count)
        throw new System.ComponentModel.Win32Exception();
}
'@
    $rendering = if (Get-Variable PSStyle -ErrorAction Ignore) { $PSStyle.OutputRendering } else { 'console colors' }
    [System.IO.File]::WriteAllText("$Marker.host", "PowerShell $($PSVersionTable.PSVersion), output rendering $rendering, NO_COLOR '$env:NO_COLOR'")
    [System.IO.File]::WriteAllText("$Marker.hwnd", [string][PwrsConsoleCapture.Session]::GetConsoleWindow().ToInt64())
    Import-Module PSReadLine
    Set-PSReadLineOption -HistorySaveStyle SaveNothing
    if ((Get-Command Set-PSReadLineOption).Parameters.ContainsKey('PredictionSource')) { Set-PSReadLineOption -PredictionSource None }
}

$global:PwrsCaptureMarker = $Marker
$global:PwrsCapturePrompts = 0
$global:PwrsCaptureSteps = 0
# The prompt PowerShell draws by default, with its row written down first.
function global:prompt {
    $global:PwrsCapturePrompts++
    $raw = $Host.UI.RawUI
    $path = "$global:PwrsCaptureMarker.prompt.$global:PwrsCapturePrompts"
    [System.IO.File]::WriteAllText("$path.part", "$($raw.CursorPosition.Y) $($raw.WindowPosition.Y) $($raw.WindowSize.Height)")
    [System.IO.File]::Move("$path.part", $path)
    "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) "
}
$null = Register-EngineEvent -SourceIdentifier PowerShell.OnIdle -Action {
    if ([System.IO.File]::Exists("$global:PwrsCaptureMarker.done")) { [System.Environment]::Exit(0) }
    $next = $global:PwrsCaptureSteps + 1
    $step = "$global:PwrsCaptureMarker.step.$next"
    if ([System.IO.File]::Exists($step)) {
        $global:PwrsCaptureSteps = $next
        [PwrsConsoleCapture.Session]::Type([System.IO.File]::ReadAllText($step))
        [System.IO.File]::WriteAllText("$global:PwrsCaptureMarker.typed.$next", '')
    }
}

. $Setup
Clear-Host
