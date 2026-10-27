#Requires -Version 7
# Packages each example as `cargo pwrs publish --release --dry-run` does,
# with this checkout's cargo-pwrs, and checks what it made. The command
# must exit 0 and print one `packaged` line. The package must have every
# file of the module folder and nothing else, apart from the parts every
# NuGet package has: [Content_Types].xml, _rels/, package/ and the
# module's .nuspec.
#
# A published module must load on every CPU its target runs on, and
# publish refuses a library compiled for instructions beyond the target's
# baseline. So the builds here are for that baseline, set in RUSTFLAGS,
# whatever target-cpu the machine's cargo configuration names. That
# changes every build's flags, so the gates run this after the steps that
# reuse their builds.
#
#   pwsh -File tools/publish_gate.ps1 [-Examples hello,file-facts] [-Target <triple>]
param(
    [string[]] $Examples = @('hello', 'file-facts'),
    [string] $Target
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Add-Type -AssemblyName System.IO.Compression.FileSystem

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
$baseline = switch ($arch) {
    'X64' { 'x86-64' }
    'Arm64' { 'generic' }
    default { throw "no baseline CPU is known for $arch" }
}
$env:RUSTFLAGS = "-C target-cpu=$baseline"
"publish gate on $arch, RUSTFLAGS=$env:RUSTFLAGS"

# Runs cargo in the checkout and returns its exit code, its output lines
# and its error lines, each read as plain text.
function Invoke-Cargo([string[]] $Arguments) {
    $psi = [System.Diagnostics.ProcessStartInfo]::new('cargo')
    foreach ($a in $Arguments) { $psi.ArgumentList.Add($a) }
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.WorkingDirectory = $root
    $p = [System.Diagnostics.Process]::Start($psi)
    $outTask = $p.StandardOutput.ReadToEndAsync()
    $errTask = $p.StandardError.ReadToEndAsync()
    $p.WaitForExit()
    [pscustomobject]@{ Code = $p.ExitCode; Out = @($outTask.Result -split "`r?`n"); Err = @($errTask.Result -split "`r?`n") }
}

$failed = 0
foreach ($example in $Examples) {
    $arguments = @('run', '--profile', 'test-fast', '-p', 'cargo-pwrs', '--', 'pwrs', 'publish', '--release', '--dry-run', '--manifest-dir', "examples/$example")
    if ($Target) { $arguments += @('--target', $Target) }
    $run = Invoke-Cargo $arguments
    $packaged = @($run.Out | Where-Object { $_.StartsWith('packaged ') } | ForEach-Object { $_.Substring('packaged '.Length) })
    if ($run.Code -ne 0 -or $packaged.Count -ne 1) {
        "FAIL $example exit=$($run.Code) packaged=$($packaged.Count)"
        @($run.Out) + @($run.Err) | Where-Object { $_ } | Select-Object -Last 15 | ForEach-Object { "  $_" }
        $failed++
        continue
    }

    # The package is target/pwrs/publish/<Name>/<Name>.<version>.nupkg,
    # and the folder it was made from target/pwrs/<Name>.
    $nupkg = $packaged[0]
    $outDir = Split-Path -Parent $nupkg
    $name = Split-Path -Leaf $outDir
    $module = Join-Path (Split-Path -Parent (Split-Path -Parent $outDir)) $name

    # A package's entry names are escaped as URI paths, so a space is %20.
    $zip = [System.IO.Compression.ZipFile]::OpenRead($nupkg)
    try {
        $entries = @($zip.Entries | Where-Object { -not $_.FullName.EndsWith('/') } | ForEach-Object { [Uri]::UnescapeDataString($_.FullName) })
    } finally {
        $zip.Dispose()
    }
    $nuget = @($entries | Where-Object { $_ -eq '[Content_Types].xml' -or $_.StartsWith('_rels/') -or $_.StartsWith('package/') -or $_ -eq "$name.nuspec" })
    $inPackage = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($e in $entries) { if ($nuget -notcontains $e) { $null = $inPackage.Add($e) } }
    $inFolder = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($f in Get-ChildItem -LiteralPath $module -Recurse -File) { $null = $inFolder.Add($f.FullName.Substring($module.Length + 1).Replace('\', '/')) }
    $missing = @($inFolder | Where-Object { -not $inPackage.Contains($_) } | Sort-Object)
    $extra = @($inPackage | Where-Object { -not $inFolder.Contains($_) } | Sort-Object)

    $verdict = if ($missing.Count -eq 0 -and $extra.Count -eq 0) { 'ok' } else { 'FAIL'; $failed++ }
    "$verdict $example $(Split-Path -Leaf $nupkg) $((Get-Item -LiteralPath $nupkg).Length) bytes: $($inPackage.Count) module files of $($inFolder.Count) in the folder, $($nuget.Count) NuGet parts, missing $($missing.Count), extra $($extra.Count)"
    $missing | Select-Object -First 10 | ForEach-Object { "  missing $_" }
    $extra | Select-Object -First 10 | ForEach-Object { "  extra $_" }
}
"publish-verdict $(if ($failed -eq 0) { 'pass' } else { "fail ($failed)" })"
exit $failed
