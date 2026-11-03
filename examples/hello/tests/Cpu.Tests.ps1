# The CPU check at import, and pwrs::cpu as a module sees it. A cap is
# proved in a child process of the same host, because the check runs once,
# when a process first loads the library. PWRS_MODULE points at the built
# module folder.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    $script:psd1 = Join-Path $module 'Hello.psd1'
    Import-Module $psd1 -Force -ErrorAction Stop
    $script:hostExe = (Get-Process -Id $PID).Path
    $script:compiled = @(Get-RustCpu -Compiled)
    $script:arm64 = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -eq 'Arm64'
    # The caps this architecture takes, lowest first, as PWRS_CPU_MAX
    # names them and Get-RustCpu writes them in Level.
    $script:caps = if ($arm64) { @('neon', 'dotprod', 'i8mm', 'sve', 'sve2') } else { @('x86-64', 'x86-64-v2', 'x86-64-v3', 'x86-64-v4') }

    # Imports the module in a fresh process of this host under the given
    # PWRS_CPU_MAX and runs $then there; returns what the child printed: a
    # WARNED line for each warning the import wrote, such as the one for
    # the bundled Calc, whose library the cap refuses as it refuses
    # Hello's, then IMPORTED and what $then printed, or REFUSED and the
    # error's message.
    function script:Invoke-Capped([string]$cap, [string]$then) {
        $saved = $env:PWRS_CPU_MAX
        $env:PWRS_CPU_MAX = $cap
        try {
            $body = "try { Import-Module '$psd1' -ErrorAction Stop 3>&1 | ForEach-Object { 'WARNED ' + `$_.Message }; 'IMPORTED'; $then } catch { 'REFUSED ' + `$_.Exception.Message }"
            @(& $hostExe -NoProfile -NonInteractive -Command $body)
        } finally {
            $env:PWRS_CPU_MAX = $saved
        }
    }

    # The lines of a capped child's output after its warnings. A single
    # line comes back as a string, so callers wrap the call in @().
    function script:Get-Verdict([string[]]$out) {
        @($out | Where-Object { $_ -notlike 'WARNED *' })
    }

    # The child's warnings that Hello imports without Calc.
    function script:Get-CalcWarning([string[]]$out) {
        @($out | Where-Object { $_ -like 'WARNED Hello imports without its bundled module Calc, whose import failed: *' })
    }
}

Describe 'What the library was compiled for' {
    It 'is offered by the machine it was built and imported on' {
        foreach ($f in $compiled) {
            $f.Detected | Should -BeTrue -Because "$($f.Name) was compiled in"
            $f.Usable | Should -BeTrue -Because "nothing caps $($f.Name) in this process"
        }
    }

    It 'lists every extension the machine is checked for' {
        $all = @(Get-RustCpu)
        @($all | Where-Object Compiled).Count | Should -Be $compiled.Count
        if ($arm64) {
            $all.Count | Should -BeGreaterThan 20
            ($all | Where-Object Name -eq 'sve2').Level | Should -Be 'sve2'
            ($all | Where-Object Name -eq 'neon').Baseline | Should -BeTrue
        } else {
            $all.Count | Should -BeGreaterThan 40
            ($all | Where-Object Name -eq 'avx512f').Level | Should -Be 'x86-64-v4'
        }
    }
}

