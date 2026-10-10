# A module whose native library cannot load: its import fails, in a child
# process of this host, with an exception naming the library and what it
# lacks. The library is the bundled Calc's, copied with Calc's
# folder and edited so that a file it needs is one no system has: a DLL its
# PE import directory names on Windows, a DT_NEEDED entry on Linux and
# FreeBSD, an LC_LOAD_DYLIB path on macOS, which is then signed again ad
# hoc. On Windows the message is PWRS's own, read from the import graph;
# elsewhere it carries dlerror's text. PWRS_MODULE points at the built
# module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $script:calc = Join-Path $module 'Calc'
    $script:hostExe = (Get-Process -Id $PID).Path
    $info = [System.Runtime.InteropServices.RuntimeInformation]
    $platform = [System.Runtime.InteropServices.OSPlatform]
    $script:onWindows = [IO.Path]::DirectorySeparatorChar -eq '\'
    $script:onMac = -not $onWindows -and $info::IsOSPlatform($platform::OSX)
    $os = if ($onWindows) { 'win' } elseif ($onMac) { 'osx' } elseif ($info::IsOSPlatform($platform::Linux)) { 'linux' } else { 'freebsd' }
    $script:process = $info::ProcessArchitecture.ToString()
    $script:rid = $os + '-' + $process.ToLowerInvariant()
    $script:libFile = if ($onWindows) { 'pwrs_example_calc.dll' } elseif ($onMac) { 'libpwrs_example_calc.dylib' } else { 'libpwrs_example_calc.so' }

    # A copy of Calc's folder in a new folder named $name: its manifest
    # and the path of its library for this platform.
    function script:Copy-Calc([string]$name) {
        $into = Join-Path $TestDrive $name
        $null = New-Item -ItemType Directory -Path $into
        Copy-Item -LiteralPath $calc -Destination $into -Recurse
        $root = (Resolve-Path -LiteralPath (Join-Path $into 'Calc')).ProviderPath
        [pscustomobject]@{ Psd1 = Join-Path $root 'Calc.psd1'; Library = [IO.Path]::Combine($root, 'runtimes', $rid, 'native', $libFile) }
    }

    # Imports $psd1 in a fresh process of this host, after running $before
    # there, and answers IMPORTED, or REFUSED with the exception's type and
    # message. The command goes encoded, so its quotes reach the child.
    function script:Invoke-Import([string]$psd1, [string]$before = '') {
        $body = $before + "; try { Import-Module '$psd1' -ErrorAction Stop; 'IMPORTED' } catch { 'REFUSED ' + `$_.Exception.GetType().FullName + ': ' + `$_.Exception.Message }"
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($body))
        @(& $hostExe -NoProfile -NonInteractive -EncodedCommand $encoded) -join "`n"
    }

    # A child step that puts $folder on PATH and adds it with
    # AddDllDirectory, so the child's loads search it whether its host
    # searches PATH, as an unpackaged one does, or only the default
    # folders and those added, as a packaged one does.
    function script:Get-SearchedStep([string]$folder) {
        '$env:PATH = ''' + $folder + ';'' + $env:PATH; ' +
        '$dirs = Add-Type -PassThru -Namespace PwrsLoadFailureTest -Name Dirs -MemberDefinition ''[DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)] public static extern IntPtr AddDllDirectory(string folder);''; ' +
        'if ($dirs::AddDllDirectory(''' + $folder + ''') -eq [IntPtr]::Zero) { throw ''AddDllDirectory refused ' + $folder + ''' }'
    }

    # The text Windows gives for a Win32 error, as the message quotes it.
    function script:Get-ErrorText([int]$code) {
        (New-Object System.ComponentModel.Win32Exception $code).Message.Replace('%1', $libFile).TrimEnd(' ', '.', "`r", "`n")
    }

    # The start of the message for the copy's library failing with $code,
    # with the type the host throws for it: on PowerShell 7
    # BadImageFormatException for a bad image, the type NativeLibrary.Load
    # throws, and DllNotFoundException otherwise.
    function script:Get-Opening($copy, [int]$code) {
        $type = if ($code -eq 193 -and $PSVersionTable.PSEdition -eq 'Core') { 'System.BadImageFormatException' } else { 'System.DllNotFoundException' }
        "REFUSED $($type): $($copy.Library) could not be loaded: Windows error $code ($(Get-ErrorText $code))."
    }

    # Writes $to over a name the file holds at Offset, padded with NULs
    # to its Length.
    function script:Set-Name([byte[]]$bytes, $name, [string]$to) {
        if ($to.Length -gt $name.Length) { throw "$to is longer than $($name.Name)" }
        $ascii = [Text.Encoding]::ASCII.GetBytes($to)
        for ($i = 0; $i -lt $name.Length; $i++) {
            $bytes[$name.Offset + $i] = if ($i -lt $ascii.Length) { $ascii[$i] } else { 0 }
        }
    }

    # The NUL-terminated name at $at.
    function script:Read-Name([byte[]]$bytes, [long]$at, [long]$room) {
        $end = $at
        while ($bytes[$end] -ne 0) { $end++ }
        if ($room -lt 0) { $room = $end - $at }
        [pscustomobject]@{ Name = [Text.Encoding]::ASCII.GetString($bytes, $at, $end - $at); Offset = $at; Length = $room }
    }

    # The DLLs a PE file's import directory names.
    function script:Get-PeImport([byte[]]$bytes) {
        $pe = [BitConverter]::ToInt32($bytes, 0x3c)
        $sections = [BitConverter]::ToUInt16($bytes, $pe + 6)
        $optional = $pe + 24
        $table = $optional + [BitConverter]::ToUInt16($bytes, $pe + 20)
        $directories = if ([BitConverter]::ToUInt16($bytes, $optional) -eq 0x20b) { $optional + 112 } else { $optional + 96 }
        $offset = {
            param([long]$rva)
            for ($i = 0; $i -lt $sections; $i++) {
                $s = $table + 40 * $i
                $address = [long][BitConverter]::ToUInt32($bytes, $s + 12)
                $size = [Math]::Max([long][BitConverter]::ToUInt32($bytes, $s + 8), [long][BitConverter]::ToUInt32($bytes, $s + 16))
                if ($rva -ge $address -and $rva -lt $address + $size) { return $rva - $address + [BitConverter]::ToUInt32($bytes, $s + 20) }
            }
            throw "no section of the file holds address $rva"
        }
        for ($d = & $offset ([BitConverter]::ToUInt32($bytes, $directories + 8)); [BitConverter]::ToUInt32($bytes, $d + 12) -ne 0; $d += 20) {
            Read-Name $bytes (& $offset ([BitConverter]::ToUInt32($bytes, $d + 12))) -1
        }
    }

    # Renames the longest DLL the PE file at $path imports that is not an
    # API set, or with -ApiSet the longest that is, to $to.
    function script:Rename-PeImport([string]$path, [string]$to, [switch]$ApiSet) {
        $bytes = [IO.File]::ReadAllBytes($path)
        $import = @(Get-PeImport $bytes | Where-Object { ($_.Name -match '^(api|ext)-') -eq [bool]$ApiSet -and $_.Length -ge $to.Length } | Sort-Object Length -Descending)
        if ($import.Count -eq 0) { throw "$path imports nothing $to fits over" }
        Set-Name $bytes $import[0] $to
        [IO.File]::WriteAllBytes($path, $bytes)
    }

    # Sets the machine the PE file at $path says it is built for.
    function script:Set-PeMachine([string]$path, [int]$machine) {
        $bytes = [IO.File]::ReadAllBytes($path)
        $pe = [BitConverter]::ToInt32($bytes, 0x3c)
        $bytes[$pe + 4] = $machine -band 0xff
        $bytes[$pe + 5] = ($machine -shr 8) -band 0xff
        [IO.File]::WriteAllBytes($path, $bytes)
    }

    # Renames the longest DT_NEEDED entry of the 64-bit little-endian ELF
    # file at $path to $to.
    function script:Rename-ElfNeeded([string]$path, [string]$to) {
        $bytes = [IO.File]::ReadAllBytes($path)
        if ($bytes[4] -ne 2 -or $bytes[5] -ne 1) { throw "$path is not a 64-bit little-endian ELF file" }
        $headers = [BitConverter]::ToInt64($bytes, 0x20)
        $headerSize = [BitConverter]::ToUInt16($bytes, 0x36)
        $loads = @()
        $dynamic = $null
        for ($i = 0; $i -lt [BitConverter]::ToUInt16($bytes, 0x38); $i++) {
            $p = $headers + $i * $headerSize
            $segment = [pscustomobject]@{ Offset = [BitConverter]::ToInt64($bytes, $p + 8); Address = [BitConverter]::ToInt64($bytes, $p + 16); Size = [BitConverter]::ToInt64($bytes, $p + 32) }
            switch ([BitConverter]::ToUInt32($bytes, $p)) { 1 { $loads += $segment } 2 { $dynamic = $segment } }
        }
        $needed = @()
        $strings = $null
        for ($d = $dynamic.Offset; $d -lt $dynamic.Offset + $dynamic.Size; $d += 16) {
            $tag = [BitConverter]::ToInt64($bytes, $d)
            if ($tag -eq 0) { break }
            if ($tag -eq 1) { $needed += [BitConverter]::ToInt64($bytes, $d + 8) }
            elseif ($tag -eq 5) { $strings = [BitConverter]::ToInt64($bytes, $d + 8) }
        }
        $load = @($loads | Where-Object { $strings -ge $_.Address -and $strings -lt $_.Address + $_.Size })[0]
        $base = $strings - $load.Address + $load.Offset
        $entry = @(foreach ($n in $needed) { Read-Name $bytes ($base + $n) -1 }) | Where-Object Length -ge $to.Length | Sort-Object Length -Descending | Select-Object -First 1
        if (-not $entry) { throw "$path needs nothing $to fits over" }
        Set-Name $bytes $entry $to
        [IO.File]::WriteAllBytes($path, $bytes)
    }

    # Renames the longest LC_LOAD_DYLIB path of the 64-bit Mach-O file at
    # $path to $to, and signs the file again ad hoc, as the edit voids the
    # signature the linker gave it.
    function script:Rename-MachODylib([string]$path, [string]$to) {
        $bytes = [IO.File]::ReadAllBytes($path)
        if ($bytes[0] -ne 0xcf -or $bytes[1] -ne 0xfa -or $bytes[2] -ne 0xed -or $bytes[3] -ne 0xfe) { throw "$path is not a 64-bit Mach-O file" }
        $at = [long]32
        $dylibs = for ($i = 0; $i -lt [BitConverter]::ToUInt32($bytes, 16); $i++) {
            $size = [long][BitConverter]::ToUInt32($bytes, $at + 4)
            if ([BitConverter]::ToUInt32($bytes, $at) -eq 0xc) {
                $name = $at + [BitConverter]::ToUInt32($bytes, $at + 8)
                Read-Name $bytes $name ($at + $size - $name - 1)
            }
            $at += $size
        }
        $dylib = @($dylibs) | Where-Object Length -ge $to.Length | Sort-Object Length -Descending | Select-Object -First 1
        if (-not $dylib) { throw "$path loads nothing $to fits over" }
        Set-Name $bytes $dylib $to
        [IO.File]::WriteAllBytes($path, $bytes)
        $signed = @(& codesign --force --sign - $path 2>&1)
        if ($LASTEXITCODE -ne 0) { throw "codesign could not sign $path again: $($signed -join ' ')" }
    }
}

