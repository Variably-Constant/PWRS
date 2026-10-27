#Requires -Version 7.4
# Signs a module folder as the settings file Settings sets out: loads
# Pwrs.Signing from Assembly, sorts the folder's files, signs those to
# sign with Pwrs.Signing or, when the settings name OpenAuthenticode, with
# the OpenAuthenticode module at OpenAuthenticode, and makes the catalog
# when the settings set catalog. Each outcome is one line on standard
# error, as every line of cargo pwrs's own is. A file that cannot be
# signed is named there too, and the run exits 1 before any file changes.
param(
    [Parameter(Mandatory)] [string] $Settings,
    [Parameter(Mandatory)] [string] $Assembly,
    [string] $OpenAuthenticode = ''
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$null = [System.Reflection.Assembly]::LoadFrom($Assembly)

function Say([string] $Line) { [Console]::Error.WriteLine("pwrs: $Line") }

# A refusal from Pwrs.Signing reaches here wrapped in the exception that
# calling it raised; its own message is the one to show.
function Stop-Signing($Record) {
    $e = $Record.Exception
    while ($null -ne $e -and $e -isnot [Pwrs.Signing.SigningException]) { $e = $e.InnerException }
    if ($null -eq $e) { throw $Record }
    [Console]::Error.WriteLine("pwrs: $($e.Message)")
    exit 1
}

try {
    $plan = [Pwrs.Signing.Plan]::Prepare([System.IO.File]::ReadAllText($Settings))
} catch {
    Stop-Signing $_
}
try {
    $stamp = if ($plan.TimestampUrl) { "timestamped by $($plan.TimestampUrl)" } else { 'with no timestamp, so each signature stops being valid when the certificate expires' }
    Say "signing as $($plan.Subject), $($plan.Hash), $stamp"
    if ($plan.Refusals.Count -gt 0) {
        foreach ($r in $plan.Refusals) { [Console]::Error.WriteLine("pwrs: $r") }
        [Console]::Error.WriteLine("pwrs: nothing is signed while a file cannot be")
        exit 1
    }
    foreach ($line in $plan.Report) { Say $line }
    if ($plan.Signer -eq 'openauthenticode' -and $plan.ToSign.Count -gt 0) {
        Import-Module -Name $OpenAuthenticode
        $sign = @{
            LiteralPath = @(foreach ($f in $plan.ToSign) { Join-Path $plan.Folder $f.Item1 })
            Certificate = $plan.CertificateWithKey()
            HashAlgorithm = $plan.Hash
        }
        if ($plan.TimestampUrl) { $sign.TimeStampServer = $plan.TimestampUrl }
        Set-OpenAuthenticodeSignature @sign
        foreach ($f in $plan.ToSign) { Say $plan.Confirm($f.Item1, $f.Item2) }
    } else {
        foreach ($f in $plan.ToSign) { Say $plan.Sign($f.Item1, $f.Item2) }
    }
    $catalog = $plan.MakeCatalog()
    if ($catalog) { Say $catalog }
} catch {
    Stop-Signing $_
} finally {
    $plan.Dispose()
}