Describe 'PWRS_CPU_MAX at import' {
    It 'refuses the import when the cap withholds an extension the library was compiled for, and allows it otherwise' {
        $lowest = $caps[0]
        $withheld = @($compiled | Where-Object { $_.Level -ne $lowest -and -not $_.Baseline })
        $out = @(Invoke-Capped $lowest '')
        $verdict = @(Get-Verdict $out)
        if ($withheld.Count -gt 0) {
            $verdict[0] | Should -BeLike 'REFUSED *'
            $verdict[0] | Should -BeLike "*PWRS_CPU_MAX=$lowest withholds*"
            $verdict[0] | Should -BeLike "*$($withheld[0].Name)*"
            @(Get-CalcWarning $out | Where-Object { $_ -like "*PWRS_CPU_MAX=$lowest withholds*" }).Count | Should -Be 1 -Because ($out -join "`n")
        } else {
            $verdict[0] | Should -Be 'IMPORTED'
            @(Get-CalcWarning $out).Count | Should -Be 0 -Because ($out -join "`n")
        }
    }

    It 'caps what a kernel may use, or refuses when the library needs more' {
        $cap, $inside, $above = if ($arm64) { 'dotprod', 'dotprod', 'i8mm' } else { 'x86-64-v3', 'avx2', 'avx512f' }
        $allowed = $caps[0..$caps.IndexOf($cap)]
        $beyond = @($compiled | Where-Object { $_.Level -notin $allowed -and -not $_.Baseline })
        $out = @(Invoke-Capped $cap "(Get-RustCpu | Where-Object Name -in '$inside', '$above' | ForEach-Object { `$_.Name + '=' + `$_.Usable }) -join ' '")
        $verdict = @(Get-Verdict $out)
        if ($beyond.Count -gt 0) {
            $verdict[0] | Should -BeLike 'REFUSED *'
            @(Get-CalcWarning $out).Count | Should -Be 1 -Because ($out -join "`n")
        } else {
            $verdict[0] | Should -Be 'IMPORTED'
            $detected = (Get-RustCpu | Where-Object Name -eq $inside).Detected
            $verdict[1] | Should -Be ("$inside=$detected $above=False")
        }
    }

    It 'runs the tiered kernel at the widest tier the cap and the CPU allow, with the same bits as the script' {
        $values = [double[]](1..1003 | ForEach-Object { [math]::Sqrt($_) * 1e-3 + 1 / $_ })
        $tier, $sum = Measure-RustTieredSum -Values $values
        $acc = [double[]]::new(8)
        for ($i = 0; $i -lt $values.Length; $i++) { $acc[$i % 8] += $values[$i] }
        $want = (($acc[0] + $acc[4]) + ($acc[2] + $acc[6])) + (($acc[1] + $acc[5]) + ($acc[3] + $acc[7]))
        [BitConverter]::DoubleToInt64Bits($sum) | Should -Be ([BitConverter]::DoubleToInt64Bits($want))
        $cpu = @(Get-RustCpu)
        $expected = if (($cpu | Where-Object Name -eq 'avx512f').Usable) { 'avx512f' } elseif (($cpu | Where-Object Name -eq 'avx2').Usable) { 'avx2' } else { 'scalar' }
        $tier | Should -Be $expected
    }

    It 'refuses a cap that names no level' {
        $out = @(Invoke-Capped 'x86-64-v9' '')
        @(Get-Verdict $out)[0] | Should -BeLike "REFUSED *PWRS_CPU_MAX is 'x86-64-v9'*"
        @(Get-CalcWarning $out | Where-Object { $_ -like "*PWRS_CPU_MAX is 'x86-64-v9'*" }).Count | Should -Be 1 -Because ($out -join "`n")
    }

    It 'refuses a cap naming the other architecture''s tier, naming the ones this architecture takes' {
        if ($arm64) {
            $other, $takes = 'x86-64-v3', 'on Arm64 it takes neon, dotprod, i8mm, sve, sve2 or native.'
        } else {
            $other, $takes = 'sve', 'it takes x86-64, x86-64-v2, x86-64-v3, x86-64-v4 or native.'
        }
        $out = @(Invoke-Capped $other '')
        @(Get-Verdict $out)[0] | Should -BeLike "REFUSED *PWRS_CPU_MAX is '$other'; $takes*"
    }
}

