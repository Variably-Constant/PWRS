#Requires -Version 7
# Downloads one package, from nuget.org's v3 flat container or, when
# Source is gallery, from the PowerShell Gallery, extracts it into Dest,
# and prints the SHA-512 of the archive so the caller can record or
# verify it.
param(
    [Parameter(Mandatory)] [string] $Id,
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $Dest,
    [ValidateSet('nuget', 'gallery')] [string] $Source = 'nuget'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$lower = $Id.ToLowerInvariant()
$url = if ($Source -eq 'gallery') {
    "https://www.powershellgallery.com/api/v2/package/$Id/$Version"
} else {
    "https://api.nuget.org/v3-flatcontainer/$lower/$Version/$lower.$Version.nupkg"
}
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) "pwrs-$lower-$Version-$PID.zip"
Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
$hash = (Get-FileHash -Path $tmp -Algorithm SHA512).Hash.ToLowerInvariant()
if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
New-Item -ItemType Directory -Path $Dest -Force | Out-Null
Expand-Archive -Path $tmp -DestinationPath $Dest -Force
Remove-Item -Force $tmp
Write-Output $hash
