# Checks that a rebuilt module takes its cmdlets over in a session
# that has already run the previous build.
#
# Three builds of one module in one host process:
#   gen0  the baseline
#   gen1  a new parameter, so the surface changes. The shell assembly
#         must take a new name and the new parameter must bind, which
#         is what fails when two loads share one assembly identity.
#   gen2  the same surface with a different body. The shell assembly
#         must keep its name, so the session keeps its types and only
#         the native library is swapped.
#
# Every build runs while the module is imported, which is also what
# proves the staged copies leave the build's own folder writable.
#
# The subject is a copy of the hello example, so nothing the
# repository tracks is edited.
[CmdletBinding()]
param(
    # A triple for `cargo pwrs build --target`, such as
    # x86_64-unknown-linux-gnu.2.35 when the host running this script is
    # a pwsh on an older glibc than the building machine's. Empty builds
    # for the building machine.
    [string] $Target = ''
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$gate = Join-Path $root 'target/reload-gate'
$crate = Join-Path $gate 'hello'
$psd1 = Join-Path $gate 'target/pwrs/Hello/Hello.psd1'
$lib = Join-Path $crate 'src/lib.rs'

$failures = New-Object System.Collections.ArrayList

function Check($what, $expected, $actual) {
    if ($expected -eq $actual) {
        "  ok   {0}: {1}" -f $what, $actual
    } else {
        "  FAIL {0}: expected '{1}', got '{2}'" -f $what, $expected, $actual
        $null = $failures.Add($what)
    }
}

function Build($label) {
    # A native command writing to stderr is a terminating error under
    # `Stop` in Windows PowerShell, and cargo reports its progress
    # there, so the preference is lifted for the call and the exit
    # code is what decides.
    $keep = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    [string[]] $targetArgs = @()
    if ($Target) { $targetArgs = @('--target', $Target) }
    try {
        $out = & cargo run --profile test-fast -p cargo-pwrs -- pwrs build --release @targetArgs --manifest-dir $crate 2>&1
    } finally {
        $ErrorActionPreference = $keep
    }
    if ($LASTEXITCODE -ne 0) {
        $out | ForEach-Object { "$_" }
        throw "$label build failed with $LASTEXITCODE"
    }
}

function ShellName() {
    Split-Path -Leaf (Get-Command Get-Greeting).ImplementingType.Assembly.Location
}

function HasLoud() {
    [bool]((Get-Command Get-Greeting).Parameters.ContainsKey('Loud'))
}

# The shells declaring a copied object, a proxy and an enum value made
# by the loaded module, each named once. Every factory a generation
# reaches is its own shell's when this is that shell alone.
function ClassShells() {
    $counter = New-Counter -Label gate
    $made = @((Get-Person -Name gate), $counter, (ConvertTo-RustSignal -Value 5))
    $counter.Dispose()
    ($made | ForEach-Object { Split-Path -Leaf $_.psobject.BaseObject.GetType().Assembly.Location } | Sort-Object -Unique) -join ' '
}

# The subject: a copy of the hello example, with a manifest of its own
# so it does not need the workspace it came out of.
if (Test-Path $crate) { Remove-Item $crate -Recurse -Force }
$null = New-Item -ItemType Directory -Force -Path $crate
Copy-Item (Join-Path $root 'examples/hello/src') -Destination $crate -Recurse
# A copy keeps the write time it came from, and cargo decides what to
# rebuild by comparing write times against the artifacts it already
# has. A previous run of this gate leaves artifacts newer than the
# copy, and without this the baseline build is the previous run's last
# generation rather than a baseline.
$now = Get-Date
Get-ChildItem $crate -Recurse -File | ForEach-Object { $_.LastWriteTime = $now }
$pwrsPath = (Join-Path $root 'crates/pwrs') -replace '\\', '/'
@"
[workspace]

[package]
name = "pwrs-example-hello"
version = "0.0.0"
edition = "2021"
description = "A copy of the pwrs example module, rebuilt by the hot reload gate."
authors = ["pwrs"]
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
pwrs = { package = "PoWerRuSt", path = "$pwrsPath" }
"@ | Set-Content -Path (Join-Path $crate 'Cargo.toml') -Encoding ascii

# Out of the workspace's own tree, so the gate's builds never
# invalidate a developer's.
$env:CARGO_TARGET_DIR = (Join-Path $gate 'target')

$original = [IO.File]::ReadAllText($lib)
$nl = if ($original -match "`r`n") { "`r`n" } else { "`n" }

"hot reload gate on $($PSVersionTable.PSEdition) $($PSVersionTable.PSVersion)"

Build 'gen0'
Import-Module $psd1 -Force
$shell0 = ShellName
"gen0 $shell0"
Check 'gen0 greeting' 'Hello, World!' (Get-Greeting -Name World)
Check 'gen0 has no -Loud' $false (HasLoud)
Check 'gen0 makes its objects from its own shell' $shell0 (ClassShells)

# gen1: a new parameter, and a body that uses it.
#
# Each pattern ends with a lookahead rather than `$` alone, because `$`
# in multiline mode matches before the newline and not before the
# carriage return, so a checkout with CRLF endings leaves one in the
# way. The lookahead does not consume it either, so the line keeps the
# ending it had.
$patched = $original -replace '(?m)^    pub panic: bool,(?=\r?$)', `
    "    pub panic: bool,$nl    /// Shout the greeting.$nl    #[param]$nl    pub loud: bool,"
if ($patched -eq $original) { throw 'the -Loud patch matched nothing' }
$gen1 = $patched -replace '(?m)^            ps\.write\(format!\("Hello, \{\}!", self\.name\)\)\?;(?=\r?$)', `
    '            ps.write(if self.loud { format!("HELLO, {}!", self.name.to_uppercase()) } else { format!("Hello, {}!", self.name) })?;'
if ($gen1 -eq $patched) { throw 'the loud-body patch matched nothing' }
[IO.File]::WriteAllText($lib, $gen1)

Build 'gen1'
Import-Module $psd1 -Force
$shell1 = ShellName
"gen1 $shell1"
Check 'gen1 has -Loud' $true (HasLoud)
Check 'gen1 shell is a new assembly' $true ($shell1 -ne $shell0)
Check 'gen1 binds the new parameter' 'HELLO, WORLD!' (Get-Greeting -Name World -Loud)
Check 'gen1 keeps the old path' 'Hello, World!' (Get-Greeting -Name World)
Check 'gen1 binds from the pipeline' 'HELLO, ADA! HELLO, BOB!' ((('Ada', 'Bob') | Get-Greeting -Loud) -join ' ')
Check 'gen1 makes its objects from its own shell' $shell1 (ClassShells)

# gen2: the same surface, a different body.
$gen2 = $gen1.Replace('HELLO, {}!', 'HEY, {}!')
if ($gen2 -eq $gen1) { throw 'the HEY patch matched nothing' }
[IO.File]::WriteAllText($lib, $gen2)

Build 'gen2'
Import-Module $psd1 -Force
$shell2 = ShellName
"gen2 $shell2"
Check 'gen2 keeps the assembly' $shell1 $shell2
Check 'gen2 runs the new body' 'HEY, WORLD!' (Get-Greeting -Name World -Loud)
Check 'gen2 keeps the old path' 'Hello, World!' (Get-Greeting -Name World)
Check 'gen2 makes its objects from its own shell' $shell2 (ClassShells)

if ($failures.Count -gt 0) {
    "hot-reload-verdict fail ($($failures.Count) of the checks above)"
    exit 1
}
'hot-reload-verdict pass'
exit 0
