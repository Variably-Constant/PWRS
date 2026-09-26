# Checks that two modules imported into one session each make their
# own objects, whichever of them was imported first.
#
# A class id is a class's position in its own module's export list,
# so every module numbers its classes from 0, and each module's
# objects have to come from that module's own factories. The subjects
# are one source, coload/src/lib.rs, built twice: as Alpha, and with
# every Alpha replaced by Beta, as Beta. Both declare the same classes
# in the same order, so an object made through the other module's
# factories comes back as the other module's type over this module's
# values: a wrong type, never a read with the wrong layout. A proxy of
# the wrong type is neither called nor disposed, and its finalizer is
# suppressed, so no module frees another's value.
#
# Each order runs in a child process of the host running this script,
# because a module stays loaded in the process that imported it. The
# copies are built under target/coload-gate, so nothing the repository
# tracks is edited.
[CmdletBinding(DefaultParameterSetName = 'Gate')]
param(
    # The manifest imported first. With -Second, this process imports
    # the two in that order and checks both.
    [Parameter(Mandatory, ParameterSetName = 'Order')] [string] $First,
    # The manifest imported second.
    [Parameter(Mandatory, ParameterSetName = 'Order')] [string] $Second,
    # A triple for `cargo pwrs build --target`, as in
    # hot_reload_gate.ps1. Empty builds for the building machine.
    [Parameter(ParameterSetName = 'Gate')] [string] $Target = ''
)

$ErrorActionPreference = 'Stop'

if ($PSCmdlet.ParameterSetName -eq 'Order') {
    $failures = 0

    function Check($what, $expected, $actual) {
        if ($expected -eq $actual) {
            "  ok   {0}: {1}" -f $what, $actual
        } else {
            "  FAIL {0}: expected '{1}', got '{2}'" -f $what, $expected, $actual
            $script:failures++
        }
    }

    # What a call answered, or what it threw, as text to compare.
    function Answer([scriptblock] $call) {
        try {
            "$(& $call)"
        } catch {
            "threw $($_.Exception.GetType().Name): $($_.Exception.Message)"
        }
    }

    # An object's type and the shell declaring it, without the shell's
    # build stamp: Alpha.Counter from Alpha.Shell.
    function Made($object) {
        if ($null -eq $object) { return 'nothing' }
        $type = $object.psobject.BaseObject.GetType()
        $shell = ($type.Assembly.GetName().Name -split '\.Shell\.')[0] + '.Shell'
        "$($type.FullName) from $shell"
    }

    # A proxy made through the other module's factories holds this
    # module's value behind the other module's library. It is never
    # called, and its finalizer, which would free the value through
    # that library, is suppressed.
    function Quarantine($object) {
        if ($null -eq $object) { return }
        $base = $object.psobject.BaseObject
        if ($base -is [IDisposable]) { [GC]::SuppressFinalize($base) }
    }

    Import-Module $First -Force
    Import-Module $Second -Force
    $names = @($First, $Second) | ForEach-Object { [IO.Path]::GetFileNameWithoutExtension($_) }
    "order: $($names -join ' then ') on $($PSVersionTable.PSEdition) $($PSVersionTable.PSVersion)"

    foreach ($module in $names) {
        # A copied class, and the same object read back into Rust.
        $record = & "Get-$($module)Record" -Name 'n' -Value 7
        Check "$module copied class" "$module.Record from $module.Shell" (Made $record)
        Check "$module copied values" "$module n 7" "$($record.Origin) $($record.Name) $($record.Value)"
        Check "$module copied round trip" "$($module):n:7" (Answer { & "Test-$($module)Record" -Record $record })

        # A proxy class: a method, a proxy and a copied object that
        # methods return, the object read back into Rust, and Dispose.
        $counter = & "New-$($module)Counter" -Value 10
        $made = Made $counter
        Check "$module proxy class" "$module.Counter from $module.Shell" $made
        if ($made -eq "$module.Counter from $module.Shell") {
            Check "$module proxy method" '15' (Answer { $counter.Add(5) })
            $half = $counter.Split()
            $halfMade = Made $half
            Check "$module proxy a method returned" "$module.Counter from $module.Shell" $halfMade
            if ($halfMade -eq "$module.Counter from $module.Shell") {
                Check "$module proxy values after the split" '8 7' "$($counter.Value) $($half.Value)"
                $half.Dispose()
                Check "$module returned proxy disposed" 'True' "$($half.IsDisposed)"
            } else {
                Quarantine $half
            }
            $snapshot = $counter.Snapshot()
            Check "$module copied class a method returned" "$module.Record from $module.Shell" (Made $snapshot)
            Check "$module copied values a method returned" "$module snapshot 8" "$($snapshot.Origin) $($snapshot.Name) $($snapshot.Value)"
            Check "$module proxy round trip" "$module=8" (Answer { & "Test-$($module)Counter" -Counter $counter })
            $counter.Dispose()
            Check "$module proxy disposed" 'True' "$($counter.IsDisposed)"
        } else {
            Quarantine $counter
        }

        # An enum, and the value read back into Rust.
        $color = & "Get-$($module)Color" -Value 2
        Check "$module enum" "$module.Color from $module.Shell" (Made $color)
        Check "$module enum value" 'Blue 2' "$color $([long]$color)"
        Check "$module enum round trip" "$($module):Blue" (Answer { & "Test-$($module)Color" -Color $color })
    }

    if ($failures -gt 0) {
        "order-verdict fail ($failures of the checks above)"
    } else {
        'order-verdict pass'
    }
    exit $failures
}

