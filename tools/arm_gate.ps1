#Requires -Version 7
# Builds PoWerRuSt for 32-bit ARM each way it builds, and runs its tests under
# qemu-arm, whose `max` CPU has NEON and the CRC instructions and flushes
# denormals as NEON does. AArch64's tests run under qemu-aarch64, since the
# NEON bodies are one source for both. Both links use rust-lld, so the machine
# needs no ARM C library. The emulators give each body's results, not its
# speed.
#
# It works on a copy of the tree, without PoWerRuSt's dev-dependencies and
# benches: criterion's dependencies compile C, and none of this runs them.
#
# Needs a Linux x86-64 machine with qemu-user and, from rustup, the targets
# armv7-unknown-linux-musleabihf, armv7-unknown-linux-gnueabi and
# aarch64-unknown-linux-musl.
#
#   pwsh -File tools/arm_gate.ps1
param(
    [string] $Work = (Join-Path ([System.IO.Path]::GetTempPath()) "pwrs-arm-gate-$PID")
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$arm = 'armv7-unknown-linux-musleabihf'
$softFloat = 'armv7-unknown-linux-gnueabi'
$aarch64 = 'aarch64-unknown-linux-musl'
$env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_RUNNER = 'qemu-arm -cpu max'
$env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER = 'rust-lld'
$env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_RUSTFLAGS = '-C link-self-contained=yes'
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUNNER = 'qemu-aarch64 -cpu max'
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = 'rust-lld'
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS = '-C link-self-contained=yes'
$env:RUSTFLAGS = $null

$tree = Join-Path $Work 'tree'
New-Item -ItemType Directory -Path $tree | Out-Null
foreach ($part in 'Cargo.toml', 'Cargo.lock', 'crates', 'examples') {
    Copy-Item -LiteralPath (Join-Path $root $part) -Destination $tree -Recurse
}

$manifest = Join-Path $tree 'crates/pwrs/Cargo.toml'
$keep = $true
$kept = foreach ($line in Get-Content -LiteralPath $manifest) {
    if ($line -match '^\[') { $keep = $line -notmatch '^\[(dev-dependencies|\[bench\])' }
    if ($keep) { $line }
}
Set-Content -LiteralPath $manifest -Value $kept

$failed = 0

# Runs one cargo command in the copy, with the settings given, and reports
# its row. `-Refused` names the error of a build that must stop with it.
function Invoke-Row([string] $Name, [string[]] $Arguments, [hashtable] $Settings = @{}, [string] $Refused, [scriptblock] $Also) {
    $before = @{}
    foreach ($key in $Settings.Keys) {
        $before[$key] = [Environment]::GetEnvironmentVariable($key)
        [Environment]::SetEnvironmentVariable($key, $Settings[$key])
    }
    Push-Location $tree
    try {
        $out = @(cargo @Arguments 2>&1 | ForEach-Object { "$_" })
        $code = $LASTEXITCODE
    } finally {
        Pop-Location
        foreach ($key in $Settings.Keys) { [Environment]::SetEnvironmentVariable($key, $before[$key]) }
    }
    $warnings = @($out | Where-Object { $_ -match '^warning: [^`]' })
    $results = @($out | Where-Object { $_ -match '^test result' })
    $ok = if ($Refused) { $code -ne 0 -and @($out | Where-Object { $_ -match $Refused }).Count -gt 0 } else { $code -eq 0 }
    if ($ok -and $Also) { $ok = & $Also $out }
    if (-not $ok) { $script:failed++ }
    "$(if ($ok) { 'ok' } else { 'FAIL' }) $Name exit=$code warnings=$($warnings.Count)"
    $results | ForEach-Object { "  $_" }
    if (-not $ok) { $out | Where-Object { $_ -match '^(error|warning)|panicked|FAILED' } | Select-Object -First 20 | ForEach-Object { "  $_" } }
}

$bootstrap = @{ RUSTC_BOOTSTRAP = '1' }
$neon = @('--target', $arm, '--features', 'arm-neon')
$base = @('--profile', 'test-fast', '-p', 'PoWerRuSt', '--lib')
$soft = @('--target', $softFloat, '--features', 'arm-neon')
Invoke-Row 'arm stable, no feature' (@('check') + $base + @('--target', $arm))
Invoke-Row 'arm stable, the feature refused' (@('check') + $base + $neon) -Refused '^error\[E0554\]'
Invoke-Row 'arm with the feature' (@('check') + $base + $neon) -Settings $bootstrap
Invoke-Row 'arm soft-float, the feature refused' (@('check') + $base + $soft) -Settings $bootstrap -Refused 'needs floating-point registers'
Invoke-Row 'arm soft-float with an FPU' (@('build') + $base + $soft) -Settings @{ RUSTC_BOOTSTRAP = '1'; CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_RUSTFLAGS = '-C target-feature=-soft-float,+vfp3,+d32,+neon' }
Invoke-Row 'arm tests with the feature' (@('test') + $base + $neon) -Settings $bootstrap
Invoke-Row 'arm kernels, every body run' (@('test') + $base + $neon + @('simd', '--', '--nocapture')) -Settings $bootstrap -Also {
    param($out)
    $skipped = @($out | Where-Object { $_ -match ' SKIPPED ' })
    $running = @($out | Where-Object { $_ -match ' RUNNING ' })
    $skipped | ForEach-Object { "  $_" } | Write-Host
    "  $($running.Count) bodies run, $($skipped.Count) skipped" | Write-Host
    $running.Count -gt 0 -and $skipped.Count -eq 0
}
Invoke-Row 'arm kernels capped to scalar' (@('test') + $base + $neon + @('simd')) -Settings @{ RUSTC_BOOTSTRAP = '1'; PWRS_CPU_MAX = 'scalar' }
Invoke-Row 'arm kernels, NEON in every function' (@('test') + $base + $neon + @('simd')) -Settings @{ RUSTC_BOOTSTRAP = '1'; CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_RUSTFLAGS = '-C link-self-contained=yes -C target-feature=+neon' }
Invoke-Row 'arm kernels, no feature' (@('test') + $base + @('--target', $arm, 'simd'))
Invoke-Row 'aarch64 kernels' (@('test') + $base + @('--target', $aarch64, 'simd'))
$lint = @('--no-deps', '--tests', '--', '-D', 'warnings')
Invoke-Row 'arm clippy' (@('clippy') + $base + $neon + $lint) -Settings $bootstrap
Invoke-Row 'aarch64 clippy' (@('clippy') + $base + @('--target', $aarch64) + $lint)

Remove-Item -LiteralPath $Work -Recurse -Force
"arm-verdict $(if ($failed -eq 0) { 'pass' } else { "fail ($failed)" })"
exit $failed