Describe 'What hand-written C# reads through Pwrs.Cpu' {
    It 'returns what the Rust side returns: the level or tier, the cap, and each extension' {
        $rust = @(Get-RustCpu)
        $cap = if ($env:PWRS_CPU_MAX -and $env:PWRS_CPU_MAX.Trim()) { $env:PWRS_CPU_MAX.Trim() } else { 'native' }
        $r = Get-RustHybridCpu -Extension $rust.Name
        foreach ($x in $rust) {
            $c = @($r.Extensions | Where-Object Name -eq $x.Name)[0]
            $c.Has | Should -Be $x.Usable -Because "Rust says $($x.Name) is usable: $($x.Usable)"
            $c.Detected | Should -Be $x.Detected -Because "Rust says $($x.Name) is detected: $($x.Detected)"
        }
        # The level pwrs::cpu::level, or the tier pwrs::cpu::arm::tier,
        # reaches: each above the lowest whose extensions are all usable,
        # in order, until one is not.
        $reached = $caps[0]
        foreach ($l in $caps[1..($caps.Count - 1)]) {
            if (@($rust | Where-Object { $_.Level -eq $l -and -not $_.Usable }).Count) { break }
            $reached = $l
        }
        if ($arm64) {
            $r.ArmTier | Should -Be $reached
            $r.Level | Should -BeNullOrEmpty
        } else {
            $r.Level | Should -Be $reached
            $r.ArmTier | Should -BeNullOrEmpty
        }
        $r.Cap | Should -Be $cap
        { Get-RustHybridCpu -Extension 'not-an-extension' -ErrorAction Stop } | Should -Throw -ExpectedMessage "*'not-an-extension' is not an extension pwrs::cpu knows on this architecture*"
    }
}