Describe 'A module whose native library cannot load' {
    It 'names the library and the file it needs that this system does not have' {
        $copy = Copy-Calc 'missing'
        if ($onWindows) {
            Rename-PeImport $copy.Library 'pwrs-gone.dll'
        } elseif ($onMac) {
            Rename-MachODylib $copy.Library '/usr/lib/pwrs-gone.dylib'
        } else {
            Rename-ElfNeeded $copy.Library 'pwrs-gone.so'
        }
        $out = Invoke-Import $copy.Psd1
        if ($onWindows) {
            $out | Should -BeExactly ((Get-Opening $copy 126) + " It imports pwrs-gone.dll, which is not on this process's DLL search path.")
        } else {
            $out | Should -BeLike ('REFUSED System.DllNotFoundException: ' + [System.Management.Automation.WildcardPattern]::Escape($copy.Library) + ' could not be loaded: *pwrs-gone*')
        }
    }
}

# The cases only Windows has, where PWRS reads the import graph itself.
# The API set case needs IsApiSetImplemented, which Windows has from 10 on.
if ([IO.Path]::DirectorySeparatorChar -eq '\') {
    Describe 'A module whose native library Windows cannot load' {
        It 'names the DLL that a DLL it imports needs and this system does not have' {
            $copy = Copy-Calc 'chain'
            $folder = Join-Path $TestDrive 'chain-path'
            $null = New-Item -ItemType Directory -Path $folder
            $middle = Join-Path $folder 'pwrs-middle.dll'
            Copy-Item -LiteralPath $copy.Library -Destination $middle
            Rename-PeImport $middle 'pwrs-gone.dll'
            Rename-PeImport $copy.Library 'pwrs-middle.dll'
            $out = Invoke-Import $copy.Psd1 (Get-SearchedStep $folder)
            $out | Should -BeExactly ((Get-Opening $copy 126) + " It imports pwrs-middle.dll, which imports pwrs-gone.dll, which is not on this process's DLL search path.")
        }

        It 'names a DLL it imports that is only on PATH, in a host that does not search PATH' {
            $copy = Copy-Calc 'pathonly'
            $folder = Join-Path $TestDrive 'pathonly-path'
            $null = New-Item -ItemType Directory -Path $folder
            $middle = Join-Path $folder 'pwrs-middle.dll'
            Copy-Item -LiteralPath $copy.Library -Destination $middle
            Rename-PeImport $middle 'pwrs-gone.dll'
            Rename-PeImport $copy.Library 'pwrs-middle.dll'
            $out = Invoke-Import $copy.Psd1 "`$env:PATH = '$folder;' + `$env:PATH"
            # An unpackaged host searches PATH and finds the DLL there; a
            # packaged one, such as pwsh installed from its MSIX package,
            # does not.
            $out | Should -BeIn @(
                ((Get-Opening $copy 126) + " It imports pwrs-middle.dll, which imports pwrs-gone.dll, which is not on this process's DLL search path."),
                ((Get-Opening $copy 126) + " It imports pwrs-middle.dll, which is not on this process's DLL search path. $middle exists, in a folder this process does not search for DLLs.")
            )
        }

        if ([Environment]::OSVersion.Version.Major -ge 10) {
            It 'names an API set it imports that this Windows does not implement' {
                $copy = Copy-Calc 'apiset'
                Rename-PeImport $copy.Library 'ext-ms-win-pwrs-gone-l1-1-0.dll' -ApiSet
                $out = Invoke-Import $copy.Psd1
                $out | Should -BeExactly ((Get-Opening $copy 126) + ' It imports ext-ms-win-pwrs-gone-l1-1-0.dll, an API set this version of Windows does not implement.')
            }
        }

        It 'names the machine the library is built for when this process runs another' {
            $copy = Copy-Calc 'machine'
            $names = @{ X86 = 'x86'; X64 = 'x64'; Arm = 'ARM'; Arm64 = 'ARM64' }
            $other, $otherName = if ($process -eq 'X64') { 0xaa64, 'ARM64' } elseif ($process -eq 'X86') { 0x8664, 'x64' } else { 0x014c, 'x86' }
            Set-PeMachine $copy.Library $other
            $out = Invoke-Import $copy.Psd1
            $out | Should -BeExactly ((Get-Opening $copy 193) + " The library is built for $otherName, and this process runs $($names[$process]).")
        }

        It 'names a DLL it imports whose copy on the search path is not a Windows DLL' {
            $copy = Copy-Calc 'notpe'
            $folder = Join-Path $TestDrive 'notpe-path'
            $null = New-Item -ItemType Directory -Path $folder
            $middle = Join-Path $folder 'pwrs-middle.dll'
            [IO.File]::WriteAllText($middle, 'not a DLL')
            Rename-PeImport $copy.Library 'pwrs-middle.dll'
            $out = Invoke-Import $copy.Psd1 (Get-SearchedStep $folder)
            $out | Should -BeExactly ((Get-Opening $copy 193) + " It imports pwrs-middle.dll, whose copy at $middle is not a Windows DLL.")
        }
    }
}
