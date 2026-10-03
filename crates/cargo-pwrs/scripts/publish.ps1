#Requires -Version 7
# Validates a built module and publishes it with PSResourceGet, or
# only packages it when DryRun is set.
param(
    [Parameter(Mandatory)] [string] $ModulePath,
    [Parameter(Mandatory)] [string] $OutDir,
    [switch] $DryRun,
    [string] $Repository = 'PSGallery'
)

$ErrorActionPreference = 'Stop'
$manifest = Get-ChildItem -Path $ModulePath -Filter *.psd1 | Select-Object -First 1
if ($null -eq $manifest) { throw "no .psd1 under $ModulePath" }
$null = Test-ModuleManifest -Path $manifest.FullName
$null = New-Item -ItemType Directory -Force -Path $OutDir

if ($DryRun) {
    Compress-PSResource -Path $ModulePath -DestinationPath $OutDir
    foreach ($p in Get-ChildItem -Path $OutDir -Filter *.nupkg) { Write-Output ("packaged " + $p.FullName) }
    exit 0
}

$key = $env:PWRS_PSGALLERY_KEY
if (-not $key) { throw 'PWRS_PSGALLERY_KEY is not set' }
Publish-PSResource -Path $ModulePath -Repository $Repository -ApiKey $key
Write-Output ("published " + $manifest.BaseName + " to " + $Repository)
