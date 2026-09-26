#Requires -Version 7
# Runs csc.dll from the fetched compiler toolset inside this pwsh
# process. The compiler and its own dependencies load through an
# isolated AssemblyLoadContext so they never collide with the Roslyn
# that pwsh itself ships.
param(
    [Parameter(Mandatory)] [string] $CscDir,
    [Parameter(Mandatory)] [string] $Arguments
)

$ErrorActionPreference = 'Stop'

if (-not ('Pwrs.Build.CscContext' -as [type])) {
    Add-Type -TypeDefinition @'
namespace Pwrs.Build
{
    public sealed class CscContext : System.Runtime.Loader.AssemblyLoadContext
    {
        private readonly string _dir;
        public CscContext(string dir) : base("pwrs-csc", isCollectible: false) { _dir = dir; }
        protected override System.Reflection.Assembly Load(System.Reflection.AssemblyName name)
        {
            string path = System.IO.Path.Combine(_dir, name.Name + ".dll");
            return System.IO.File.Exists(path) ? LoadFromAssemblyPath(path) : null;
        }
    }
}
'@
}

$ctx = [Pwrs.Build.CscContext]::new($CscDir)
$csc = $ctx.LoadFromAssemblyPath((Join-Path $CscDir 'csc.dll'))
$entry = $csc.EntryPoint
if ($null -eq $entry) {
    # Almost always the compiler needing a newer runtime than this
    # pwsh has, rather than a damaged download, so the two versions
    # are named here. Set PWRS_TOOLSET to a toolset built for the
    # runtime this host does have.
    $cfg = Join-Path $CscDir 'csc.runtimeconfig.json'
    $wants = if (Test-Path $cfg) {
        $f = (Get-Content $cfg -Raw | ConvertFrom-Json).runtimeOptions.framework
        "$($f.name) $($f.version)"
    } else { 'unknown' }
    throw "csc.dll in $CscDir exposed no entry point. It is built for $wants; this pwsh runs on .NET $([Environment]::Version). Set PWRS_TOOLSET to a Microsoft.Net.Compilers.Toolset version built for that runtime."
}
$exit = $entry.Invoke($null, @(, [string[]] ($Arguments -split ' ', 2)))
exit [int] $exit
