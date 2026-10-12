# What importing a built module costs, and which part of it is ours.
#
# The generated `.psm1` is the only PowerShell PWRS ships. It loads the
# bootstrap assembly, asks the loader for the shell, drops any earlier
# binary module holding the same file, imports the shell and exports the
# names. Everything else in an import belongs to the engine: reading the
# manifest, creating the module scope, compiling the script and
# `Export-ModuleMember`.
#
# The first import of a session pays assembly load and JIT that a later
# one does not, so the module is imported once before timing starts.
#
# Reduced by minimum across rounds: competing work only ever adds time.
param(
    [string] $Module = 'target/pwrs/Hello',
    [int] $Rounds = 30,
    # Build the same import up one layer at a time. Copies the module to
    # a temporary folder and edits the manifest there, so the built
    # module is untouched.
    [switch] $Layers
)
$ErrorActionPreference = 'Stop'

$dir = (Resolve-Path $Module).Path
$name = Split-Path $dir -Leaf
$psd1 = Join-Path $dir "$name.psd1"
$tfm = if ($PSVersionTable.PSEdition -eq 'Core') { 'net10.0' } else { 'netstandard2.0' }
Import-Module $psd1 -Force

# `$null =` and not `| Out-Null`: the latter is a cmdlet in a pipeline and
# would sit inside the timed region, adding its own invocation to every row.
function Measure-Best([string] $Label, [scriptblock] $Block) {
    $null = & $Block
    $best = [double]::MaxValue
    foreach ($r in 1..$Rounds) {
        [System.GC]::Collect()
        [System.GC]::WaitForPendingFinalizers()
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $null = & $Block
        $sw.Stop()
        if ($sw.Elapsed.TotalMilliseconds -lt $best) { $best = $sw.Elapsed.TotalMilliseconds }
    }
    "{0,-36} {1,8:N3} ms" -f $Label, $best
}

"$name on PowerShell $($PSVersionTable.PSVersion), $Rounds rounds, minimum"
""
Measure-Best 'Import-Module -Force, warm' { Import-Module $psd1 -Force }
Measure-Best '  Loader.Load' { $null = [Pwrs.Bootstrap.Loader]::Load($dir, $name, $tfm) }

$shell = [Pwrs.Bootstrap.Loader]::Load($dir, $name, $tfm)
$glob = "*$name.Shell*"
Measure-Best '  drop any earlier binary module' {
    foreach ($m in Get-Module -All) {
        if ($m.ModuleType -eq 'Binary' -and ($m.Path -eq $shell.Location -or $m.Name -like $glob)) {
            Remove-Module -ModuleInfo $m -Force -ErrorAction SilentlyContinue
        }
    }
}
Measure-Best '  Import-Module -Assembly' { Import-Module -Assembly $shell -Force }

""
"the two constructs the psm1 does not use:"
Measure-Best 'Get-Module | Where-Object' {
    $null = Get-Module -All | Where-Object { $_.ModuleType -eq 'Binary' -and ($_.Path -eq $shell.Location -or $_.Name -like $glob) }
}
Measure-Best 'Join-Path, nested' { $null = Join-Path (Join-Path $dir $tfm) 'Pwrs.Bootstrap.dll' }
Measure-Best '[IO.Path]::Combine' { $null = [System.IO.Path]::Combine($dir, $tfm, 'Pwrs.Bootstrap.dll') }

if (-not $Layers) { return }

# The same import one layer at a time, against a copy: the assembly on
# its own, wrapped in the script module, with the manifest, and with the
# manifest's format file.
# The copy exports the same cmdlet names, so the original has to go
# first or every import below collides on them.
Remove-Module $name -Force -ErrorAction SilentlyContinue

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("pwrs_layers_" + [DateTime]::Now.Ticks)
$null = New-Item -ItemType Directory -Path $work
foreach ($i in Get-ChildItem $dir) { Copy-Item $i.FullName -Destination $work -Recurse }

$copyPsd1 = Join-Path $work "$name.psd1"
$copyPsm1 = Join-Path $work "$name.psm1"
# The shell's name carries a stamp of the source it was built from.
$copyDll = (Get-ChildItem (Join-Path $work $tfm) -Filter "$name.Shell.*.dll" | Select-Object -First 1).FullName
$full = Get-Content $copyPsd1 -Raw

""
"one layer at a time:"
Measure-Best 'the shell assembly on its own' { Import-Module $copyDll -Force }
Measure-Best 'wrapped in the script module' { Import-Module $copyPsm1 -Force }
[System.IO.File]::WriteAllText($copyPsd1, ($full -replace "(?m)^\s*FormatsToProcess.*\r?\n", ''))
Measure-Best 'with the manifest' { Import-Module $copyPsd1 -Force }
[System.IO.File]::WriteAllText($copyPsd1, $full)
Measure-Best 'with FormatsToProcess' { Import-Module $copyPsd1 -Force }

# The format cost is fixed, not proportional: prove it with no views.
$fmt = Join-Path $work "$name.Format.ps1xml"
if (Test-Path $fmt) {
    [System.IO.File]::WriteAllText($fmt, "<?xml version=`"1.0`" encoding=`"utf-8`"?>`n<Configuration>`n  <ViewDefinitions>`n  </ViewDefinitions>`n</Configuration>`n")
    Measure-Best '  the same, with zero views defined' { Import-Module $copyPsd1 -Force }
}
"`nleft in place, the assemblies are loaded: $work"
