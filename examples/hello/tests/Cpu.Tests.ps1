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

    It 'lists every extension the machine is asked about' {
        $all = @(Get-RustCpu)
        $all.Count | Should -BeGreaterThan 40
        @($all | Where-Object Compiled).Count | Should -Be $compiled.Count
        ($all | Where-Object Name -eq 'avx512f').Level | Should -Be 'x86-64-v4'
    }
}

Describe 'PWRS_CPU_MAX at import' {
    It 'refuses the import when the cap withholds an extension the library was compiled for, and allows it otherwise' {
        $withheld = @($compiled | Where-Object Level -ne 'x86-64')
        $out = @(Invoke-Capped 'x86-64' '')
        $verdict = @(Get-Verdict $out)
        if ($withheld.Count -gt 0) {
            $verdict[0] | Should -BeLike 'REFUSED *'
            $verdict[0] | Should -BeLike '*PWRS_CPU_MAX=x86-64 withholds*'
            $verdict[0] | Should -BeLike "*$($withheld[0].Name)*"
            @(Get-CalcWarning $out | Where-Object { $_ -like '*PWRS_CPU_MAX=x86-64 withholds*' }).Count | Should -Be 1 -Because ($out -join "`n")
        } else {
            $verdict[0] | Should -Be 'IMPORTED'
            @(Get-CalcWarning $out).Count | Should -Be 0 -Because ($out -join "`n")
        }
    }

    It 'caps what a kernel may use, or refuses when the library needs more' {
        $aboveV3 = @($compiled | Where-Object { $_.Level -in 'x86-64-v4', 'native' })
        $out = @(Invoke-Capped 'x86-64-v3' "(Get-RustCpu | Where-Object Name -in 'avx2', 'avx512f' | ForEach-Object { `$_.Name + '=' + `$_.Usable }) -join ' '")
        $verdict = @(Get-Verdict $out)
        if ($aboveV3.Count -gt 0) {
            $verdict[0] | Should -BeLike 'REFUSED *'
            @(Get-CalcWarning $out).Count | Should -Be 1 -Because ($out -join "`n")
        } else {
            $verdict[0] | Should -Be 'IMPORTED'
            $avx2 = (Get-RustCpu | Where-Object Name -eq 'avx2').Detected
            $verdict[1] | Should -Be ("avx2=$avx2 avx512f=False")
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
}