$root = Split-Path -Parent $PSScriptRoot
$gate = Join-Path $root 'target/coload-gate'
$fixture = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'coload/src/lib.rs'))
$pwrsPath = (Join-Path $root 'crates/pwrs') -replace '\\', '/'

# Out of the workspace's own tree, so the gate's builds never
# invalidate a developer's.
$env:CARGO_TARGET_DIR = (Join-Path $gate 'target')

# Writes the fixture as the named module, with a manifest of its own
# so it does not need the workspace, builds it, and answers the path
# of its module manifest.
function Build($name) {
    $crate = Join-Path $gate $name.ToLowerInvariant()
    $src = Join-Path $crate 'src'
    $null = New-Item -ItemType Directory -Force -Path $src
    $text = $fixture.Replace('Alpha', $name)
    if ($name -ne 'Alpha' -and $text -eq $fixture) { throw "naming the fixture $name changed nothing" }
    [IO.File]::WriteAllText((Join-Path $src 'lib.rs'), $text)
    $manifest = @"
[workspace]

[package]
name = "pwrs-coload-$($name.ToLowerInvariant())"
version = "0.0.0"
edition = "2021"
description = "The $name module of the co-load gate."
authors = ["pwrs"]
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
pwrs = { package = "PoWerRuSt", path = "$pwrsPath" }
"@
    [IO.File]::WriteAllText((Join-Path $crate 'Cargo.toml'), $manifest)

    # A native command writing to stderr is a terminating error under
    # `Stop` in Windows PowerShell, and cargo reports its progress
    # there, so the preference is lifted for the call and the exit
    # code is what decides.
    $keep = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    [string[]] $targetArgs = @()
    if ($Target) { $targetArgs = @('--target', $Target) }
    try {
        $out = & cargo run --profile test-fast --manifest-path (Join-Path $root 'Cargo.toml') -p cargo-pwrs -- pwrs build --release @targetArgs --manifest-dir $crate 2>&1
    } finally {
        $ErrorActionPreference = $keep
    }
    if ($LASTEXITCODE -ne 0) {
        $out | ForEach-Object { "$_" }
        throw "$name build failed with $LASTEXITCODE"
    }
    Join-Path $gate "target/pwrs/$name/$name.psd1"
}

"co-load gate on $($PSVersionTable.PSEdition) $($PSVersionTable.PSVersion)"

$alpha = Build 'Alpha'
$beta = Build 'Beta'

# This host's own executable, so each order runs in the edition under
# test.
$exe = (Get-Process -Id $PID).Path
$failed = New-Object System.Collections.ArrayList
foreach ($order in @(@($alpha, $beta), @($beta, $alpha))) {
    $keep = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = & $exe -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -First $order[0] -Second $order[1] 2>&1
    } finally {
        $ErrorActionPreference = $keep
    }
    $code = $LASTEXITCODE
    $out | ForEach-Object { "$_" }
    if ($code -ne 0) {
        $label = ($order | ForEach-Object { [IO.Path]::GetFileNameWithoutExtension($_) }) -join ' then '
        $null = $failed.Add("$label exited $code")
    }
}

if ($failed.Count -gt 0) {
    "coload-verdict fail ($($failed -join '; '))"
    exit 1
}
'coload-verdict pass'
exit 0
