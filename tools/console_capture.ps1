# Captures PowerShell console sessions as PNG images for the wiki: a
# classic console window (conhost.exe) on a chosen monitor, fed one step
# of typed input at a time, and captured from a step's first prompt down
# to its last. Dot-source it, then call Start-ConsoleCapture,
# Invoke-ConsoleStep for each step and Stop-ConsoleCapture.
#
# The console takes its settings from a shortcut the run writes: the
# font, colors and sizes HKCU\Console gives the shell when it is started
# by name, and a window origin on the monitor. Windows clamps a
# STARTUPINFO position onto the primary monitor, but not a shortcut's
# origin. SW_HIDE in STARTUPINFO leaves the window hidden. It is shown,
# without being activated, only while it is captured, and only once it
# lies wholly on the monitor; a window found anywhere else is hidden and
# closed at once. The session gets the environment Windows builds for the
# user, not this process's own, and tools/console_session.ps1 in it types
# each step into its own console input buffer, so no key reaches another
# window. Nothing is killed: the session exits once the run writes its
# done file, and a session that does not is closed as its close button
# would close it. The files
# the two halves pass each other are left in the temp folder.
#
# -Probe makes the same launch on a desktop of its own, which nothing
# displays, and takes no images.

if (-not ('PwrsConsoleCapture.Native' -as [type])) {
    Add-Type -AssemblyName System.Drawing
    Add-Type -Namespace PwrsConsoleCapture -Name Native -UsingNamespace System.Text, System.Collections.Generic -MemberDefinition @'
public delegate bool EnumProc(IntPtr h, IntPtr l);
public delegate bool MonitorProc(IntPtr m, IntPtr dc, ref RECT r, IntPtr l);
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct MONITORINFOEX { public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice; }
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct STARTUPINFO { public int cb; public string lpReserved; public string lpDesktop; public string lpTitle; public int dwX; public int dwY; public int dwXSize; public int dwYSize; public int dwXCountChars; public int dwYCountChars; public int dwFillAttribute; public int dwFlags; public short wShowWindow; public short cbReserved2; public IntPtr lpReserved2; public IntPtr hStdInput; public IntPtr hStdOutput; public IntPtr hStdError; }
[StructLayout(LayoutKind.Sequential)] public struct PROCESS_INFORMATION { public IntPtr hProcess; public IntPtr hThread; public int dwProcessId; public int dwThreadId; }
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct PROCESSENTRY32W { public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID; public uint th32ModuleID; public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase; public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile; }

[ComImport, Guid("00021401-0000-0000-C000-000000000046")] public class ShellLinkObject { }
[ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("000214F9-0000-0000-C000-000000000046")]
public interface IShellLinkW {
    void GetPath([MarshalAs(UnmanagedType.LPWStr)] StringBuilder file, int max, IntPtr data, uint flags);
    void GetIDList(out IntPtr list);
    void SetIDList(IntPtr list);
    void GetDescription([MarshalAs(UnmanagedType.LPWStr)] StringBuilder name, int max);
    void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string name);
    void GetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] StringBuilder dir, int max);
    void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string dir);
    void GetArguments([MarshalAs(UnmanagedType.LPWStr)] StringBuilder args, int max);
    void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string args);
    void GetHotkey(out short key);
    void SetHotkey(short key);
    void GetShowCmd(out int cmd);
    void SetShowCmd(int cmd);
    void GetIconLocation([MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int max, out int index);
    void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string path, int index);
    void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved);
    void Resolve(IntPtr hwnd, uint flags);
    void SetPath([MarshalAs(UnmanagedType.LPWStr)] string file);
}
[ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("45e2b4ae-b1c3-11d0-b92f-00a0c90312e1")]
public interface IShellLinkDataList {
    void AddDataBlock(IntPtr block);
    void CopyDataBlock(uint signature, out IntPtr block);
    void RemoveDataBlock(uint signature);
    void GetFlags(out uint flags);
    void SetFlags(uint flags);
}

[DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);
[DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr dc, IntPtr clip, MonitorProc f, IntPtr l);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool GetMonitorInfoW(IntPtr m, ref MONITORINFOEX mi);
[DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern IntPtr CreateDesktopW(string name, IntPtr device, IntPtr mode, uint flags, uint access, IntPtr attributes);
[DllImport("user32.dll", SetLastError = true)] public static extern bool CloseDesktop(IntPtr desk);
[DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
[DllImport("advapi32.dll", SetLastError = true)] public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
[DllImport("userenv.dll", SetLastError = true)] public static extern bool CreateEnvironmentBlock(out IntPtr env, IntPtr token, bool inherit);
[DllImport("userenv.dll", SetLastError = true)] public static extern bool DestroyEnvironmentBlock(IntPtr env);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern bool CreateProcessW(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
[DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr h, uint ms);
[DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
[DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern bool Process32FirstW(IntPtr s, ref PROCESSENTRY32W e);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern bool Process32NextW(IntPtr s, ref PROCESSENTRY32W e);
[DllImport("user32.dll")] public static extern bool EnumDesktopWindows(IntPtr desk, EnumProc f, IntPtr l);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
[DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
[DllImport("user32.dll", SetLastError = true)] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);

public class Screen { public string Device; public RECT Bounds; }
public static List<Screen> Screens() {
    var list = new List<Screen>();
    EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr m, IntPtr dc, ref RECT r, IntPtr l) => {
        var mi = new MONITORINFOEX();
        mi.cbSize = Marshal.SizeOf(typeof(MONITORINFOEX));
        GetMonitorInfoW(m, ref mi);
        list.Add(new Screen { Device = mi.szDevice, Bounds = mi.rcMonitor });
        return true;
    }, IntPtr.Zero);
    return list;
}
// The process and every process descended from it, by parent id.
public static List<uint> TreeOf(uint root) {
    var tree = new List<uint> { root };
    var snap = CreateToolhelp32Snapshot(2, 0);
    if (snap == new IntPtr(-1)) return tree;
    try {
        var links = new List<uint[]>();
        var e = new PROCESSENTRY32W();
        e.dwSize = (uint)Marshal.SizeOf(typeof(PROCESSENTRY32W));
        if (Process32FirstW(snap, ref e)) {
            do { links.Add(new uint[] { e.th32ProcessID, e.th32ParentProcessID }); } while (Process32NextW(snap, ref e));
        }
        for (int i = 0; i < tree.Count; i++)
            foreach (var link in links)
                if (link[1] == tree[i] && link[0] != tree[i] && !tree.Contains(link[0])) tree.Add(link[0]);
    } finally {
        CloseHandle(snap);
    }
    return tree;
}
// The top-level console window, shown or hidden, that a process in the
// tree created on the desktop (the thread's own when desk is zero).
public static IntPtr ConsoleWindowOf(List<uint> tree, IntPtr desk) {
    IntPtr found = IntPtr.Zero;
    EnumDesktopWindows(desk, (h, l) => {
        uint pid;
        GetWindowThreadProcessId(h, out pid);
        if (!tree.Contains(pid)) return true;
        var cls = new StringBuilder(64);
        GetClassNameW(h, cls, 64);
        if (cls.ToString() != "ConsoleWindowClass") return true;
        found = h;
        return false;
    }, IntPtr.Zero);
    return found;
}
// Saves a shortcut to target whose console properties (NT_CONSOLE_PROPS)
// have these settings, with automatic window positioning off.
public static void WriteConsoleLink(string path, string target, ushort fill, ushort popupFill,
    short bufferWidth, short bufferHeight, short windowWidth, short windowHeight, short x, short y,
    short fontWidth, short fontHeight, uint fontFamily, uint fontWeight, string faceName, uint cursorSize,
    bool quickEdit, bool insertMode, uint historySize, uint historyBuffers, bool historyNoDup, uint[] colors) {
    if (colors.Length != 16) throw new ArgumentException("the color table has sixteen colors");
    var face = Encoding.Unicode.GetBytes(faceName);
    if (face.Length > 62) throw new ArgumentException("a face name has at most 31 characters");
    var block = new byte[0xCC];
    Action<int, uint> u32 = (at, v) => BitConverter.GetBytes(v).CopyTo(block, at);
    Action<int, short> s16 = (at, v) => BitConverter.GetBytes(v).CopyTo(block, at);
    u32(0, 0xCC);
    u32(4, 0xA0000002);
    BitConverter.GetBytes(fill).CopyTo(block, 8);
    BitConverter.GetBytes(popupFill).CopyTo(block, 10);
    s16(12, bufferWidth); s16(14, bufferHeight);
    s16(16, windowWidth); s16(18, windowHeight);
    s16(20, x); s16(22, y);
    s16(32, fontWidth); s16(34, fontHeight);
    u32(36, fontFamily); u32(40, fontWeight);
    face.CopyTo(block, 44);
    u32(108, cursorSize);
    u32(116, quickEdit ? 1u : 0u);
    u32(120, insertMode ? 1u : 0u);
    u32(128, historySize); u32(132, historyBuffers); u32(136, historyNoDup ? 1u : 0u);
    for (int i = 0; i < 16; i++) u32(140 + 4 * i, colors[i]);
    var link = (IShellLinkW)new ShellLinkObject();
    link.SetPath(target);
    var pinned = GCHandle.Alloc(block, GCHandleType.Pinned);
    try { ((IShellLinkDataList)link).AddDataBlock(pinned.AddrOfPinnedObject()); } finally { pinned.Free(); }
    ((System.Runtime.InteropServices.ComTypes.IPersistFile)link).Save(path, true);
}
'@
}

# The console settings conhost gives a shell started by name: HKCU\Console,
# under the values of the key named after the shell's path, if there is
# one, with %SystemRoot% written as such and \ as _.
function Get-ConsoleSettings([string] $Exe) {
    $names = @('WindowSize', 'ScreenBufferSize', 'ScreenColors', 'PopupColors', 'FontSize', 'FontFamily', 'FontWeight', 'FaceName', 'CursorSize', 'QuickEdit', 'InsertMode', 'HistoryBufferSize', 'NumberOfHistoryBuffers', 'HistoryNoDup') + (0..15 | ForEach-Object { 'ColorTable{0:D2}' -f $_ })
    $settings = @{}
    $base = Get-ItemProperty -Path HKCU:\Console
    foreach ($name in $names) {
        if ($null -eq $base.$name) { throw "HKCU\Console has no $name" }
        $settings[$name] = $base.$name
    }
    $root = [regex]::Escape($env:SystemRoot)
    $key = ($Exe -replace "^$root", '%SystemRoot%').Replace('\', '_')
    $own = Get-ItemProperty -LiteralPath "HKCU:\Console\$key" -ErrorAction Ignore
    if ($own) { foreach ($name in $names) { if ($null -ne $own.$name) { $settings[$name] = $own.$name } } }
    $settings
}

# The window's rectangle, and whether it lies wholly inside Bounds.
function Get-WindowPlace([IntPtr] $Handle, $Bounds) {
    $r = New-Object PwrsConsoleCapture.Native+RECT
    $null = [PwrsConsoleCapture.Native]::GetWindowRect($Handle, [ref]$r)
    [pscustomobject]@{
        Inside = $r.Left -ge $Bounds.Left -and $r.Top -ge $Bounds.Top -and $r.Right -le $Bounds.Right -and $r.Bottom -le $Bounds.Bottom
        Text = "($($r.Left),$($r.Top))-($($r.Right),$($r.Bottom))"
    }
}

# Waits for Path to exist, while the session runs and within its time.
function Wait-SessionFile($Session, [string] $Path, [string] $What) {
    $until = [DateTime]::UtcNow.AddSeconds($Session.TimeoutSeconds)
    while (-not [System.IO.File]::Exists($Path)) {
        if ([PwrsConsoleCapture.Native]::WaitForSingleObject($Session.Process, 0) -eq 0) { throw "the console session ended before $What" }
        if ([DateTime]::UtcNow -gt $until) { throw "no $What within $($Session.TimeoutSeconds) s" }
        if (-not $Session.Shown -and [PwrsConsoleCapture.Native]::IsWindowVisible($Session.Handle)) {
            $place = Get-WindowPlace $Session.Handle $Session.Bounds
            if (-not $place.Inside) {
                $null = [PwrsConsoleCapture.Native]::ShowWindow($Session.Handle, 0)
                throw "the console window was shown at $($place.Text), off monitor $($Session.Monitor); it was hidden"
            }
        }
        Start-Sleep -Milliseconds 20
    }
}

# A prompt's record: cursor row, window top row and window height.
function Read-Prompt($Session, [int] $Index) {
    $v = [int[]](([System.IO.File]::ReadAllText("$($Session.Marker).prompt.$Index")).Trim() -split ' ')
    [pscustomobject]@{ Index = $Index; Row = $v[0]; Top = $v[1]; Height = $v[2] }
}

function Start-ConsoleCapture {
    param(
        [Parameter(Mandatory)] [int] $Monitor,
        # A script the session dot-sources before its first prompt.
        [Parameter(Mandatory)] [string] $Setup,
        [ValidateSet('pwsh', 'powershell')] [string] $Shell = 'pwsh',
        [int] $Columns = 120,
        [int] $Rows = 60,
        [int] $TimeoutSeconds = 120,
        [switch] $Probe
    )
    # Physical pixels on every monitor for this thread's window calls.
    $null = [PwrsConsoleCapture.Native]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
    $screen = [PwrsConsoleCapture.Native]::Screens() | Where-Object Device -eq "\\.\DISPLAY$Monitor"
    if (-not $screen) { throw "no monitor \\.\DISPLAY$Monitor" }
    $bounds = $screen.Bounds
    $folder = Join-Path ([System.IO.Path]::GetTempPath()) 'pwrs-console-capture'
    $null = New-Item -ItemType Directory -Force -Path $folder
    $id = [guid]::NewGuid().ToString('N')
    $marker = Join-Path $folder $id

    $conhost = "$env:SystemRoot\System32\conhost.exe"
    $exe = if ($Shell -eq 'powershell') { "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" }
           elseif ($PSVersionTable.PSEdition -eq 'Core') { [Environment]::ProcessPath }
           else { (Get-Command pwsh -CommandType Application).Source }
    $c = Get-ConsoleSettings $exe
    # A size value has its width in the low word and its height in the high one.
    $low = { param($v) [int16]([int]$v -band 0xFFFF) }
    $high = { param($v) [int16](([int]$v -shr 16) -band 0xFFFF) }
    $colors = [uint32[]](0..15 | ForEach-Object { $c['ColorTable{0:D2}' -f $_] })
    $link = "$marker.lnk"
    [PwrsConsoleCapture.Native]::WriteConsoleLink($link, $conhost, $c.ScreenColors, $c.PopupColors,
        $Columns, (& $high $c.ScreenBufferSize), $Columns, $Rows, ($bounds.Left + 40), ($bounds.Top + 40),
        (& $low $c.FontSize), (& $high $c.FontSize), $c.FontFamily, $c.FontWeight, $c.FaceName, $c.CursorSize,
        ($c.QuickEdit -ne 0), ($c.InsertMode -ne 0), $c.HistoryBufferSize, $c.NumberOfHistoryBuffers, ($c.HistoryNoDup -ne 0), $colors)

    $session = Join-Path $PSScriptRoot 'console_session.ps1'
    $line = "`"$conhost`" `"$exe`" -NoLogo -NoProfile -NoExit -ExecutionPolicy Bypass -File `"$session`" -Marker `"$marker`" -Setup `"$Setup`""
    $si = New-Object PwrsConsoleCapture.Native+STARTUPINFO
    $si.cb = [System.Runtime.InteropServices.Marshal]::SizeOf([type][PwrsConsoleCapture.Native+STARTUPINFO])
    # STARTF_USESHOWWINDOW with SW_HIDE; STARTF_TITLEISLINKNAME, so the
    # title names the shortcut conhost reads its settings from.
    $si.dwFlags = 0x1 -bor 0x800
    $si.wShowWindow = 0
    $si.lpTitle = $link
    $desk = [IntPtr]::Zero
    if ($Probe) {
        $desktop = "pwrs-console-capture-$id"
        # GENERIC_ALL.
        $desk = [PwrsConsoleCapture.Native]::CreateDesktopW($desktop, [IntPtr]::Zero, [IntPtr]::Zero, 0, 0x10000000, [IntPtr]::Zero)
        if ($desk -eq [IntPtr]::Zero) { throw [System.ComponentModel.Win32Exception]::new() }
        $si.lpDesktop = "WinSta0\$desktop"
    }

    # TOKEN_QUERY | TOKEN_DUPLICATE.
    $token = [IntPtr]::Zero
    if (-not [PwrsConsoleCapture.Native]::OpenProcessToken([PwrsConsoleCapture.Native]::GetCurrentProcess(), 0x000A, [ref]$token)) { throw [System.ComponentModel.Win32Exception]::new() }
    $environment = [IntPtr]::Zero
    try {
        if (-not [PwrsConsoleCapture.Native]::CreateEnvironmentBlock([ref]$environment, $token, $false)) { throw [System.ComponentModel.Win32Exception]::new() }
    } finally {
        $null = [PwrsConsoleCapture.Native]::CloseHandle($token)
    }
    $launch = New-Object PwrsConsoleCapture.Native+PROCESS_INFORMATION
    try {
        # CREATE_UNICODE_ENVIRONMENT. PowerShell passes $null to a string
        # parameter as "", so the directory is passed as a true null.
        if (-not [PwrsConsoleCapture.Native]::CreateProcessW($conhost, [System.Text.StringBuilder]::new($line), [IntPtr]::Zero, [IntPtr]::Zero, $false, 0x400, $environment, [NullString]::Value, [ref]$si, [ref]$launch)) {
            throw [System.ComponentModel.Win32Exception]::new()
        }
    } finally {
        $null = [PwrsConsoleCapture.Native]::DestroyEnvironmentBlock($environment)
    }
    $s = [pscustomobject]@{
        Monitor = $Monitor; Bounds = $bounds; Marker = $marker; Process = $launch.hProcess; Thread = $launch.hThread
        ProcessId = $launch.dwProcessId; Desk = $desk; Handle = [IntPtr]::Zero; Shown = $false; Probe = [bool]$Probe
        TimeoutSeconds = $TimeoutSeconds; Steps = 0; Last = $null; Host = ''
    }
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(20)
        while ($s.Handle -eq [IntPtr]::Zero) {
            if ([PwrsConsoleCapture.Native]::WaitForSingleObject($s.Process, 0) -eq 0) { throw 'conhost ended before its window appeared' }
            if ([DateTime]::UtcNow -gt $deadline) { throw 'the console window never appeared' }
            $s.Handle = [PwrsConsoleCapture.Native]::ConsoleWindowOf([PwrsConsoleCapture.Native]::TreeOf([uint32]$s.ProcessId), $desk)
            if ($s.Handle -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 5 }
        }
        $place = Get-WindowPlace $s.Handle $bounds
        if (-not $place.Inside) {
            $null = [PwrsConsoleCapture.Native]::ShowWindow($s.Handle, 0)
            $null = [PwrsConsoleCapture.Native]::PostMessageW($s.Handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
            throw "the console window was created at $($place.Text), not wholly on monitor $Monitor; it was hidden and sent a close message"
        }
        Wait-SessionFile $s "$marker.hwnd" 'the session named its console window'
        $s.Host = [System.IO.File]::ReadAllText("$marker.host")
        $named = [IntPtr]::new([long][System.IO.File]::ReadAllText("$marker.hwnd"))
        if ($named -ne $s.Handle) { throw "the session's console window is $named, not $($s.Handle)" }
        Wait-SessionFile $s "$marker.prompt.1" 'the first prompt'
        $s.Last = Read-Prompt $s 1
    } catch {
        $null = Stop-ConsoleCapture $s
        throw
    }
    $s
}

# Shows the window where it is, without activating it, above the windows
# beside it, and waits for it to draw.
function Show-CaptureWindow($Session) {
    $place = Get-WindowPlace $Session.Handle $Session.Bounds
    if (-not $place.Inside) { throw "the console window is at $($place.Text), off monitor $($Session.Monitor)" }
    # SW_SHOWNOACTIVATE, then HWND_TOP with SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE.
    $null = [PwrsConsoleCapture.Native]::ShowWindow($Session.Handle, 4)
    $null = [PwrsConsoleCapture.Native]::SetWindowPos($Session.Handle, [IntPtr]::Zero, 0, 0, 0, 0, 0x0013)
    $Session.Shown = $true
    Start-Sleep -Milliseconds 500
}

function Hide-CaptureWindow($Session) {
    $null = [PwrsConsoleCapture.Native]::ShowWindow($Session.Handle, 0)
    $Session.Shown = $false
}

# Saves the client area's rows FirstRow through LastRow, and half a row
# under them, as a PNG; with LastRow below zero, down to the last row
# whose middle line of pixels is not all background, since text drawn in
# a row covers its middle.
function Save-ConsoleRows($Session, [string] $Out, [int] $FirstRow, [int] $LastRow, [int] $Height) {
    $client = New-Object PwrsConsoleCapture.Native+RECT
    $null = [PwrsConsoleCapture.Native]::GetClientRect($Session.Handle, [ref]$client)
    $cell = $client.Bottom / $Height
    $whole = [System.Drawing.Bitmap]::new($client.Right, $client.Bottom, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    try {
        $g = [System.Drawing.Graphics]::FromImage($whole)
        $dc = $g.GetHdc()
        # PW_CLIENTONLY | PW_RENDERFULLCONTENT.
        try { $printed = [PwrsConsoleCapture.Native]::PrintWindow($Session.Handle, $dc, 3) } finally { $g.ReleaseHdc($dc); $g.Dispose() }
        if (-not $printed) { throw 'PrintWindow drew nothing' }
        $background = $whole.GetPixel($client.Right - 1, $client.Bottom - 1).ToArgb()
        if ($LastRow -lt 0) {
            $LastRow = $FirstRow
            for ($row = $Height - 1; $row -gt $FirstRow; $row--) {
                $y = [int][Math]::Floor(($row + 0.5) * $cell)
                $plain = $true
                for ($x = 0; $x -lt $client.Right; $x++) { if ($whole.GetPixel($x, $y).ToArgb() -ne $background) { $plain = $false; break } }
                if (-not $plain) { $LastRow = $row; break }
            }
        }
        $top = [int][Math]::Floor($FirstRow * $cell)
        $bottom = [int][Math]::Min($client.Bottom, [Math]::Ceiling(($LastRow + 1.5) * $cell))
        $image = $whole.Clone([System.Drawing.Rectangle]::new(0, $top, $client.Right, $bottom - $top), $whole.PixelFormat)
    } finally {
        $whole.Dispose()
    }
    try {
        $plain = $true
        for ($y = 0; $y -lt $image.Height -and $plain; $y += 2) {
            for ($x = 0; $x -lt $image.Width; $x += 2) { if ($image.GetPixel($x, $y).ToArgb() -ne $background) { $plain = $false; break } }
        }
        if ($plain) { throw "the capture for $Out is all background" }
        $image.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
        "image $Out $($image.Width)x$($image.Height)"
    } finally {
        $image.Dispose()
    }
}

# Types Text into the session and waits for the step's last prompt. Each
# Enter in Text (a carriage return) ends one input and each input ends at
# one prompt, so the last is that many prompts on from the first, and
# Text cannot include a reply to a command's prompt. With -Image, saves the
# console from the step's first prompt down to its last; with -During as
# well, saves it that many milliseconds after the text is typed, down to
# the last row in use, for what shows only while a command runs.
function Invoke-ConsoleStep {
    param(
        [Parameter(Mandatory)] $Session,
        [Parameter(Mandatory)] [string] $Text,
        [string] $Image = '',
        [int] $During = 0
    )
    $Session.Steps++
    $k = $Session.Steps
    $start = $Session.Last
    $take = $Image -and -not $Session.Probe
    if ($take -and $During) { Show-CaptureWindow $Session }
    $step = "$($Session.Marker).step.$k"
    [System.IO.File]::WriteAllText("$step.part", $Text, [System.Text.UTF8Encoding]::new($false))
    [System.IO.File]::Move("$step.part", $step)
    if ($take -and $During) {
        Wait-SessionFile $Session "$($Session.Marker).typed.$k" "step $k to be typed"
        Start-Sleep -Milliseconds $During
        Save-ConsoleRows $Session $Image ($start.Row - $start.Top) -1 $start.Height
    }
    $enters = @($Text.ToCharArray() | Where-Object { $_ -eq "`r" }).Count
    if ($enters -lt 1) { throw "step $k has no Enter, so it never reaches a prompt" }
    $index = $start.Index + $enters
    Wait-SessionFile $Session "$($Session.Marker).prompt.$index" "prompt $index, the last of step $k"
    $last = Read-Prompt $Session $index
    $Session.Last = $last
    if ($Image -and $last.Top -ne $start.Top) { throw "step $k scrolled the window: its output does not fit in $($last.Height) rows" }
    if ($take -and -not $During) {
        Show-CaptureWindow $Session
        Save-ConsoleRows $Session $Image ($start.Row - $start.Top) ($last.Row - $last.Top) $last.Height
    }
    if ($Session.Shown) { Hide-CaptureWindow $Session }
}

# Writes the session's done file so it exits, closes it as its close
# button would if it does not, and frees the handles the run has open.
# Returns what happened.
function Stop-ConsoleCapture($Session) {
    $state = 'the console session ended'
    if ([PwrsConsoleCapture.Native]::WaitForSingleObject($Session.Process, 0) -ne 0) {
        [System.IO.File]::WriteAllText("$($Session.Marker).done", '')
        if ([PwrsConsoleCapture.Native]::WaitForSingleObject($Session.Process, 10000) -ne 0 -and $Session.Handle -ne [IntPtr]::Zero) {
            # WM_CLOSE, as the close button sends.
            $null = [PwrsConsoleCapture.Native]::PostMessageW($Session.Handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
            $null = [PwrsConsoleCapture.Native]::WaitForSingleObject($Session.Process, 5000)
        }
        if ([PwrsConsoleCapture.Native]::WaitForSingleObject($Session.Process, 0) -ne 0) {
            $state = "LEFT OPEN: conhost $($Session.ProcessId), started by this run, did not close"
        }
    }
    $null = [PwrsConsoleCapture.Native]::CloseHandle($Session.Thread)
    $null = [PwrsConsoleCapture.Native]::CloseHandle($Session.Process)
    if ($Session.Desk -ne [IntPtr]::Zero) { $null = [PwrsConsoleCapture.Native]::CloseDesktop($Session.Desk) }
    $state
}