Describe 'The AArch64 import check, given stand-in system calls' {
    BeforeAll {
        # The runtime's CpuCheck.ArmRefusal checks an AArch64 requirement
        # list against stand-in functions in place of the operating
        # system's calls, so each system's reports are checked here on any
        # machine. The entries are pwrs::cpu's own, every feature in them.
        # The runtime is reached through a cmdlet's base type: on
        # PowerShell 7 it is loaded in this module's own load context,
        # which a type literal does not search.
        $runtime = (Get-Command Get-RustCpu).ImplementingType.BaseType.Assembly
        $script:check = $runtime.GetType('Pwrs.CpuCheck', $true)
        $script:flags = [Reflection.BindingFlags]'NonPublic, Static'
        $script:refusal = $check.GetMethod('ArmRefusal', $flags)
        $script:entries = @{}
        foreach ($s in 'linux', 'windows', 'macos') { $entries[$s] = Get-RustArmRequirement -System $s }

        # The entries of a system's list whose fields pass $keep, a script
        # block given the name, the tier as a number and the report.
        function script:Select-Entry([string]$system, [scriptblock]$keep) {
            @($entries[$system] -split ' ' | Where-Object {
                    $name, $tier, $report = $_ -split ','
                    & $keep $name ([int]$tier) $report
                })
        }

        # What the check returns for the entries, for the list's system $listed
        # judged on the system $running; null when it lets the library in.
        function script:Get-ArmRefusal([string]$listed, [string[]]$list, [int]$cap, [string]$capName, [string]$running,
            [Func[uint64, uint64]]$auxval, [Func[uint32, bool]]$present, [Func[string, bool]]$sysctl) {
            $refusal.Invoke($null, @("PWRS-ARM/1 $listed $($list -join ' ')", 'hello.so', $cap, $capName, $running, $auxval, $present, $sysctl))
        }

        $script:noCap = 6
        $script:allBits = [Func[uint64, uint64]] { param($type) [uint64]::MaxValue }
        $script:noBits = [Func[uint64, uint64]] { param($type) [uint64]0 }
        $script:every = [Func[uint32, bool]] { param($n) $true }
        $script:everyName = [Func[string, bool]] { param($n) $true }
        $script:nothing = [Func[uint32, bool]] { param($n) $false }
        $script:noName = [Func[string, bool]] { param($n) $false }
    }

    It 'lets a library in when Linux reports every feature, and names each one beyond the baseline Linux does not report' {
        $all = Select-Entry linux { $true }
        Get-ArmRefusal linux $all $noCap $null linux $allBits $nothing $noName | Should -BeNullOrEmpty
        $names = Select-Entry linux { param($name, $tier) $tier -ne 1 } | ForEach-Object { ($_ -split ',')[0] }
        $names | Should -Not -Contain 'neon'
        $refused = Get-ArmRefusal linux $all $noCap $null linux $noBits $nothing $noName
        $refused | Should -BeLike "hello.so was compiled for AArch64 features it cannot use here: this CPU does not offer $($names -join ', ').*"
    }

    It 'never reads what the target compiles every crate with, which the list puts at neon''s tier' {
        # Linux's neon with no bits reported, and macOS's baseline with no
        # name reading nonzero, as a macOS without hw.optional.AdvSIMD reads.
        $neon = Select-Entry linux { param($name) $name -eq 'neon' }
        Get-ArmRefusal linux $neon $noCap $null linux $noBits $nothing $noName | Should -BeNullOrEmpty
        $baseline = Select-Entry macos { param($name, $tier) $tier -eq 1 }
        $baseline.Count | Should -Be 15
        Get-ArmRefusal macos $baseline $noCap $null macos $noBits $nothing $noName | Should -BeNullOrEmpty
        $i8mm = Select-Entry macos { param($name) $name -eq 'i8mm' }
        Get-ArmRefusal macos ($baseline + $i8mm) $noCap $null macos $noBits $nothing $noName | Should -BeLike '*this CPU does not offer i8mm.*'
    }

    It 'requires Linux''s half-precision floats to come with their Advanced SIMD form for aes' {
        $aes = Select-Entry linux { param($name) $name -eq 'aes' }
        # fp, asimd, aes and fphp without asimdhp, then with it.
        $without = [Func[uint64, uint64]] { param($type) if ($type -eq 16) { [uint64]0x20B } else { [uint64]0 } }
        $with = [Func[uint64, uint64]] { param($type) if ($type -eq 16) { [uint64]0x60B } else { [uint64]0 } }
        Get-ArmRefusal linux $aes $noCap $null linux $without $nothing $noName | Should -BeLike '*this CPU does not offer aes.*'
        Get-ArmRefusal linux $aes $noCap $null linux $with $nothing $noName | Should -BeNullOrEmpty
    }

    It 'refuses on Windows exactly the features Windows has no constant for, and needs every constant a feature names' {
        $all = Select-Entry windows { $true }
        $unreported = Select-Entry windows { param($name, $tier, $report) -not $report } | ForEach-Object { ($_ -split ',')[0] }
        $unreported | Should -Be @('sm4', 'fhm', 'fcma', 'frintts', 'dit', 'rand')
        Get-ArmRefusal windows $all $noCap $null windows $allBits $every $noName |
            Should -BeLike "*: Windows has no way to report $($unreported -join ', '), so they cannot be confirmed.*"
        $sha3 = Select-Entry windows { param($name) $name -eq 'sha3' }
        $only64 = [Func[uint32, bool]] { param($n) $n -ne 65 }
        Get-ArmRefusal windows $sha3 $noCap $null windows $allBits $only64 $noName | Should -BeLike '*this CPU does not offer sha3.*'
        Get-ArmRefusal windows $sha3 $noCap $null windows $allBits $every $noName | Should -BeNullOrEmpty
    }

    It 'refuses on macOS exactly the features macOS has no sysctl name for, and takes either of crc''s names' {
        $all = Select-Entry macos { $true }
        $unreported = Select-Entry macos { param($name, $tier, $report) -not $report } | ForEach-Object { ($_ -split ',')[0] }
        $unreported | Should -Contain 'sve'
        $unreported | Should -Not -Contain 'crc'
        Get-ArmRefusal macos $all $noCap $null macos $allBits $nothing $everyName |
            Should -BeLike "*: macOS has no way to report $($unreported -join ', '), so they cannot be confirmed.*"
        # crc is in macOS's baseline, which the check does not read, so its
        # entry is given its own tier, Beyond, for the check to read its two
        # names.
        $crc = Select-Entry macos { param($name) $name -eq 'crc' }
        Get-ArmRefusal macos $crc $noCap $null macos $allBits $nothing $noName | Should -BeNullOrEmpty
        $crc = $crc -replace '^crc,1,', 'crc,6,'
        $newName = [Func[string, bool]] { param($n) $n -eq 'hw.optional.arm.FEAT_CRC32' }
        $oldName = [Func[string, bool]] { param($n) $n -eq 'hw.optional.armv8_crc32' }
        Get-ArmRefusal macos $crc $noCap $null macos $allBits $nothing $newName | Should -BeNullOrEmpty
        Get-ArmRefusal macos $crc $noCap $null macos $allBits $nothing $oldName | Should -BeNullOrEmpty
        Get-ArmRefusal macos $crc $noCap $null macos $allBits $nothing $noName | Should -BeLike '*this CPU does not offer crc.*'
    }

    It 'withholds what PWRS_CPU_MAX caps away, but never what the target compiles every crate with' {
        $all = Select-Entry linux { $true }
        $above = Select-Entry linux { param($name, $tier) $tier -gt 4 } | ForEach-Object { ($_ -split ',')[0] }
        Get-ArmRefusal linux $all 4 'sve' linux $allBits $nothing $noName | Should -BeLike "*: PWRS_CPU_MAX=sve withholds $($above -join ', ').*"
        # macOS's own baseline sits at neon's tier in the list, so a neon
        # cap leaves dotprod and crc alone and takes i8mm.
        $macos = Select-Entry macos { param($name) $name -in 'dotprod', 'crc', 'i8mm' }
        Get-ArmRefusal macos $macos 1 'neon' macos $allBits $nothing $everyName | Should -BeLike '*: PWRS_CPU_MAX=neon withholds i8mm.*'
    }

    It 'leaves a list for another system to that system, and names an entry it cannot read' {
        Get-ArmRefusal linux (Select-Entry linux { $true }) $noCap $null windows $noBits $nothing $noName | Should -BeNullOrEmpty
        Get-ArmRefusal linux @('aes,6,3:x:1') $noCap $null linux $allBits $nothing $noName | Should -BeLike '*: its list has entries this runtime cannot read: aes,6,3:x:1.*'
    }

    # The platform decides the branch, not $IsLinux: the Pester shim the
    # FreeBSD gate runs under sets a global $IsLinux there.
    It 'makes the check''s own call to this operating system' {
        if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Linux)) {
            $check.GetMethod('AuxVal', $flags).Invoke($null, @([uint64]16)) | Should -Not -Be 0
        } elseif ($IsMacOS) {
            # AES's name, which every Apple silicon Mac reads nonzero and an
            # Intel Mac does not have.
            $check.GetMethod('SysctlReadsNonzero', $flags).Invoke($null, @('hw.optional.arm.FEAT_AES')) | Should -Be $arm64
        } elseif ($IsWindows -or $PSVersionTable.PSEdition -eq 'Desktop') {
            # The constant for sse2 on x64 Windows and the one for neon on
            # Arm64 Windows: every such CPU has the feature.
            $feature = if ($arm64) { [uint32]19 } else { [uint32]10 }
            $check.GetMethod('FeaturePresent', $flags).Invoke($null, @($feature)) | Should -BeTrue
        } else {
            Set-ItResult -Skipped -Because 'no AArch64 list names this operating system, so the check makes no call to it'
        }
    }
}
